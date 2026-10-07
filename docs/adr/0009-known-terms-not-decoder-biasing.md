# Known terms and low-confidence passages, not decoder biasing

Parakeet mishears names: one 38-minute talk spelled one product name 7 ways and turned the handle "poteto" into "Potato". transcribe-cpp 0.3.1 has no name biasing for Parakeet (its vocabulary option works only for Whisper and Qwen3-ASR, and Parakeet ignores it with a warning), and upstream transcribe.cpp issue #126 asking for it is open. The only off-the-shelf alternative, sherpa-onnx hotwords, needs beam search and a different engine (ADR 0002).

Decision: the transcript stays as the engine heard it. `index.md` lists **known terms** taken from the source's own metadata (post text, title, uploader, description) and **low-confidence passages** taken from the engine's per-token scores. The extraction helper uses both to spell names right in the lessons and to flag doubtful quotes.

Issue 24 measured extra signals for four phrases that still score above 0.92. None catches the four without a large rise in false flags. This decision stays. No behavior changes.

## Considered options

- **Rewrite the transcript with fuzzy matches to known terms.** Rejected: a wrong match ("Potato" is a real word) silently corrupts the source text that every lesson cites.
- **Patch the transcribe.cpp greedy decoder to boost terms.** Rejected for now: boosting must run inside the decode loop, so it is a C++ fork until upstream merges it. Revisit when upstream adds biasing for Parakeet.
- **Flag transcript words that are one edit from a known term, or lower the 0.92 threshold.** Rejected after the measurement below.

## Issue 24 measurement

The four phrases are from the poteto talk (source `x:2102050467505430555`, 5,660 words, parakeet-ultra Q8_0). Word scores are the #4 calibration table. That table's words match the saved transcript of this post with 0 mismatches. A recheck of the published rule still gives 59 passages, 22 of 31 known mishearings, and 40 other words. "Other words" means words below the threshold, with hesitation words removed, that are not in those 31 spans.

| Heard | True form | How we know | Score | In metadata? |
| --- | --- | --- | ---: | --- |
| Potato (00:00:04); potato (00:37:48) | poteto | The uploader and the title handle are `poteto`. At 00:37:48 the speaker says "potato with an E". | 0.982 and 0.945 | Yes. `poteto` is already a known term. The list does not mark these two words. |
| slot pull requests (00:06:35) | slop | The original field report maps slot to slop. A reply on the post says "slop". The same transcript says "slop" correctly at 00:23:16 (score 0.971). | 0.999 | No. The post text, title, uploader, and description do not contain "slop". Replies are not fetched. "slop" is lowercase, so the name rule would drop it anyway. |
| heat snapshots (00:07:41) | heap snapshots | The original field report maps heat to heap. The same talk says "heap snapshots" at 00:03:35 (score 0.9996), in the same Chrome DevTools passage. | 0.976 | No. "heap" is not in the metadata. |
| Control Glass (00:08:45) | Unknown | The original field report lists the name as unclear. It is not in the post, the title, the handle, or the saved replies. Later words are "control skills". That does not give the spelling. | Control 0.922, Glass 0.981 | No true form is available to match. |

PStack (0.829) and PSnack (0.854) are already low-confidence passages. Their true spellings are also absent from the metadata. They are not part of this measurement.

TED false flags use the four Ultra transcripts from the ADR 0008 bench (1,666, 2,263, 2,310, and 3,054 engine words) and the YouTube title, uploader, and description fetched for those URLs. The bench libraries stored the file name as the title, so they are not the metadata under test. Every TED flag below also appears in the human captions. The flag is a real word in the talk, not an engine-only mishearing.

| Candidate | Hits on the four phrases | False flags on the poteto talk | False flags on the 4 TED talks |
| --- | --- | ---: | ---: |
| Known terms, as shipped | Lists `poteto` only. Does not mark slot, heat, or Control Glass. | 0 new flags | Not a new flag. |
| Edit distance 1, both lengths at least 5, and neither word is a prefix of the other | Potato, both times. Misses the other three. | 0 | 1. "either" matches Esther. The captions also say "either". |
| Edit distance 1, both lengths at least 4 | Potato, both times. Also misses the other three. | 2. "compiler" matches compile. "grow" matches grok. | 26. Examples: want/Wait, kind/Mind, book/Look, fond/Find. All 26 are in the captions. |
| Soundex, length at least 4 | Potato, both times. Also matches Glass to Galaxy, which is the wrong name. | 4 besides Potato: Glass, gloss, compiler, complicated | 137, all in the captions |
| Edit distance 1 against every post-text word of length at least 4, not only names | None of the four. "slop" and "heap" are not in the post. | 81, mostly here/where and make/take | Not run. The field count is already a flood. |
| Same rule against the saved reply page | slot matches slop. Potato matches poteto. Misses heat and Control Glass. | 496, including stop/slop and slow/slop | Not run. |
| One-edit pairs that share a content neighbor | heat/heap, because both precede "snapshots". Misses slot (its neighbor is "pull"; "slop" has different neighbors), Potato, and Control Glass. | 8 other pairs, such as team/term and would/could | 8 pairs, such as factors/actors and like/live. No name fix. |
| Threshold 0.923 | Control only. Glass is context, not a doubtful word. | 46 other words, up from 40. Passages 66, up from 59. The new words include Cursor, which is correct. | No per-word scores in the bench transcripts. |
| Threshold 0.9816 | Both Potato words, Control Glass, and heat. Misses slot. | 288 other words. 253 passages. | Same limit. |
| Threshold 0.9994 | All four | 743 other words. 440 passages. | Same limit. |

The 0.93 row from #4 still holds: 25 of 31 mishearings and 86 other words. Slot scores 0.9993. The first measured threshold that includes it is 0.9994.

No candidate wins. A fuzzy link can mark Potato, and the skill text already tells the helper to read `poteto` for that word. The same link cannot see slot, heat, or Control Glass, because those true forms are not known terms. A threshold that reaches slot flags most of the talk. The 0.92 passages and the known-term list stay as they are.
