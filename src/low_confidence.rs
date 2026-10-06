//! Low-confidence passages: spans of the transcript that the speech engine scored as doubtful,
//! so the extraction helper checks them before it quotes them (ADR 0009).

use crate::engine::Word;

/// A word scores as doubtful below this confidence. Calibrated on the 38-minute field run
/// 20260921-lauren-poteto (5,660 words, parakeet-ultra Q8_0): at 0.92 the passages hold 22 of
/// 31 known mishearings (all 17 forms of the product name) and flag 40 other words (0.7 %).
/// At 0.90 they hold 18 of 31 and flag 16 other words; at 0.93, 25 of 31 and 86 other words.
pub const THRESHOLD: f32 = 0.92;
/// Hesitation words are never doubtful: no lesson quotes them, and they made 13 of the 53
/// other words below the threshold on the field run.
const FILLERS: &[&str] = &["uh", "um", "ah", "er", "hmm", "mm"];
/// Two doubtful words with at most this many words between them form one passage, so a
/// misheard phrase ("bug bar") is one entry.
const MERGE_GAP_WORDS: usize = 2;
/// Words of context shown on each side of a passage.
const CONTEXT_WORDS: usize = 3;

/// True when the engine scored the word below `THRESHOLD` and it is not a hesitation word.
fn doubtful(word: &Word) -> bool {
    let bare = word
        .text
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    word.confidence.is_some_and(|p| p < THRESHOLD) && !FILLERS.contains(&bare.as_str())
}

/// One low-confidence passage: the doubtful words `first..=last` of the transcript.
pub struct Passage {
    pub first: usize,
    pub last: usize,
    /// The lowest word confidence in the passage.
    pub lowest: f32,
}

/// The low-confidence passages of `words`, in order. Words without a score never count.
/// `lowest` ignores hesitation words, as the passages do.
pub fn passages(words: &[Word]) -> Vec<Passage> {
    let mut passages: Vec<Passage> = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let Some(p) = word.confidence.filter(|_| doubtful(word)) else {
            continue;
        };
        match passages.last_mut() {
            Some(last) if i - last.last - 1 <= MERGE_GAP_WORDS => {
                last.last = i;
                last.lowest = last.lowest.min(p);
            }
            _ => passages.push(Passage {
                first: i,
                last: i,
                lowest: p,
            }),
        }
    }
    passages
}

/// The passage with a few words of context, doubtful words in bold. Every word of the passage
/// itself is shown, also a confident word between two doubtful ones.
pub fn excerpt(words: &[Word], passage: &Passage) -> String {
    let start = passage.first.saturating_sub(CONTEXT_WORDS);
    let end = (passage.last + CONTEXT_WORDS + 1).min(words.len());
    let mut out = Vec::new();
    for (i, word) in words.iter().enumerate().take(end).skip(start) {
        if (passage.first..=passage.last).contains(&i) && doubtful(word) {
            out.push(format!("**{}**", word.text));
        } else {
            out.push(word.text.clone());
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words one second apart, each with a score; a negative score means none.
    fn scored(words: &[(&str, f32)]) -> Vec<Word> {
        words
            .iter()
            .enumerate()
            .map(|(i, (text, p))| Word {
                start: i as f64,
                end: i as f64 + 1.0,
                text: (*text).into(),
                confidence: (*p >= 0.0).then_some(*p),
            })
            .collect()
    }

    #[test]
    fn doubtful_words_close_together_form_one_passage() {
        // Scores from the field run: "Grothbot," 0.82, "bug" 0.99 and "bar" 0.89.
        let words = scored(&[
            ("framework", 0.99),
            ("that", 0.99),
            ("powers", 0.99),
            ("Grothbot,", 0.82),
            ("we", 0.99),
            ("made", 0.99),
            ("rules", 0.99),
            ("or", 0.99),
            ("bug", 0.995),
            ("bar", 0.886),
            ("or", 0.99),
            ("skills", -1.0),
        ]);
        let passages = passages(&words);
        // "Grothbot," and "bar" have 5 words between them: two passages.
        assert_eq!(
            passages
                .iter()
                .map(|p| (p.first, p.last, p.lowest))
                .collect::<Vec<_>>(),
            [(3, 3, 0.82), (9, 9, 0.886)]
        );
        assert_eq!(
            excerpt(&words, &passages[0]),
            "framework that powers **Grothbot,** we made rules"
        );
        assert_eq!(
            excerpt(&words, &passages[1]),
            "rules or bug **bar** or skills"
        );
    }

    #[test]
    fn a_gap_of_two_words_merges_and_three_does_not() {
        let words = scored(&[
            ("ver", 0.87),
            ("uh", 0.95),
            ("cor", 0.95),
            ("verification", 0.89),
            ("is", 0.99),
            ("really", 0.99),
            ("hard", 0.99),
            ("today", 0.5),
        ]);
        let passages = passages(&words);
        assert_eq!(
            passages
                .iter()
                .map(|p| (p.first, p.last))
                .collect::<Vec<_>>(),
            [(0, 3), (7, 7)]
        );
        assert_eq!(passages[0].lowest, 0.87);
        assert_eq!(
            excerpt(&words, &passages[0]),
            "**ver** uh cor **verification** is really hard"
        );
    }

    #[test]
    fn hesitation_words_are_never_doubtful() {
        // Field run, 00:07:57: "you rely on uh formal methods", "uh" scored 0.86.
        let words = scored(&[("on", 0.99), ("uh", 0.86), ("Um,", 0.5), ("TLA", 0.91)]);
        let passages = passages(&words);
        assert_eq!(passages.len(), 1);
        assert_eq!((passages[0].first, passages[0].lowest), (3, 0.91));
        assert_eq!(excerpt(&words, &passages[0]), "on uh Um, **TLA**");
    }

    #[test]
    fn words_without_scores_give_no_passages() {
        assert!(passages(&scored(&[("a", -1.0), ("b", -1.0)])).is_empty());
        // The threshold itself is not doubtful.
        assert!(passages(&scored(&[("a", THRESHOLD)])).is_empty());
    }
}
