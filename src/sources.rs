use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn is_x_post(input: &str) -> bool {
    x_status_id(input).is_some()
}
/// The status ID of an X post URL, or None when the input is not an X post URL.
pub fn x_status_id(input: &str) -> Option<&str> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("mobile."))
        .unwrap_or(&host);
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let parts: Vec<_> = path.split('/').collect();
    let post = matches!(host, "x.com" | "twitter.com")
        && parts.len() == 3
        && !parts[0].is_empty()
        && parts[0]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && parts[1] == "status"
        && !parts[2].is_empty()
        && parts[2].bytes().all(|c| c.is_ascii_digit());
    post.then(|| parts[2])
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Sources {
    Local {
        path: String,
    },
    X {
        url: String,
        author: String,
        date: String,
        text: String,
        links: Vec<String>,
        mentions: Vec<String>,
        quoted_posts: Vec<QuotedPost>,
    },
    Web {
        url: String,
        channel_url: Option<String>,
        links: Vec<String>,
    },
}
#[derive(Serialize, Deserialize)]
pub struct QuotedPost {
    pub url: String,
    pub text: String,
}
#[derive(Serialize, Deserialize)]
pub struct Chapter {
    pub start_time: f64,
    pub title: String,
}

// None means a valid post without video. Malformed formats must use the fallback.
pub fn mp4_variant(post: &Value) -> anyhow::Result<Option<&str>> {
    let all = &post["media"]["all"];
    if all.is_null() {
        return Ok(None);
    }
    let items = all
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("X media.all is not an array"))?;
    let Some(item) = items
        .iter()
        .find(|m| matches!(m["type"].as_str(), Some("video" | "gif" | "animated_gif")))
    else {
        return Ok(None);
    };
    let formats = item["formats"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("X video has no formats"))?;
    let variant = formats
        .iter()
        .filter(|f| f["container"] == "mp4" && f["url"].as_str().is_some_and(crate::fetch::is_url))
        .min_by_key(|f| f["bitrate"].as_u64().unwrap_or(u64::MAX));
    Ok(Some(variant.and_then(|f| f["url"].as_str()).ok_or_else(
        || anyhow::anyhow!("X video has no mp4 format"),
    )?))
}

fn push_unique(links: &mut Vec<String>, url: &str) {
    if crate::fetch::is_url(url) && !links.iter().any(|s| s == url) {
        links.push(url.to_owned());
    }
}
pub fn description_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest
        .find("https://")
        .into_iter()
        .chain(rest.find("http://"))
        .min()
    {
        rest = &rest[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .unwrap_or(rest.len());
        let mut url = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while url.ends_with(close) && url.matches(close).count() > url.matches(open).count() {
                url = &url[..url.len() - 1];
            }
        }
        push_unique(&mut links, url);
        rest = &rest[end..];
    }
    links
}
pub fn x_links(post: &Value) -> (Vec<String>, Vec<String>) {
    let mut links = Vec::new();
    let mut mentions = Vec::new();
    let own_url = post["url"].as_str().unwrap_or("");
    let mut media_links = Vec::new();
    if let Some(facets) = post["raw_text"]["facets"].as_array() {
        for facet in facets {
            if facet["type"] == "media" {
                for key in ["original", "replacement", "expanded_url"] {
                    if let Some(url) = facet[key].as_str() {
                        media_links.push(url);
                    }
                }
            }
        }
        for facet in facets {
            match facet["type"].as_str() {
                Some("mention") => {
                    if let Some(handle) = facet["original"].as_str() {
                        push_unique(
                            &mut mentions,
                            &format!("https://x.com/{}", handle.trim_start_matches('@')),
                        );
                    }
                }
                Some("url") => {
                    if let Some(url) = facet["replacement"]
                        .as_str()
                        .or_else(|| facet["expanded_url"].as_str())
                        .or_else(|| facet["expanded"].as_str())
                    {
                        push_unique(&mut links, url);
                    }
                }
                _ => {}
            }
        }
    }
    fn card_links(card: &Value, links: &mut Vec<String>) {
        match card {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(key.as_str(), "url" | "expanded_url" | "destination_url") {
                        if let Some(url) = value.as_str() {
                            push_unique(links, url);
                        }
                    } else {
                        card_links(value, links);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    card_links(value, links);
                }
            }
            Value::String(url) if crate::fetch::is_url(url) => push_unique(links, url),
            _ => {}
        }
    }
    card_links(&post["embed_card"], &mut links);
    let post_id = own_url
        .split(['?', '#'])
        .next()
        .unwrap_or(own_url)
        .trim_end_matches('/')
        .rsplit('/')
        .next();
    links.retain(|url| {
        let media_post = url
            .split_once("/video/")
            .or_else(|| url.split_once("/photo/"))
            .map(|(post, _)| post);
        let own_media =
            media_post.is_some_and(|post| is_x_post(post) && post.rsplit('/').next() == post_id);
        !media_links.contains(&url.as_str()) && !own_media
    });
    (links, mentions)
}

