//! A web page's words, as a reader view shows them: its article — or, where
//! the page does not say which part that is, the block its paragraphs gather
//! in — without its menus, footers, scripts, pictures, or its links'
//! addresses. Pandoc makes it Markdown, its mathematics TeX whether the page
//! wrote it as MathML or as MathJax's `\(…\)`; without pandoc, dom_query's
//! own Markdown does.
//!
//! And a video's captions, through yt-dlp when the machine has it: only the
//! captions, never the video.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use dom_query::{Document, NodeId, NodeRef};

/// A page, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub title: String,
    pub markdown: String,
}

/// What is never a page's text: its machinery, its furniture, and a few of
/// the furnishings big sites share — Wikipedia's `[edit]` and `[1]`.
const CHROME: &str = "script:not([type^='math/tex']), style, noscript, template, iframe, object, embed, svg, canvas, video, audio, \
    form, button, input, select, textarea, nav, footer, aside, body > header, \
    [role=navigation], [role=banner], [role=contentinfo], [role=complementary], [role=search], [aria-hidden=true], [hidden], \
    img, picture, figure > a:empty, \
    .mw-editsection, sup.reference, .navbox, .toc, #toc, .catlinks, .printfooter, .noprint, .mw-jump-link";

/// Where pages say their article is, most particular first.
const ARTICLE: [&str; 3] = ["article", "[role=main]", "main"];

/// What pandoc writes: Markdown the window draws, nothing else — no raw
/// HTML, no attributes, tables as pipes.
const MARKDOWN: &str = "markdown-smart-raw_html-native_divs-native_spans-fenced_divs-bracketed_spans-link_attributes-header_attributes\
    -simple_tables-multiline_tables-grid_tables";

/// A page's title: what it says it is to be shared as, or its `<title>`, or
/// its first heading.
fn title_of(doc: &Document) -> String {
    let meta = doc.select("meta[property='og:title']").attr("content").map(|t| t.to_string());
    let titled = || Some(doc.select("title").text().to_string());
    let heading = || Some(doc.select("h1").first().text().to_string());
    [meta, titled(), heading()].into_iter().flatten().map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")).find(|t| !t.is_empty()).unwrap_or_default()
}

/// Letters and digits: what says a block has words.
fn letters(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

/// The block a page's paragraphs gather in: each paragraph counts its words
/// to its parent, and half to its grandparent, and the most counted wins.
fn gathered<'a>(doc: &'a Document) -> Option<NodeRef<'a>> {
    let mut score: HashMap<NodeId, usize> = HashMap::new();
    let mut nodes: HashMap<NodeId, NodeRef<'a>> = HashMap::new();
    for p in doc.select("p, pre, blockquote").nodes() {
        let words = letters(&p.text());
        if words < 25 {
            continue;
        }
        let words = words.min(2000);
        if let Some(parent) = p.parent() {
            *score.entry(parent.id).or_default() += words;
            nodes.insert(parent.id, parent);
            if let Some(grand) = parent.parent().filter(|g| g.is_element()) {
                *score.entry(grand.id).or_default() += words / 2;
                nodes.insert(grand.id, grand);
            }
        }
    }
    let (best, _) = score.into_iter().max_by_key(|(id, s)| (*s, std::cmp::Reverse(*id)))?;
    nodes.remove(&best)
}

/// The part of a page that is its article.
fn article<'a>(doc: &'a Document) -> Option<NodeRef<'a>> {
    let gathered = gathered(doc);
    if let Some(block) = gathered {
        // The page's own word for the article, if it holds what gathered:
        // it has the title and the headings a paragraph block may not.
        for sel in ARTICLE {
            if let Some(holder) = block.ancestors_it(None).chain(std::iter::once(block)).find(|n| n.is(sel)) {
                return Some(holder);
            }
        }
        // Unless it is a sliver of a page that is mostly lists or code.
        let all = doc.body().map_or(0, |b| letters(&b.text()));
        let held = letters(&block.text());
        if held >= 200 || held * 3 >= all {
            return Some(block);
        }
    }
    // A page of lists or code, with few paragraphs: the longest article it
    // names, or its body.
    for sel in ARTICLE {
        let longest = doc.select(sel).nodes().iter().copied().max_by_key(|n| letters(&n.text()));
        if let Some(node) = longest.filter(|n| letters(&n.text()) >= 200) {
            return Some(node);
        }
    }
    doc.body()
}

