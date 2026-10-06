//! Known terms: names, handles and product names taken from a source's own metadata, so the
//! extraction helper can spell misheard words right in lessons (ADR 0009). The transcript keeps
//! the words as the engine heard them.

use crate::{fetch::Metadata, sources::Sources};

/// A long description (sponsor reads, link lists) must not bury the names from the uploader,
/// title and post text, which come first.
const MAX_TERMS: usize = 40;

/// Words that start a sentence or a title in upper case but are not names. Compared in lower
/// case, with a typographic apostrophe read as `'`.
const COMMON: &[&str] = &[
    "a",
    "about",
    "after",
    "again",
    "all",
    "also",
    "am",
    "an",
    "and",
    "any",
    "are",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "best",
    "but",
    "by",
    "can",
    "check",
    "could",
    "day",
    "did",
    "do",
    "does",
    "don't",
    "each",
    "episode",
    "every",
    "first",
    "follow",
    "for",
    "from",
    "full",
    "get",
    "go",
    "good",
    "great",
    "had",
    "has",
    "have",
    "he",
    "hello",
    "her",
    "here",
    "here's",
    "hey",
    "hi",
    "his",
    "how",
    "i",
    "i'd",
    "i'll",
    "i'm",
    "i've",
    "if",
    "in",
    "into",
    "is",
    "it",
    "it's",
    "its",
    "join",
    "just",
    "last",
    "learn",
    "let's",
    "like",
    "live",
    "made",
    "make",
    "many",
    "me",
    "more",
    "most",
    "my",
    "new",
    "next",
    "no",
    "not",
    "now",
    "of",
    "off",
    "ok",
    "okay",
    "on",
    "one",
    "only",
    "or",
    "our",
    "out",
    "over",
    "part",
    "please",
    "read",
    "see",
    "she",
    "so",
    "some",
    "subscribe",
    "that",
    "that's",
    "the",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "to",
    "today",
    "too",
    "two",
    "up",
    "us",
    "very",
    "was",
    "watch",
    "we",
    "we're",
    "well",
    "were",
    "what",
    "what's",
    "when",
    "where",
    "which",
    "while",
    "who",
    "why",
    "will",
    "with",
    "would",
    "yes",
    "you",
    "you're",
    "your",
];

/// The known terms of a source, in order of first appearance: the uploader, then names in the
/// title, the post text, quoted posts and the description. A term is an @handle, or a run of
/// capitalized or CamelCase words without punctuation between them ("Cursor Compile").
/// Duplicates differ only in case or a leading `@`.
pub fn extract(meta: &Metadata) -> Vec<String> {
    let mut texts: Vec<&str> = vec![&meta.title];
    if let Sources::X {
        text, quoted_posts, ..
    } = &meta.sources
    {
        texts.push(text);
        texts.extend(quoted_posts.iter().map(|quote| quote.text.as_str()));
    }
    texts.extend(meta.description.as_deref());
    let mut terms: Vec<String> = meta.uploader.iter().cloned().collect();
    for text in texts {
        terms.extend(names(text));
    }
    let mut seen = std::collections::HashSet::new();
    terms.retain(|term| seen.insert(term.trim_start_matches('@').to_lowercase()));
    terms.truncate(MAX_TERMS);
    terms
}

/// The handles and capitalized names of one text.
fn names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    for raw in text.split_whitespace() {
        if raw.contains("://") || raw.starts_with("www.") || raw.contains('#') {
            flush(&mut run, &mut names);
            continue;
        }
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '@');
        let word = word
            .strip_suffix("'s")
            .or_else(|| word.strip_suffix("\u{2019}s"))
            .unwrap_or(word);
        if let Some(handle) = word.strip_prefix('@') {
            flush(&mut run, &mut names);
            if !handle.is_empty() && handle.chars().all(|c| c.is_alphanumeric() || c == '_') {
                names.push(format!("@{handle}"));
            }
            continue;
        }
        if is_name(word) {
            run.push(word);
        } else {
            flush(&mut run, &mut names);
        }
        // Punctuation after a word ends the name: "London. Watch" is two sentences.
        if raw.ends_with(|c: char| !c.is_alphanumeric()) {
            flush(&mut run, &mut names);
        }
    }
    flush(&mut run, &mut names);
    names
}

fn flush(run: &mut Vec<&str>, names: &mut Vec<String>) {
    if !run.is_empty() {
        names.push(run.join(" "));
        run.clear();
    }
}

/// A word that starts in upper case or has an upper-case letter inside (iPhone, macOS), has at
/// least two characters, contains a letter, and is not a common word.
fn is_name(word: &str) -> bool {
    word.chars().count() >= 2
        && word.chars().any(char::is_uppercase)
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || "'\u{2019}.-".contains(c))
        && !COMMON.contains(&word.replace('\u{2019}', "'").to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{QuotedPost, Sources};

    fn x_meta(title: &str, uploader: &str, text: &str) -> Metadata {
        Metadata {
            id: "x:1".into(),
            title: title.into(),
            source: "https://x.com/a/status/1".into(),
            uploader: Some(uploader.into()),
            upload_date: None,
            duration: None,
            file_size: None,
            description: None,
            sources: Sources::X {
                url: "https://x.com/a/status/1".into(),
                author: String::new(),
                date: String::new(),
                text: text.into(),
                links: Vec::new(),
                mentions: Vec::new(),
                quoted_posts: Vec::new(),
            },
            chapters: Vec::new(),
        }
    }

    /// The field-run post (meta.json of 20260921-lauren-poteto): the handle and the product
    /// and event names, and no common word from the post.
    #[test]
    fn field_run_post_yields_the_handle_and_product_names() {
        let text = "here's how i shipped 2,500 PRs last month to production\n\nthis was \
                    originally supposed to be for Cursor Compile in London. i couldn't make \
                    it since i was livestreaming for Grok @Bot Galaxy so i'm making it \
                    available for free here on X! watch it on 2x speed, i talk slowly";
        let meta = x_meta(
            "lauren (@poteto): here's how i shipped 2,500 PRs last month to production this was originally supp",
            "poteto",
            text,
        );
        assert_eq!(
            extract(&meta),
            [
                "poteto",
                "PRs",
                "Cursor Compile",
                "London",
                "Grok",
                "@Bot",
                "Galaxy"
            ]
        );
    }

    #[test]
    fn common_words_links_and_duplicates_stay_out() {
        let mut meta = x_meta(
            "How we use Bugbot and Grokbot",
            "Some Channel",
            "Watch Bugbot review. The @grokbot team. https://Example.com/Path #AI",
        );
        meta.sources = match meta.sources {
            Sources::X { url, .. } => Sources::X {
                url,
                author: String::new(),
                date: String::new(),
                text: "Watch Bugbot review. The @grokbot team. https://Example.com/Path #AI".into(),
                links: Vec::new(),
                mentions: Vec::new(),
                quoted_posts: vec![QuotedPost {
                    url: String::new(),
                    text: "Our iPhone and macOS apps".into(),
                }],
            },
            other => other,
        };
        meta.description =
            Some("I'm Lauren\u{2019}s friend. Subscribe! We work at SpaceX AI.".into());
        assert_eq!(
            extract(&meta),
            [
                "Some Channel",
                "Bugbot",
                "Grokbot",
                "iPhone",
                "macOS",
                "Lauren",
                "SpaceX AI"
            ]
        );
    }
}
