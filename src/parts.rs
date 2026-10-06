use crate::{engine::Word, sources::Chapter};
use serde::Serialize;

pub const PART_WORDS: usize = 2_500;
/// A part with no true sentence end after `PART_WORDS` ends at the next segment once it reaches
/// this size. Only a transcript without punctuation gets that far.
const PART_WORDS_MAX: usize = 3_000;
/// Approximate English token count. This is a reading-size estimate, not a tokenizer.
pub const TOKENS_PER_WORD: f64 = 1.33;
/// A segment ends at the first true sentence end once it spans this many seconds.
const SEGMENT_SECS: f64 = 20.0;
/// A segment with no true sentence end ends before the word that would make it longer than this.
const SEGMENT_MAX_SECS: f64 = 60.0;

/// One paragraph of the transcript: a line of segments.jsonl and of transcript.md. It starts at
/// its first word, so its timestamp marks the word it cites.
#[derive(Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub struct Part<'a> {
    pub segments: &'a [Segment],
    pub chapter: Option<&'a Chapter>,
    /// Number of this piece, from 1, when a chapter is longer than one part; None otherwise.
    pub piece: Option<usize>,
}
impl Part<'_> {
    pub fn words(&self) -> usize {
        self.segments
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum()
    }
}
pub fn tokens_estimate(words: usize) -> usize {
    (words as f64 * TOKENS_PER_WORD).round() as usize
}

/// True when the text `before` ends a sentence and the text `after` starts the next one.
/// The engine often writes a full stop at a pause inside a sentence, and then goes on in
/// lowercase. So terminal punctuation followed by a lowercase word is not a sentence end.
fn sentence_end(before: &str, after: &str) -> bool {
    let terminal = before
        .trim_end()
        .trim_end_matches(['"', '\'', ')', '\u{201d}', '\u{2019}'])
        .ends_with(['.', '?', '!', '\u{2026}']);
    let lowercase = after
        .chars()
        .find(|c| c.is_alphanumeric())
        .is_some_and(char::is_lowercase);
    terminal && !lowercase
}

/// Group words into segments that start at a true sentence start (see `SEGMENT_SECS`). Chunk
/// cuts often fall inside a sentence (ADR 0013), so segments ignore them.
pub fn segments(words: &[Word]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut first = 0;
    for i in 1..=words.len() {
        let close = i == words.len() || {
            let start = words[first].start;
            (words[i - 1].end - start >= SEGMENT_SECS
                && sentence_end(&words[i - 1].text, &words[i].text))
                || words[i].end - start > SEGMENT_MAX_SECS
        };
        if close {
            let text: Vec<&str> = words[first..i].iter().map(|w| w.text.as_str()).collect();
            segments.push(Segment {
                start: words[first].start,
                end: words[i - 1].end,
                text: text.join(" "),
            });
            first = i;
        }
    }
    segments
}