/// `{\displaystyle x}` — how Wikipedia writes every formula — is `x`.
fn plain_tex(tex: &str) -> String {
    let tex = tex.trim();
    match tex.strip_prefix("{\\displaystyle").and_then(|t| t.strip_suffix('}')) {
        Some(inner) => inner.trim().to_string(),
        None => tex.to_string(),
    }
}

/// A page's article as Markdown, and its title.
pub fn page(html: &str) -> Page {
    let doc = Document::from(html);
    let title = title_of(&doc);
    doc.select(CHROME).remove();
    for annotation in doc.select("annotation[encoding='application/x-tex']").nodes() {
        let tex = plain_tex(&annotation.text());
        annotation.set_text(tex);
    }
    // Links keep their words, not where they go: a snapshot is read, not
    // browsed.
    let links = doc.select("a");
    links.remove_all_attrs();
    links.rename("span");
    let Some(node) = article(&doc) else {
        return Page { title, markdown: String::new() };
    };
    let markdown = match pandoc(&node.html()) {
        Some(markdown) => markdown,
        None => window_escapes(&node.md(None)),
    };
    Page { title, markdown: tidy(&markdown) }
}

/// dom_query's Markdown with the window's escapes: it escapes what
/// CommonMark might read as markup, but the window reads `\(…\)` and `\[…\]`
/// as TeX, so those escapes — and the ones that matter only at a line's
/// start, elsewhere — are taken back; and a dollar, which the window reads as
/// TeX too, is escaped. Code is left as it is.
fn window_escapes(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut fenced = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if fenced {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let start = line.len() - line.trim_start().len();
        let mut code = false;
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '`' {
                code = !code;
            } else if !code && c == '\\' {
                if let Some(&(_, next)) = chars.peek() {
                    // `# `, `+ `, `> `, `- ` and `1. ` begin a heading, a list
                    // or a quote only where a line begins.
                    let leads = i == start && matches!(next, '#' | '+' | '>' | '-');
                    let numbered = next == '.' && i > start && line[start..i].bytes().all(|b| b.is_ascii_digit());
                    if matches!(next, '(' | ')' | '[' | ']' | '{' | '}' | '.' | '!' | '|' | '<' | '>' | '+' | '#' | '-') && !leads && !numbered {
                        continue;
                    }
                }
            } else if !code && c == '$' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('\n');
    }
    out
}

/// HTML as Markdown, through pandoc, reading TeX in `$…$` and `\(…\)` as
/// mathematics; `None` without pandoc.
fn pandoc(html: &str) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("pandoc")
        .args(["-f", "html+tex_math_dollars+tex_math_single_backslash", "-t", MARKDOWN, "--wrap=none"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    // Written from a thread of its own, so a page bigger than the pipe holds
    // cannot wait on pandoc while pandoc waits to be read.
    let html = html.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(html.as_bytes());
    });
    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// No run of blank lines longer than one, no trailing spaces; and a table
/// whose head pandoc left empty — a page's `<th>` row not in a `<thead>` —
/// headed by its first row.
fn tidy(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let empty_head = |l: &str| l.starts_with('|') && l.len() > 1 && l.chars().all(|c| c == '|' || c == ' ');
    let rule = |l: &str| l.starts_with('|') && l.contains('-') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let mut rows: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if empty_head(lines[i].trim_end()) && lines.get(i + 1).is_some_and(|l| rule(l.trim_end())) && lines.get(i + 2).is_some_and(|l| l.starts_with('|')) {
            rows.extend([lines[i + 2], lines[i + 1]]);
            i += 3;
            continue;
        }
        rows.push(lines[i]);
        i += 1;
    }
    let mut out = String::new();
    let mut blank = true;
    for line in rows {
        let line = line.trim_end();
        if line.is_empty() {
            if !blank {
                out.push('\n');
            }
            blank = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        blank = false;
    }
    out.trim_end().to_string() + "\n"
}

