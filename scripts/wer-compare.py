#!/usr/bin/env python3
"""Compare the word error rate (WER) of two or more builds on the same recordings.

Usage: wer-compare.py REF_DIR NAME=HYP_DIR [NAME=HYP_DIR ...]

REF_DIR holds one reference transcript per recording, <id>.txt (plain text or WebVTT). Each HYP_DIR
holds the transcript of every <id>, also <id>.txt. The first NAME is the baseline of the comparisons.
Prints Markdown tables: per recording, aggregate, and each build against the baseline, then one
VERDICT line per build. Exits 1 when a hypothesis file is missing.

Normalizer (applied to reference and hypothesis alike; same as scribe-model-bench/wer.py):
1. WebVTT: drop the header, cue timings and cue numbers; markdown: drop [hh:mm:ss] stamps and '#' headings.
2. Drop sound annotations in () or [] such as (Laughter), (Applause), [music]; drop music notes.
3. Lowercase. '%' -> ' percent', '&' -> ' and ', '$N' -> 'N dollars'. Hyphens, slashes -> space.
4. Numbers -> words: integers up to 999,999,999,999, decimals read digit by digit after 'point',
   ordinals 1st/2nd/3rd/Nth -> words, years 1100-1999 and 2010-2099 as pairs ('1984' -> 'nineteen eighty four').
5. Keep [a-z0-9'] tokens; strip leading and trailing apostrophes; drop fillers um uh hmm mm ah er erm mhm.

Noise bound: for two builds, the paired sign-test bound 2*sqrt(c)/N, where c is the number of
hypothesis words that differ between them (difflib opcodes) and N the reference words. Under the null
hypothesis that a change is equally likely to fix or add an error, each changed word moves the error
count by at most 1 with symmetric sign, so the error-count difference has standard deviation at most sqrt(c).
"""
import difflib
import pathlib
import re
import sys

FILL = {"um", "uh", "hmm", "mm", "ah", "er", "erm", "mhm"}
ONES = "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen".split()
TENS = "_ _ twenty thirty forty fifty sixty seventy eighty ninety".split()
ORD = {"one": "first", "two": "second", "three": "third", "five": "fifth", "eight": "eighth", "nine": "ninth", "twelve": "twelfth"}