pub fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
fn blockquote(text: &str, out: &mut String) {
    for line in text.lines() {
        out.push_str(&format!("> {line}\n"));
    }
    out.push('\n');
}
pub fn render(sources: &Sources, chapters: &[Chapter]) -> String {
    let mut out = String::from("## Sources\n\n");
    match sources {
        Sources::Local { path } => out.push_str(&format!("- {path}\n\n")),
        Sources::Web {
            url,
            channel_url,
            links,
        } => {
            out.push_str(&format!("- <{url}>\n"));
            for link in channel_url.iter().chain(links) {
                out.push_str(&format!("- <{link}>\n"));
            }
            out.push('\n');
        }
        Sources::X {
            url,
            author,
            date,
            text,
            links,
            mentions,
            quoted_posts,
        } => {
            out.push_str(&format!(
                "- Post: <{url}>\n- Author: {author}\n- Date: {date}\n\n"
            ));
            blockquote(text, &mut out);
            for link in links.iter().chain(mentions) {
                out.push_str(&format!("- <{link}>\n"));
            }
            out.push('\n');
            for quote in quoted_posts {
                out.push_str(&format!("Quoted post: <{}>\n\n", quote.url));
                blockquote(&quote.text, &mut out);
            }
        }
    }
    if !chapters.is_empty() {
        out.push_str("## Chapters\n\n");
        for chapter in chapters {
            out.push_str(&format!(
                "- [{}] {}\n",
                timestamp(chapter.start_time),
                chapter.title.replace(['\r', '\n'], " ")
            ));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_only_status_urls_on_x_hosts() {
        for host in ["x.com", "twitter.com", "www.x.com", "mobile.twitter.com"] {
            assert_eq!(
                x_status_id(&format!(
                    "https://{host}/pidotdev/status/2107033061905104941?s=20"
                )),
                Some("2107033061905104941")
            );
        }
        for url in [
            "https://x.com.evil/a/status/123",
            "https://evil/x.com/a/status/123",
            "https://x.com/a/status/no",
            "https://x.com/a",
            "https://x.com/a/status/123/video/1",
        ] {
            assert!(!is_x_post(url));
        }
    }
    #[test]
    fn extracts_mentions_and_excludes_own_media() {
        let raw: Value = serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        let (links, mentions) = x_links(&raw["posts"][0]);
        assert!(links.is_empty());
        assert_eq!(
            mentions,
            ["https://x.com/badlogicgames", "https://x.com/mitsuhiko"]
        );
    }
    #[test]
    fn extracts_description_links_in_order_without_punctuation() {
        let raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/youtube.json")).unwrap();
        assert_eq!(
            description_links(raw["description"].as_str().unwrap()),
            ["http://ed.ted.com/lessons/try-something-new-for-30-days-matt-cutts"]
        );
        assert_eq!(
            description_links(
                "(https://example.com/a_(b)). <http://example.org> https://example.com/a_(b)"
            ),
            ["https://example.com/a_(b)", "http://example.org"]
        );
    }
    #[test]
    fn expands_outbound_facets_and_renders_sources() {
        let raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/x-outbound.json")).unwrap();
        let (links, mentions) = x_links(&raw["posts"][0]);
        assert_eq!(links, ["http://openai.com/index/hello-gpt-4o/"]);
        assert!(mentions.is_empty());
        let sources = Sources::X {
            url: "https://x.com/OpenAI/status/1790072174117613963".into(),
            author: "OpenAI (@OpenAI)".into(),
            date: "Mon May 13 17:13:00 +0000 2024".into(),
            text: "First line\n\nLast line".into(),
            links,
            mentions,
            quoted_posts: vec![QuotedPost {
                url: "https://x.com/example/status/1".into(),
                text: "Quoted text".into(),
            }],
        };
        assert_eq!(
            render(
                &sources,
                &[Chapter {
                    start_time: 3661.9,
                    title: "Next chapter".into()
                }]
            ),
            "## Sources\n\n- Post: <https://x.com/OpenAI/status/1790072174117613963>\n- Author: OpenAI (@OpenAI)\n- Date: Mon May 13 17:13:00 +0000 2024\n\n> First line\n> \n> Last line\n\n- <http://openai.com/index/hello-gpt-4o/>\n\nQuoted post: <https://x.com/example/status/1>\n\n> Quoted text\n\n## Chapters\n\n- [01:01:01] Next chapter\n\n"
        );
    }
    #[test]
    fn renders_local_and_web_sources_without_chapters() {
        assert_eq!(
            render(
                &Sources::Local {
                    path: "/absolute/video.mp4".into()
                },
                &[]
            ),
            "## Sources\n\n- /absolute/video.mp4\n\n"
        );
        let raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/youtube.json")).unwrap();
        let sources = Sources::Web {
            url: raw["webpage_url"].as_str().unwrap().into(),
            channel_url: raw["channel_url"].as_str().map(str::to_owned),
            links: description_links(raw["description"].as_str().unwrap()),
        };
        let markdown = render(&sources, &[]);
        assert!(markdown.contains("- <https://www.youtube.com/watch?v=UNP03fDSj1U>"));
        assert!(
            markdown
                .contains("- <http://ed.ted.com/lessons/try-something-new-for-30-days-matt-cutts>")
        );
        assert!(!markdown.contains("## Chapters"));
    }
    #[test]
    fn renders_chapters_from_real_youtube_metadata() {
        let raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/youtube.json")).unwrap();
        let chapters: Vec<Chapter> = serde_json::from_value(raw["chapters"].clone()).unwrap();
        let markdown = render(
            &Sources::Local {
                path: "/video.mp4".into(),
            },
            &chapters,
        );
        assert!(markdown.ends_with("## Chapters\n\n- [00:00:00] <Untitled Chapter 1>\n- [00:00:25] Try Something New for 30 Days\n- [00:01:39] Write a Novel\n- [00:02:51] Day 31\n\n"));
    }
    #[test]
    fn excludes_media_links_on_equivalent_twitter_hosts() {
        let mut raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/x-outbound.json")).unwrap();
        raw["posts"][0]["embed_card"] = serde_json::json!({"url": "https://mobile.twitter.com/OpenAI/status/1790072174117613963/video/1"});
        assert_eq!(
            x_links(&raw["posts"][0]).0,
            ["http://openai.com/index/hello-gpt-4o/"]
        );
    }
    #[test]
    fn malformed_media_and_non_http_formats_are_errors() {
        assert!(mp4_variant(&serde_json::json!({"media": {"all": "invalid"}})).is_err());
        assert!(mp4_variant(&serde_json::json!({"media": {"all": [{"type": "video", "formats": [{"container": "mp4", "url": "file:///private/video.mp4"}]}]}})).is_err());
    }
    #[test]
    fn card_links_are_deduplicated_and_media_is_excluded() {
        let mut raw: Value =
            serde_json::from_str(include_str!("../tests/fixtures/x-outbound.json")).unwrap();
        raw["posts"][0]["embed_card"] = serde_json::json!({"url": "http://openai.com/index/hello-gpt-4o/", "destination_url": "https://example.com/article", "media": {"url": "https://x.com/OpenAI/status/1790072174117613963/video/1"}});
        assert_eq!(
            x_links(&raw["posts"][0]).0,
            [
                "http://openai.com/index/hello-gpt-4o/",
                "https://example.com/article"
            ]
        );
    }
    #[test]
    fn video_selection_does_not_use_a_later_item_or_unknown_bitrate_first() {
        let post = serde_json::json!({"media": {"all": [
            {"type": "photo"},
            {"type": "gif", "formats": [{"container": "mp4", "url": "https://example.com/unknown"}, {"container": "mp4", "bitrate": 20, "url": "https://example.com/low"}]},
            {"type": "video", "formats": [{"container": "mp4", "bitrate": 1, "url": "https://example.com/later"}]}
        ]}});
        assert_eq!(mp4_variant(&post).unwrap(), Some("https://example.com/low"));
        assert_eq!(
            mp4_variant(&serde_json::json!({"media": {"all": [{"type": "photo"}]}})).unwrap(),
            None
        );
        assert!(
            mp4_variant(&serde_json::json!({"media": {"all": [{"type": "video", "formats": []}]}}))
                .is_err()
        );
    }
    #[test]
    fn selects_lowest_bitrate_mp4_from_first_video() {
        let raw: Value = serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        assert_eq!(
            mp4_variant(&raw["posts"][0]).unwrap().unwrap(),
            "https://video.twimg.com/amplify_video/2107019854444105728/vid/avc1/480x270/8A4sbWV2hynP7PON.mp4?tag=29"
        );
    }
}