/// A YouTube video's id, however its address is written: `watch?v=`,
/// `youtu.be/`, `shorts/`, `embed/`, `live/`.
pub fn youtube(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").or_else(|| host.strip_prefix("m.")).or_else(|| host.strip_prefix("music.")).unwrap_or(&host);
    let id = match host {
        "youtu.be" => path.split(['?', '#', '/']).next()?.to_string(),
        "youtube.com" | "youtube-nocookie.com" => {
            let (route, query) = path.split_once('?').unwrap_or((path, ""));
            match route.split('/').collect::<Vec<_>>()[..] {
                ["watch"] => query.split('&').find_map(|pair| pair.strip_prefix("v="))?.split('#').next()?.to_string(),
                ["shorts" | "embed" | "live" | "v", id, ..] => id.split('#').next()?.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    (id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then_some(id)
}

/// A video's English captions and its title, through yt-dlp: its own
/// captions if it has them, else the ones made automatically. The video is
/// not downloaded.
pub fn captions(url: &str, dir: &Path, stem: &str) -> Result<(String, PathBuf), String> {
    captions_with("yt-dlp", url, dir, stem)
}

fn captions_with(program: &str, url: &str, dir: &Path, stem: &str) -> Result<(String, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Captions left by an earlier try are not this one's.
    for old in written(dir, stem) {
        let _ = std::fs::remove_file(old);
    }
    let out = Command::new(program)
        .args(["--skip-download", "--write-subs", "--write-auto-subs", "--sub-langs", "en,en-orig,en-.*", "--sub-format", "vtt/srt/best"])
        .args(["--no-playlist", "--no-warnings", "--print", "title", "--no-simulate", "-o"])
        .arg(dir.join(format!("{stem}.%(ext)s")))
        .arg("--")
        .arg(url)
        .output()
        .map_err(|_| "yt-dlp is not installed here: brew install yt-dlp".to_string())?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect::<String>();
        return Err(format!("yt-dlp could not read it: {said}"));
    }
    let title = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or_default().trim().to_string();
    let found = written(dir, stem);
    // The person's own English first, then any English, then anything.
    let rank = |p: &PathBuf| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        (!name.contains(".en."), !name.contains(".en"), name)
    };
    let best = found.into_iter().min_by_key(rank).ok_or("it has no English captions to read")?;
    Ok((title, best))
}

/// The caption files yt-dlp wrote for `stem`.
fn written(dir: &Path, stem: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with(&format!("{stem}.")) && (name.ends_with(".vtt") || name.ends_with(".srt"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here(program: &str) -> bool {
        Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
    }

    const BLOG: &str = r#"<!doctype html><html><head><title>Search | A blog</title>
        <meta property="og:title" content="Uniform cost search, gently"><script>track()</script><style>p{}</style></head>
        <body><header><a href="/">A blog</a><nav><a href="/about">About</a></nav></header>
        <div class="layout"><aside><p>Subscribe to the newsletter for weekly posts about all sorts of things.</p></aside>
        <article><h1>Uniform cost search</h1>
        <div class="content"><p>Uniform cost search expands the node with the lowest path cost <a href="/g">g(n)</a> first.</p>
        <p>It is optimal when every step costs at least some positive amount, which rules out zero-cost loops.</p>
        <img src="tree.png" alt="a tree"><pre><code>frontier = PriorityQueue()</code></pre></div></article>
        <div class="related"><p>Related: breadth-first search and its many friends, read it next week.</p></div></div>
        <footer><p>Copyright the blog, all rights reserved, every year since the beginning.</p></footer></body></html>"#;

    #[test]
    fn a_page_s_article_is_kept_and_its_furniture_left() {
        let doc = Document::from(BLOG);
        doc.select(CHROME).remove();
        let node = article(&doc).unwrap();
        assert!(node.is("article"), "the article holds the paragraphs and its title");
        let page = page(BLOG);
        assert_eq!(page.title, "Uniform cost search, gently");
        for kept in ["Uniform cost search", "lowest path cost g(n) first", "rules out zero-cost loops", "frontier = PriorityQueue()"] {
            assert!(page.markdown.contains(kept), "{kept}: {}", page.markdown);
        }
        for left in ["Subscribe", "Copyright", "About", "track()", "tree.png", "Related", "/g"] {
            assert!(!page.markdown.contains(left), "{left}: {}", page.markdown);
        }
    }

    #[test]
    fn a_page_that_names_no_article_is_read_where_its_paragraphs_gather() {
        let html = r#"<html><head><title>Notes</title></head><body><div id="menu"><ul><li>Home</li><li>Notes</li></ul></div>
            <div id="text"><h2>Heuristics</h2><p>An admissible heuristic never overestimates the cost of reaching the goal.</p>
            <p>A consistent heuristic obeys the triangle inequality along every edge of the graph.</p></div></body></html>"#;
        let doc = Document::from(html);
        doc.select(CHROME).remove();
        assert_eq!(article(&doc).unwrap().attr("id").as_deref(), Some("text"));
        assert_eq!(page(html).title, "Notes");
    }

    #[test]
    fn a_page_s_mathematics_comes_out_as_tex() {
        if !here("pandoc") {
            return;
        }
        let html = r##"<html><head><title>Softmax</title></head><body><main><div class="mw-parser-output">
            <h2>Definition<span class="mw-editsection">[edit]</span></h2>
            <p>The softmax of a vector <span class="mwe-math-element"><span style="display: none;"><math alttext="{\displaystyle \mathbf {z} }">
            <semantics><mi>z</mi><annotation encoding="application/x-tex">{\displaystyle \mathbf {z} }</annotation></semantics></math></span>
            <img src="z.svg" aria-hidden="true" alt="z"></span> is a probability distribution<sup class="reference"><a href="#c1">[1]</a></sup>,
            and the loss \(L = -\log p\) costs $5 to compute, and so on for a while longer.</p>
            <table><tr><th>x</th><th>y</th></tr><tr><td>1</td><td>2</td></tr></table></div></main></body></html>"##;
        let page = page(html);
        let md = &page.markdown;
        assert!(md.contains("## Definition\n"), "{md}");
        assert!(md.contains("$\\mathbf {z}$") || md.contains("$\\mathbf{z}$"), "{md}");
        assert!(md.contains("$L = -\\log p$") && md.contains("\\$5"), "{md}");
        assert!(!md.contains("[1]") && !md.contains("[edit]") && !md.contains("z.svg"), "{md}");
        assert!(md.contains("| x   | y   |\n|-----|-----|\n| 1   | 2   |"), "a table as pipes, headed by its headings: {md}");
    }

    #[test]
    fn without_pandoc_a_page_is_escaped_as_the_window_reads_it() {
        let md = "# Search\n\nThe cost g\\(n\\) is \\[1\\] cheap\\. It costs $5\\!\n\n1\\. not a list\n\\# not a heading\n\n`a\\(b` and $x\n\n```\necho $HOME \\(x\\)\n```\n";
        let out = window_escapes(md);
        assert!(out.contains("The cost g(n) is [1] cheap. It costs \\$5!"), "{out}");
        assert!(out.contains("1\\. not a list\n\\# not a heading"), "what begins a line keeps its escape: {out}");
        assert!(out.contains("`a\\(b` and \\$x"), "code keeps its own: {out}");
        assert!(out.contains("echo $HOME \\(x\\)"), "a fence keeps its own: {out}");
        assert_eq!(page_without_pandoc(BLOG).replace("\n\n", "\n").lines().filter(|l| l.contains("lowest")).count(), 1);
    }

    /// What `page` makes of a page when pandoc is not here.
    fn page_without_pandoc(html: &str) -> String {
        let doc = Document::from(html);
        doc.select(CHROME).remove();
        let links = doc.select("a");
        links.remove_all_attrs();
        links.rename("span");
        tidy(&window_escapes(&article(&doc).unwrap().md(None)))
    }

    #[test]
    fn a_youtube_video_is_known_however_its_address_is_written() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ#t=1",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
        ] {
            assert_eq!(youtube(url).as_deref(), Some("dQw4w9WgXcQ"), "{url}");
        }
        for url in ["https://www.youtube.com/@channel", "https://www.youtube.com/watch?v=short", "https://example.com/watch?v=dQw4w9WgXcQ", "ftp://youtu.be/dQw4w9WgXcQ"] {
            assert_eq!(youtube(url), None, "{url}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_video_s_captions_come_from_yt_dlp_and_say_so_when_it_is_not_here() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("proton-library-web-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = captions_with("proton-no-such-yt-dlp", "https://youtu.be/dQw4w9WgXcQ", &dir, "S04").unwrap_err();
        assert!(err.contains("brew install yt-dlp"), "{err}");

        // A stand-in that writes what yt-dlp writes: automatic captions in
        // two Englishes, and the title on its output.
        let fake = dir.join("yt-dlp");
        std::fs::write(
            &fake,
            "#!/bin/sh\nfor last; do :; done\nout=\"\"\nwhile [ $# -gt 0 ]; do [ \"$1\" = -o ] && out=\"$2\"; shift; done\n\
             base=\"${out%.%(ext)s}\"\nprintf 'WEBVTT\\n\\n00:00:01.000 --> 00:00:03.000\\nhello\\n' > \"$base.en-GB.vtt\"\n\
             printf 'WEBVTT\\n\\n00:00:01.000 --> 00:00:03.000\\nuniform cost search\\n' > \"$base.en.vtt\"\necho 'Lecture 3: Search'\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (title, path) = captions_with(fake.to_str().unwrap(), "https://youtu.be/dQw4w9WgXcQ", &dir, "S04").unwrap();
        assert_eq!(title, "Lecture 3: Search");
        assert_eq!(path.file_name().unwrap(), "S04.en.vtt", "the plain English first");
    }
}