def words(n):
    if n < 20:
        return ONES[n]
    if n < 100:
        return TENS[n // 10] + ("" if n % 10 == 0 else " " + ONES[n % 10])
    if n < 1000:
        return ONES[n // 100] + " hundred" + ("" if n % 100 == 0 else " " + words(n % 100))
    for size, name in ((10**9, "billion"), (10**6, "million"), (1000, "thousand")):
        if n >= size:
            rest = n % size
            return words(n // size) + " " + name + ("" if rest == 0 else " " + words(rest))
    return str(n)


def number(m):
    raw, frac, suffix = m.group(1).replace(",", ""), m.group(2), (m.group(3) or "")
    n = int(raw)
    if not frac and not suffix and len(raw) == 4 and (1100 <= n <= 1999 or 2010 <= n <= 2099):
        text = words(n // 100) + " " + ("hundred" if n % 100 == 0 else ("oh " if n % 100 < 10 else "") + words(n % 100))
    else:
        text = words(n) if n < 10**12 else raw
    if frac:
        text += " point " + " ".join(ONES[int(d)] for d in frac[1:])
    if suffix:
        last = text.split()[-1]
        o = ORD.get(last) or (last[:-1] + "ieth" if last.endswith("y") else last + "th")
        text = " ".join(text.split()[:-1] + [o])
    return " " + text + " "


def norm(t):
    if t.startswith("WEBVTT"):
        t = "\n".join(l for l in t.splitlines()[1:] if "-->" not in l and not re.fullmatch(r"\s*(\d+|Kind:.*|Language:.*)\s*", l))
    t = re.sub(r"^#.*$", " ", t, flags=re.M)
    t = re.sub(r"\[\d+:\d+(:\d+)?\]", " ", t)
    t = re.sub(r"\([^)]*\)|\[[^\]]*\]|[♪♫]", " ", t)
    t = t.lower().replace("%", " percent").replace("&", " and ")
    t = re.sub(r"\$(\d[\d,]*(?:\.\d+)?)", r"\1 dollars", t)
    t = re.sub(r"[-–—/]", " ", t)
    t = re.sub(r"(?<![\w.])(\d{1,3}(?:,\d{3})+|\d+)(\.\d+)?(st|nd|rd|th)?\b", number, t)
    out = []
    for w in re.findall(r"[a-z0-9']+", t):
        w = w.strip("'")
        if w and w not in FILL:
            out.append(w)
    return out


def align(r, h):
    """Levenshtein alignment: (errors, (substitutions, deletions, insertions))."""
    n, m = len(r), len(h)
    prev = list(range(m + 1))
    ops = [(0, 0, j) for j in range(m + 1)]
    for i in range(1, n + 1):
        cur = [i] + [0] * m
        cops = [(0, i, 0)] + [None] * m
        for j in range(1, m + 1):
            c = [(prev[j - 1] + (r[i - 1] != h[j - 1]), "s"), (prev[j] + 1, "d"), (cur[j - 1] + 1, "i")]
            v, k = min(c)
            cur[j] = v
            s, d, ins = ops[j - 1] if k == "s" else ops[j] if k == "d" else cops[j - 1]
            cops[j] = (s + (k == "s" and r[i - 1] != h[j - 1]), d + (k == "d"), ins + (k == "i"))
        prev, ops = cur, cops
    return prev[m], ops[m]


def changed_words(a, b):
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    return sum(max(i2 - i1, j2 - j1) for op, i1, i2, j1, j2 in sm.get_opcodes() if op != "equal")


def main(argv):
    if len(argv) < 3 or any("=" not in a for a in argv[2:]):
        sys.exit(__doc__.split("\n\n")[1])
    ref_dir = pathlib.Path(argv[1])
    builds = [tuple(a.split("=", 1)) for a in argv[2:]]
    ids = sorted(p.stem for p in ref_dir.glob("*.txt"))
    missing = [f"{d}/{i}.txt" for _, d in builds for i in ids if not (pathlib.Path(d) / f"{i}.txt").is_file()]
    if not ids or missing:
        sys.exit(f"no reference files in {ref_dir}" if not ids else "missing hypothesis files: " + " ".join(missing))
    ref = {i: norm((ref_dir / f"{i}.txt").read_text()) for i in ids}
    res = {}
    print("| Build | Recording | Ref words | WER % | Sub | Del | Ins |")
    print("| --- | --- | ---: | ---: | ---: | ---: | ---: |")
    for name, d in builds:
        for i in ids:
            h = norm((pathlib.Path(d) / f"{i}.txt").read_text())
            e, (s, dl, ins) = align(ref[i], h)
            res[(name, i)] = dict(n=len(ref[i]), e=e, s=s, d=dl, i=ins, h=h)
            print(f"| {name} | {i} | {len(ref[i])} | {100 * e / len(ref[i]):.2f} | {s} | {dl} | {ins} |")
    print()
    print("| Build | Recordings | Ref words | WER % | Sub | Del | Ins | Errors |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    n = sum(len(ref[i]) for i in ids)
    for name, _ in builds:
        xs = [res[(name, i)] for i in ids]
        e = sum(x["e"] for x in xs)
        print(f"| {name} | {len(ids)} | {n} | {100 * e / n:.2f} | {sum(x['s'] for x in xs)} | {sum(x['d'] for x in xs)} | {sum(x['i'] for x in xs)} | {e} |")
    print()
    base = builds[0][0]
    print(f"| Build vs {base} | Recordings | Changed hyp words | Error delta | WER delta (points) | Noise bound 2*sqrt(c)/N (points) | Verdict |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | --- |")
    verdicts = []
    for name, _ in builds[1:]:
        c = sum(changed_words(res[(base, i)]["h"], res[(name, i)]["h"]) for i in ids)
        de = sum(res[(name, i)]["e"] - res[(base, i)]["e"] for i in ids)
        bound = 200 * c**0.5 / n
        if de <= 0:
            verdict = "not worse"
        elif 100 * de / n <= bound:
            verdict = "within noise"
        else:
            verdict = "WORSE than noise"
        print(f"| {name} | {len(ids)} | {c} | {de:+d} | {100 * de / n:+.3f} | {bound:.3f} | {verdict} |")
        ok = verdict != "WORSE than noise"
        verdicts.append(
            f"VERDICT: {name} aggregate WER is {'no worse than' if ok else 'WORSE than'} {base} beyond noise "
            f"({100 * de / n:+.3f} points vs bound {bound:.3f}; {verdict})."
        )
    print()
    for v in verdicts:
        print(v)


if __name__ == "__main__":
    main(sys.argv)