pub fn split<'a>(segments: &'a [Segment], chapters: &'a [Chapter]) -> Vec<Part<'a>> {
    // Segments are whole and can straddle a chapter boundary. Assign each one to the
    // chapter that holds its midpoint, which is the chapter that holds most of its time.
    let chapter_at = |segment: &Segment| {
        let midpoint = (segment.start + segment.end) / 2.0;
        chapters
            .iter()
            .filter(|c| c.start_time <= midpoint)
            .max_by(|a, b| a.start_time.total_cmp(&b.start_time))
    };
    let mut parts = Vec::new();
    let mut start = 0;
    let mut words = 0;
    let mut chapter = segments.first().and_then(chapter_at);
    for (i, segment) in segments.iter().enumerate() {
        let next_chapter = chapter_at(segment);
        // Near the size limit, cut only where a sentence starts. A chapter start cuts anyway.
        let full = words >= PART_WORDS
            && (words >= PART_WORDS_MAX || sentence_end(&segments[i - 1].text, &segment.text));
        if i > start
            && (full
                || chapter.map(|c| c as *const Chapter)
                    != next_chapter.map(|c| c as *const Chapter))
        {
            parts.push(Part {
                segments: &segments[start..i],
                chapter,
                piece: None,
            });
            start = i;
            words = 0;
        }
        chapter = next_chapter;
        words += segment.text.split_whitespace().count();
    }
    if start < segments.len() {
        parts.push(Part {
            segments: &segments[start..],
            chapter,
            piece: None,
        });
    }
    // Number the pieces of each chapter that needed more than one part.
    let same = |a: &Part, b: &Part| match (a.chapter, b.chapter) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        _ => false,
    };
    let mut first = 0;
    for i in 1..=parts.len() {
        if i == parts.len() || !same(&parts[first], &parts[i]) {
            if i - first > 1 {
                for (n, part) in parts[first..i].iter_mut().enumerate() {
                    part.piece = Some(n + 1);
                }
            }
            first = i;
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A segment of `words` words that is one whole sentence.
    fn segment(start: f64, words: usize) -> Segment {
        let mut text = vec!["word"; words];
        text[0] = "Word";
        Segment {
            start,
            end: start + 10.0,
            text: text.join(" ") + ".",
        }
    }
    /// Words spoken one per `step` seconds from `start`, as a word-timed engine returns them.
    fn timed(text: &str, start: f64, step: f64) -> Vec<Word> {
        text.split_whitespace()
            .enumerate()
            .map(|(i, text)| Word {
                start: start + i as f64 * step,
                end: start + (i + 1) as f64 * step,
                text: text.to_owned(),
            })
            .collect()
    }
    /// Sentences of ten words, `count` of them, one word per 0.4 s.
    fn sentences(count: usize, start: f64) -> Vec<Word> {
        let sentence = "We talk about the build and the tests again today.";
        let text = vec![sentence; count].join(" ");
        timed(&text, start, 0.4)
    }
    #[test]
    fn empty_and_short_transcripts() {
        assert!(split(&[], &[]).is_empty());
        let segments = [segment(0.0, 20)];
        let parts = split(&segments, &[]);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].words(), 20);
        assert_eq!(tokens_estimate(20), 27);
    }
    #[test]
    fn splits_near_limit_at_segment_boundaries_without_loss() {
        let segments = [segment(0.0, 1200), segment(10.0, 1400), segment(20.0, 10)];
        let parts = split(&segments, &[]);
        assert_eq!(
            parts.iter().map(Part::words).collect::<Vec<_>>(),
            [2600, 10]
        );
        let actual: Vec<_> = parts.iter().flat_map(|p| p.segments.iter()).collect();
        assert_eq!(actual.len(), segments.len());
        for (a, b) in actual.iter().zip(&segments) {
            assert!(std::ptr::eq(*a, b));
        }
    }

    #[test]
    fn respects_chapters_and_splits_oversized_chapters() {
        let segments = [
            segment(0.0, 5),
            segment(10.0, 1500),
            segment(20.0, 1500),
            segment(30.0, 100),
            segment(40.0, 5),
        ];
        let chapters = [
            Chapter {
                start_time: 10.0,
                title: "Long".into(),
            },
            Chapter {
                start_time: 40.0,
                title: "End".into(),
            },
        ];
        let parts = split(&segments, &chapters);
        assert_eq!(
            parts.iter().map(Part::words).collect::<Vec<_>>(),
            [5, 3000, 100, 5]
        );
        assert_eq!(
            parts
                .iter()
                .map(|p| p.chapter.map(|c| c.title.as_str()))
                .collect::<Vec<_>>(),
            [None, Some("Long"), Some("Long"), Some("End")]
        );
        assert_eq!(
            parts.iter().map(|p| p.piece).collect::<Vec<_>>(),
            [None, Some(1), Some(2), None]
        );
        let actual: Vec<_> = parts.iter().flat_map(|p| p.segments.iter()).collect();
        for (a, b) in actual.iter().zip(&segments) {
            assert!(std::ptr::eq(*a, b));
        }
        assert_eq!(actual.len(), segments.len());
        assert_eq!(parts.iter().map(Part::words).sum::<usize>(), 3110);
    }
    #[test]
    fn segment_belongs_to_the_chapter_holding_its_midpoint() {
        // Real shape from UNP03fDSj1U: a segment starts 0.8 s before chapter "Day 31".
        let segments = [
            Segment {
                start: 110.8,
                end: 169.7,
                text: "novel".into(),
            },
            Segment {
                start: 170.2,
                end: 197.2,
                text: "day thirty one".into(),
            },
        ];
        let chapters = [
            Chapter {
                start_time: 99.0,
                title: "Write a Novel".into(),
            },
            Chapter {
                start_time: 171.0,
                title: "Day 31".into(),
            },
        ];
        let parts = split(&segments, &chapters);
        assert_eq!(
            parts
                .iter()
                .map(|p| p.chapter.map(|c| c.title.as_str()))
                .collect::<Vec<_>>(),
            [Some("Write a Novel"), Some("Day 31")]
        );
    }

    #[test]
    fn segments_start_at_a_sentence_start_not_at_a_chunk_cut() {
        // A 30 s chunk cut fell inside "and then we ship it". The words carry their own times.
        let mut words = timed("So we measure first. Then we cut the code", 0.0, 2.5);
        words.extend(timed("and then we ship it. After that we rest.", 22.5, 2.5));
        let segments = segments(&words);
        let starts: Vec<_> = segments
            .iter()
            .map(|s| (s.start, s.text.as_str()))
            .collect();
        // Segment one reaches 20 s at "ship it." (ends at 35 s); "After" starts at 35 s.
        assert_eq!(
            starts,
            [
                (
                    0.0,
                    "So we measure first. Then we cut the code and then we ship it."
                ),
                (35.0, "After that we rest.")
            ]
        );
        assert_eq!(segments[1].end, 45.0);
    }
    #[test]
    fn a_full_stop_before_a_lowercase_word_is_not_a_cut() {
        let mut words = sentences(250, 0.0);
        // A false full stop, as the engine writes at a pause: "rule. or maybe".
        let tail = "you might forget to read a rule. or maybe the user ignores them. \
                    So these are not enforceable.";
        words.extend(timed(tail, 1000.0, 0.4));
        let segments = segments(&words);
        assert!(segments.iter().all(|s| !s.text.starts_with("or maybe")));
        let parts = split(&segments, &[]);
        assert_eq!(parts.len(), 2);
        assert!(parts[1].segments[0].text.starts_with("So these"));
        assert!(
            parts[0]
                .segments
                .last()
                .unwrap()
                .text
                .ends_with("ignores them.")
        );
    }
    /// Field run 20260921-lauren-poteto: part 02 started "forget to read a rule or maybe ...",
    /// because the part cut fell on a segment that started at a chunk cut inside a sentence.
    /// The segments below are the field run's 41-45 (one per chunk), each shortened to its
    /// first and last words, after 2,480 words of whole sentences.
    #[test]
    fn no_part_starts_mid_sentence_at_a_chunk_cut() {
        let field = [
            (
                1025.603,
                1052.803,
                "And then a step above that, right, where uh where and this is where \
                 a chance that it might, for various reasons, you know,",
            ),
            (
                1053.078,
                1082.118,
                "forget to read a rule or maybe the user that is piloting the agent \
                 put the put these in your rules and and bug",
            ),
            (
                1082.719,
                1107.599,
                "and skills as well. But if you don't, then you have this big glaring \
                 you know, relying only on the style guide. I think",
            ),
            (
                1108.069,
                1135.669,
                "The style guide or you know like looking at human reviews is a good \
                 course you layer that with rules and bugba and skills.",
            ),
            (
                1137.325,
                1165.644,
                "On the codebase front, and in the Grokbot codebase, we actually have invested into \
                 And so a lot of lessons came out of that.",
            ),
        ];
        let mut segments = vec![segment(0.0, 2_480)];
        segments.extend(field.map(|(start, end, text)| Segment {
            start,
            end,
            text: text.into(),
        }));
        let parts = split(&segments, &[]);
        assert_eq!(parts.len(), 2);
        // The first true sentence end at a segment boundary after 2,500 words: "skills." | "On".
        assert!(
            parts[1].segments[0]
                .text
                .starts_with("On the codebase front"),
            "part 2 starts mid-sentence: {}",
            parts[1].segments[0].text
        );
    }
    fn split_words(words: &[Word]) -> Vec<String> {
        let segments = segments(words);
        split(&segments, &[])
            .iter()
            .map(|p| {
                p.segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
    #[test]
    fn a_transcript_without_punctuation_still_splits() {
        let words = timed(&vec!["word"; 7_000].join(" "), 0.0, 0.4);
        let parts = split_words(&words);
        assert!(parts.len() > 1);
        assert!(
            parts
                .iter()
                .all(|p| p.split_whitespace().count() <= PART_WORDS_MAX + 150)
        );
        assert_eq!(
            parts
                .iter()
                .map(|p| p.split_whitespace().count())
                .sum::<usize>(),
            7_000
        );
    }
}
