//! What an Obsidian note says about itself: its tags, its other names, and
//! the notes it links to.
//!
//! A vault exported from Notion into Obsidian carries its organisation in
//! front matter — `tags:` and `aliases:` — and in the body, as `#tags` and
//! `[[wikilinks]]`. The picker filters by tag and offers the notes a note
//! links to, so adding one note can bring the ones it leans on.
//!
//! Hand-read rather than through a YAML library: only these two keys are
//! wanted, in the three shapes Obsidian writes them — `tags: [a, b]`,
//! `tags: a, b`, and a list of `- a` lines beneath.

/// A note's tags, other names, links and embeds, in the order first met,
/// each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// The notes linked to, by the name a link gives: `[[Search#BFS|BFS]]`
    /// links to `Search`.
    pub links: Vec<String>,
    /// What is embedded: `![[diagram.png]]`.
    pub embeds: Vec<String>,
}

impl Meta {
    pub fn of(text: &str) -> Meta {
        let mut meta = Meta::default();
        let lines: Vec<&str> = text.lines().collect();
        let mut body = 0;
        if lines.first().map(|l| l.trim()) == Some("---") {
            if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") {
                front(&lines[1..end + 1], &mut meta);
                body = end + 2;
            }
        }
        let mut fenced = false;
        for line in &lines[body.min(lines.len())..] {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            links(line, &mut meta);
            hashtags(line, &mut meta.tags);
        }
        meta
    }
}

/// Push once, keeping the first spelling.
fn add(list: &mut Vec<String>, value: &str) {
    let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if !value.is_empty() && !list.iter().any(|v| v.eq_ignore_ascii_case(value)) {
        list.push(value.to_string());
    }
}

/// A tag, however it was quoted or hashed: `'#ai'` is `ai`.
fn tag(list: &mut Vec<String>, value: &str) {
    add(list, value.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches('#'));
}

/// `tags:` and `aliases:` in the front matter, in any of their shapes.
fn front(lines: &[&str], meta: &mut Meta) {
    let mut under: Option<&str> = None;
    for line in lines {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            match under {
                Some("tags") => tag(&mut meta.tags, item),
                Some("aliases") => add(&mut meta.aliases, item),
                _ => {}
            }
            continue;
        }
        under = None;
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let key = match key.as_str() {
            "tags" | "tag" => "tags",
            "aliases" | "alias" => "aliases",
            _ => continue,
        };
        let value = value.trim();
        if value.is_empty() {
            under = Some(key);
            continue;
        }
        let value = value.trim_start_matches('[').trim_end_matches(']');
        for item in value.split(',') {
            match key {
                "tags" => {
                    for word in item.split_whitespace() {
                        tag(&mut meta.tags, word);
                    }
                }
                _ => add(&mut meta.aliases, item),
            }
        }
    }
}

/// `[[Note]]`, `[[Note#Heading|shown]]`, `![[picture.png]]`.
fn links(line: &str, meta: &mut Meta) {
    let mut rest = line;
    while let Some(open) = rest.find("[[") {
        let embed = rest[..open].ends_with('!');
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let inner = &after[..close];
        let target = inner.split(['|', '#', '^']).next().unwrap_or("").trim();
        if !target.is_empty() {
            match embed {
                true => add(&mut meta.embeds, target),
                false => add(&mut meta.links, target),
            }
        }
        rest = &after[close + 2..];
    }
}

/// `#tag` and `#nested/tag` in prose: after a space or at the start, and not
/// all digits — `#1` is a number, `# Heading` a heading.
fn hashtags(line: &str, tags: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && (i == 0 || chars[i - 1].is_whitespace()) {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && (chars[end].is_alphanumeric() || matches!(chars[end], '_' | '-' | '/')) {
                end += 1;
            }
            let word: String = chars[start..end].iter().collect();
            if !word.is_empty() && !word.chars().all(|c| c.is_ascii_digit()) {
                add(tags, &word);
            }
            i = end;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_tags_and_aliases_in_every_shape() {
        let flow = Meta::of("---\ntags: [comp3670, \"exam prep\"]\naliases: Search, Uninformed search\n---\nbody");
        assert_eq!(flow.tags, ["comp3670", "exam", "prep"]);
        assert_eq!(flow.aliases, ["Search", "Uninformed search"]);

        let block = Meta::of("---\ntitle: Search\ntags:\n  - comp3670\n  - '#ai'\naliases:\n  - BFS\n---\n");
        assert_eq!(block.tags, ["comp3670", "ai"]);
        assert_eq!(block.aliases, ["BFS"]);
    }

    #[test]
    fn links_embeds_and_hashtags_in_the_body() {
        let meta = Meta::of("# Search\nSee [[Graphs#BFS|breadth-first]] and [[Heaps]], #comp3670 and #ml/search.\n![[frontier.png]] Item #1 is a number.\n```\n[[not a link]] #not-a-tag\n```\n[[Heaps]] again");
        assert_eq!(meta.links, ["Graphs", "Heaps"]);
        assert_eq!(meta.embeds, ["frontier.png"]);
        assert_eq!(meta.tags, ["comp3670", "ml/search"]);
    }

    #[test]
    fn an_unclosed_link_or_front_matter_is_no_trouble() {
        let meta = Meta::of("---\ntags: a\n# never closed\n[[Half");
        assert!(meta.tags.is_empty(), "an unclosed front matter is not front matter");
        assert!(meta.links.is_empty());
        assert_eq!(Meta::of(""), Meta::default());
    }
}
