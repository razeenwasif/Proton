//! Pages of the web as sources: fetched on a worker of their own through the
//! literature's client — which says who is asking, waits its turn at each
//! host, and asks again when a host is busy — and kept as they were: a page
//! as the Markdown of its article, a PDF whole, a YouTube video as its
//! captions. Nothing is fetched but what the person named, and a page is
//! fetched again only when they ask.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use proton_library::extract::{Body, Pdfs};
use proton_library::formats::{captions, web};
use proton_library::Kind;
use proton_literature::http::{host_of, Failure, Http};
use proton_study::snapshot::{self, Read as Kept};

use super::Answer;

/// What a page is asked for as.
const ACCEPT: &str = "text/html,application/xhtml+xml,application/pdf;q=0.9,text/plain;q=0.8,text/markdown;q=0.8,*/*;q=0.5";

/// The longest a page or a PDF may take to come.
const TIMEOUT: Duration = Duration::from_secs(120);

/// One address for the worker to fetch, for one source.
pub(super) struct Fetch {
    pub id: String,
    pub url: String,
}

/// What the worker says of a page it fetched: its words, as a file's are
/// said, and what it turned out to be.
pub(super) struct Fetched {
    pub read: Kept,
    pub title: String,
    pub kind: Kind,
}

/// A web address, from what the person typed: `https://` and `http://`
/// ones as they are, a bare `example.com/page` given `https://`.
pub(super) fn address(typed: &str) -> Option<String> {
    let typed = typed.trim();
    let url = if typed.starts_with("https://") || typed.starts_with("http://") {
        typed.to_string()
    } else if !typed.contains("://") && typed.split('/').next().is_some_and(|host| host.contains('.')) {
        format!("https://{typed}")
    } else {
        return None;
    };
    let host = host_of(&url);
    (!url.contains(char::is_whitespace) && !host.is_empty()).then_some(url)
}

/// A source's provisional title, until its page says what it is called:
/// the address without its scheme.
pub(super) fn provisional(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.trim_end_matches('/').chars().take(80).collect()
}

/// Fetch each address in turn, keep its words in `snapshots` and what came
/// whole in `kept`, and say what became of it.
pub(super) fn fetch_all(items: Vec<Fetch>, http: Http, snapshots: PathBuf, kept: PathBuf, pdfs: Pdfs, stop: Arc<AtomicBool>, answers: Sender<(String, Answer)>) {
    for item in items {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let answer = match fetch(&item, &http, &kept, &pdfs, &stop) {
            Ok(fetched) => match snapshot::keep_in(&snapshots, &item.id, &fetched.read.body) {
                Ok(()) => Answer::Fetched(Box::new(fetched)),
                Err(why) => Answer::Failed(why),
            },
            Err(why) => Answer::Failed(why),
        };
        if answers.send((item.id, answer)).is_err() {
            return;
        }
    }
}

fn now() -> u64 {
    proton_study::now()
}

/// One address, fetched and read.
fn fetch(item: &Fetch, http: &Http, kept: &Path, pdfs: &Pdfs, stop: &AtomicBool) -> Result<Fetched, String> {
    if web::youtube(&item.url).is_some() {
        let (title, file) = web::captions(&item.url, kept, &item.id)?;
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let cues = captions::parse(&text);
        if cues.is_empty() {
            return Err("its captions are empty".into());
        }
        let read = Kept { body: Body::Cues(cues), size: text.len() as u64, mtime: now(), sha256: snapshot::digest_text(&text) };
        return Ok(Fetched { read, title, kind: Kind::Captions });
    }
    // Asked over TLS, always: an `http://` page is asked for at `https://`.
    let url = match item.url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => item.url.clone(),
    };
    let host = host_of(&url);
    let mut response = http.get(&url, &[("Accept", ACCEPT)], TIMEOUT, proton_library::pdf::LARGEST, false, stop).map_err(|failure| match failure {
        Failure::Network { why, .. } if item.url.starts_with("http://") => format!("{host} could not be reached over https: {why}"),
        failure => failure.to_string(),
    })?;
    match response.status {
        200..=299 => {}
        401 | 403 => return Err(format!("{host} would not give it ({}): it may be behind a sign-in, or only for browsers", response.status)),
        404 | 410 => return Err(format!("{host} has no page there ({})", response.status)),
        status => return Err(format!("{host} answered {status}")),
    }
    let ctype = response.header("content-type").unwrap_or_default().to_ascii_lowercase();
    let mut bytes = Vec::new();
    response.body.read_to_end(&mut bytes).map_err(|e| {
        let why = e.to_string();
        match why.contains("limit") || why.contains("too large") || why.contains("BodyExceedsLimit") {
            true => format!("it is bigger than the {} MB a source may be", proton_library::pdf::LARGEST >> 20),
            false => format!("{host} stopped answering: {why}"),
        }
    })?;
    if ctype.starts_with("application/pdf") || bytes.starts_with(b"%PDF-") {
        std::fs::create_dir_all(kept).map_err(|e| e.to_string())?;
        let path = kept.join(format!("{}.pdf", item.id));
        proton_research::store::atomic_write(&path, &bytes)?;
        let read = snapshot::read(&path, Kind::Pdf, pdfs).map_err(|why| why.say())?;
        return Ok(Fetched { read, title: named(&url), kind: Kind::Pdf });
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let html = ctype.contains("html") || (ctype.is_empty() && text.trim_start().starts_with('<'));
    let (title, words, kind) = if html {
        let page = web::page(&text);
        (page.title, page.markdown, Kind::Web)
    } else if ctype.starts_with("text/markdown") || ctype.starts_with("text/x-markdown") {
        (named(&url), text, Kind::Markdown)
    } else if ctype.starts_with("text/") {
        (named(&url), text, Kind::Text)
    } else {
        return Err(format!("{host} gave a {} file, which is not read", ctype.split(';').next().unwrap_or("strange")));
    };
    if !words.chars().any(char::is_alphanumeric) {
        return Err("there are no words on it to read: it may be a page only a browser can draw".into());
    }
    let read = Kept { body: Body::Lines(words.clone()), size: bytes.len() as u64, mtime: now(), sha256: snapshot::digest_text(&words) };
    Ok(Fetched { read, title, kind })
}

/// What an address names: its last part, as a file's name.
fn named(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
    let last = path.rsplit('/').next().unwrap_or(path);
    let decoded = decode(last);
    match decoded.trim() {
        "" => provisional(url),
        name => name.to_string(),
    }
}

/// `%20` and its kin, as the characters they are.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_what_was_typed_or_given_https() {
        assert_eq!(address(" https://en.wikipedia.org/wiki/Softmax ").as_deref(), Some("https://en.wikipedia.org/wiki/Softmax"));
        assert_eq!(address("cs.anu.edu.au/courses/comp3670").as_deref(), Some("https://cs.anu.edu.au/courses/comp3670"));
        assert_eq!(address("http://example.com/a").as_deref(), Some("http://example.com/a"));
        for not in ["", "notes", "ftp://example.com/x", "file:///etc/passwd", "https://", "https://exa mple.com"] {
            assert_eq!(address(not), None, "{not}");
        }
        assert_eq!(provisional("https://en.wikipedia.org/wiki/Softmax/"), "en.wikipedia.org/wiki/Softmax");
        assert_eq!(named("https://example.com/notes/Week%203%20Lecture.pdf?dl=1"), "Week 3 Lecture.pdf");
        assert_eq!(named("https://example.com/"), "example.com");
    }
}
