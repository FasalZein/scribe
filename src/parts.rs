use crate::{engine::Segment, sources::Chapter};

pub const PART_WORDS: usize = 2_500;
/// Approximate English token count. This is a reading-size estimate, not a tokenizer.
pub const TOKENS_PER_WORD: f64 = 1.33;

pub struct Part<'a> {
    pub segments: &'a [Segment],
    pub chapter: Option<&'a Chapter>,
    /// Letter of this piece when a chapter is longer than one part; None otherwise.
    pub piece: Option<char>,
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

pub fn split<'a>(segments: &'a [Segment], chapters: &'a [Chapter]) -> Vec<Part<'a>> {
    // Engine segments are whole and can straddle a chapter boundary. Assign each one to the
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
        if i > start
            && (words >= PART_WORDS
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
    // Letter the pieces of each chapter that needed more than one part.
    let same = |a: &Part, b: &Part| match (a.chapter, b.chapter) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        _ => false,
    };
    let mut first = 0;
    for i in 1..=parts.len() {
        if i == parts.len() || !same(&parts[first], &parts[i]) {
            if i - first > 1 {
                for (n, part) in parts[first..i].iter_mut().enumerate() {
                    part.piece = char::from_u32('a' as u32 + n as u32);
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
    fn segment(start: f64, words: usize) -> Segment {
        Segment {
            start,
            end: start + 10.0,
            text: vec!["word"; words].join(" "),
        }
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
            [None, Some('a'), Some('b'), None]
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
}
