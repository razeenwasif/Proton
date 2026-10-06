//! A lecture's captions: SRT and WebVTT, read into what was said and when.
//!
//! Captions are how a recorded lecture becomes something to search and cite:
//! a passage of them is cited by the moment it was said — `@ 3:12-3:40` — so a
//! citation can open the video there.
//!
//! Both formats are cues separated by blank lines, each with a timing line
//! `start --> end`. SRT writes `00:03:12,500`; WebVTT `00:03:12.500`, or
//! `03:12.500` under the hour, and allows a header, notes and styling. Markup
//! inside a cue — `<i>`, `<v Speaker>`, `<c.yellow>`, a karaoke timestamp — is
//! dropped. Captions made automatically repeat each line as it scrolls up;
//! a line said twice in a row is kept once.

/// One cue: what was said, from when to when, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Every cue in a caption file, in order. A file with none is empty, not an
/// error: a transcription that heard nothing.
pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<Cue> = Vec::new();
    let mut last_line = String::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(at) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start, end)) = timing(lines[at]) else {
            continue;
        };
        let mut said: Vec<String> = Vec::new();
        for line in &lines[at + 1..] {
            let line = clean(line);
            if line.is_empty() {
                continue;
            }
            // A rolling caption repeats the line before it as it scrolls.
            if said.is_empty() && line == last_line {
                continue;
            }
            said.push(line);
        }
        if let Some(line) = said.last() {
            last_line = line.clone();
        }
        if said.is_empty() {
            // All of it was said already: the cue only scrolled. Its time
            // still belongs to what was being said.
            if let Some(previous) = out.last_mut() {
                previous.end = previous.end.max(end);
            }
            continue;
        }
        out.push(Cue { start, end: end.max(start), text: said.join("\n") });
    }
    out
}

/// `00:03:12,500 --> 00:03:15,000 align:start` → both times in seconds.
fn timing(line: &str) -> Option<(f64, f64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    Some((clock(start.trim())?, clock(end)?))
}

/// `00:03:12,500`, `00:03:12.500` or `03:12.500` → seconds.
fn clock(text: &str) -> Option<f64> {
    let text = text.replace(',', ".");
    let parts: Vec<&str> = text.split(':').collect();
    let (h, m, s) = match parts[..] {
        [m, s] => ("0", m, s),
        [h, m, s] => (h, m, s),
        _ => return None,
    };
    let h: f64 = h.trim().parse().ok()?;
    let m: f64 = m.trim().parse().ok()?;
    let s: f64 = s.trim().parse().ok()?;
    (h >= 0.0 && m >= 0.0 && s >= 0.0).then_some(h * 3600.0 + m * 60.0 + s)
}

/// A cue's line as it reads: markup dropped, entities decoded, spaces tidied.
fn clean(line: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut brace = false;
    for c in line.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            // `{\an8}`, a positioning code some SRT files carry.
            '{' if !tag => brace = true,
            '}' if brace => brace = false,
            c if !tag && !brace => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&quot;", "\"");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn srt_cues_are_read_with_their_times() {
        let text = "1\r\n00:00:02,000 --> 00:00:05,500\r\nGood morning, <i>everyone</i>.\r\n\r\n2\r\n00:01:06,120 --> 00:01:09,000\r\n{\\an8}Today: A* search.\r\n";
        let cues = parse(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0], Cue { start: 2.0, end: 5.5, text: "Good morning, everyone.".into() });
        assert!((cues[1].start - 66.12).abs() < 1e-9, "{}", cues[1].start);
        assert_eq!(cues[1].text, "Today: A* search.");
    }

    #[test]
    fn webvtt_is_read_past_its_header_notes_and_markup() {
        let text = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE made by a machine\n\n00:03.000 --> 00:06.000 align:start position:0%\n<v Lecturer>Heuristics &amp; costs\n\nintro\n01:00:01.500 --> 01:00:03.000\nthe last <00:00:01.900><c>word</c>\n";
        let cues = parse(text);
        assert_eq!(cues.len(), 2, "{cues:#?}");
        assert_eq!((cues[0].start, cues[0].end), (3.0, 6.0));
        assert_eq!(cues[0].text, "Heuristics & costs");
        assert_eq!(cues[1].start, 3601.5);
        assert_eq!(cues[1].text, "the last word");
    }

    #[test]
    fn a_rolling_caption_is_kept_once() {
        // As automatic captions scroll: each cue repeats the line before.
        let text = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nso the frontier is\n\n00:00:03.000 --> 00:00:05.000\nso the frontier is\na priority queue\n\n00:00:05.000 --> 00:00:05.010\na priority queue\n";
        let cues = parse(text);
        let said: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(said, ["so the frontier is", "a priority queue"]);
        assert_eq!(cues[1].end, 5.01, "a cue that only scrolled lends its time to the one before");
    }

    #[test]
    fn nothing_but_noise_is_no_cues() {
        assert!(parse("").is_empty());
        assert!(parse("WEBVTT\n\nNOTE nothing\n").is_empty());
        assert!(parse("1\nnot a time --> either\nwords\n").is_empty());
    }

    #[test]
    fn captions_are_cut_into_about_a_minute_each_and_cited_by_time() {
        let mut text = String::new();
        for i in 0..30 {
            let (a, b) = (i * 5, i * 5 + 5);
            text.push_str(&format!("{}\n00:{:02}:{:02},000 --> 00:{:02}:{:02},000\nsentence number {i}.\n\n", i + 1, a / 60, a % 60, b / 60, b % 60));
        }
        let out = crate::chunk::cues(Path::new("Lecture 4.srt"), &parse(&text));
        // Thirty sentences of five seconds: ended at a sentence once half a
        // minute has run.
        assert_eq!(out.len(), 5, "{out:#?}");
        assert_eq!((out[0].from, out[0].to), (0, 30));
        assert_eq!(out[0].cite_as("Lecture 4.srt"), "Lecture 4.srt @ 0:00-0:30");
        assert_eq!(out[4].cite_as("S12"), "S12 @ 2:00-2:30");
        let read = crate::chunk::parse_cite("[[S12 @ 2:00-2:30]]").unwrap();
        assert_eq!((read.path.as_str(), read.from, read.to, read.place), ("S12", 120, 150, crate::chunk::Place::Seconds));
    }
}
