//! Documents and slides as words: Word, OpenDocument, RTF and EPUB through
//! pandoc, as Markdown; a PowerPoint's slides one by one, each slide's text
//! and its speaker notes, read from the zip it is with the system's `unzip`
//! and its XML with quick-xml; a picture's words through tesseract.
//!
//! Each is a program the machine has or has not: when it has not, the file
//! is said not to be readable, and why — never read wrong.

use std::path::Path;
use std::process::Command;

/// The extensions pandoc reads, as what it calls them.
fn pandoc_format(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "docx" => Some("docx"),
        "odt" => Some("odt"),
        "rtf" => Some("rtf"),
        "epub" => Some("epub"),
                _ => None,
    }
}

/// A program's output, or why there is none.
fn run(command: &mut Command, what: &str, install: &str) -> Result<Vec<u8>, String> {
    let out = command.output().map_err(|_| format!("{what} is not installed here: {install}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect::<String>();
        return Err(format!("{what} could not read it: {said}"));
    }
    Ok(out.stdout)
}

/// A document as Markdown, its mathematics as TeX: what pandoc makes of it.
pub fn document(path: &Path) -> Result<String, String> {
    let Some(format) = pandoc_format(path) else {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("this kind of");
        return Err(format!("a .{ext} document is not read: save it as .docx or PDF"));
    };
    let out = run(Command::new("pandoc").args(["-f", format, "-t", "markdown-smart", "--wrap=none"]).arg(path), "pandoc", "brew install pandoc")?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The names in a zip, in order.
fn zip_names(path: &Path) -> Result<Vec<String>, String> {
    let out = run(Command::new("unzip").arg("-Z1").arg(path), "unzip", "it comes with macOS and most Linux")?;
    Ok(String::from_utf8_lossy(&out).lines().map(str::to_string).collect())
}

fn zip_entry(path: &Path, name: &str) -> Result<String, String> {
    let out = run(Command::new("unzip").arg("-p").arg(path).arg(name), "unzip", "it comes with macOS and most Linux")?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The text runs of a slide's XML, a paragraph a line.
pub fn slide_text(xml: &str) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == b"a:t" => in_text = true,
            Ok(Event::End(e)) if e.name().as_ref() == b"a:t" => in_text = false,
            Ok(Event::End(e)) if e.name().as_ref() == b"a:p" => {
                if !line.trim().is_empty() {
                    lines.push(line.trim().to_string());
                }
                line.clear();
            }
            Ok(Event::Text(t)) if in_text => line.push_str(&t.decode().unwrap_or_default()),
            // Entities come apart from the text around them.
            Ok(Event::GeneralRef(r)) if in_text => {
                if r.is_char_ref() {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        line.push(c);
                    }
                } else {
                    line.push_str(match r.decode().unwrap_or_default().as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if !line.trim().is_empty() {
        lines.push(line.trim().to_string());
    }
    lines.join("\n")
}

/// A PowerPoint's slides in order, each its text and then its notes.
pub fn slides(path: &Path) -> Result<Vec<String>, String> {
    if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pptx")) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("this kind of");
        return Err(format!("a .{ext} deck is not read: save it as .pptx or PDF"));
    }
    let names = zip_names(path)?;
    let number = |name: &str, prefix: &str| name.strip_prefix(prefix)?.strip_suffix(".xml")?.parse::<usize>().ok();
    let mut slides: Vec<(usize, String)> = names.iter().filter_map(|n| Some((number(n, "ppt/slides/slide")?, n.clone()))).collect();
    slides.sort_by_key(|(n, _)| *n);
    let mut pages = Vec::new();
    for (n, name) in slides {
        let mut page = slide_text(&zip_entry(path, &name)?);
        let notes = format!("ppt/notesSlides/notesSlide{n}.xml");
        if names.contains(&notes) {
            let said = slide_text(&zip_entry(path, &notes)?);
            // A notes page repeats the slide's number; that is not a note.
            let said: Vec<&str> = said.lines().filter(|l| l.trim() != n.to_string()).collect();
            if !said.is_empty() {
                page.push_str(&format!("\n\nNotes: {}", said.join(" ")));
            }
        }
        pages.push(page);
    }
    if pages.is_empty() {
        return Err("it has no slides".into());
    }
    Ok(pages)
}

/// A picture's words, as tesseract reads them.
pub fn picture(path: &Path) -> Result<String, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "tif" | "tiff" | "bmp" | "webp" | "heic") {
        return Err(format!("a .{ext} picture is not read"));
    }
    let out = run(Command::new("tesseract").arg(path).args(["stdout", "--psm", "3"]), "tesseract", "brew install tesseract")?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here(program: &str) -> bool {
        Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn a_slides_text_is_its_runs_a_paragraph_a_line() {
        let xml = r#"<p:sld xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree>
            <p:sp><p:txBody><a:p><a:r><a:t>Breadth-first </a:t></a:r><a:r><a:t>search</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:txBody><a:p><a:r><a:t>Complete when b &amp; d are finite</a:t></a:r></a:p><a:p></a:p></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sld>"#;
        assert_eq!(slide_text(xml), "Breadth-first search\nComplete when b & d are finite");
    }

    #[test]
    fn what_cannot_be_read_says_so() {
        assert!(document(Path::new("notes.pages")).unwrap_err().contains("save it as .docx or PDF"));
        assert!(slides(Path::new("deck.key")).unwrap_err().contains("save it as .pptx"));
        assert!(picture(Path::new("drawing.svg")).unwrap_err().contains("not read"));
    }

    #[test]
    fn a_pictures_words_are_read_by_tesseract_when_it_is_here() {
        if !here("tesseract") || !Command::new("magick").arg("-version").output().is_ok_and(|o| o.status.success()) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-library-picture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("slide.png");
        let drawn = Command::new("magick")
            .args(["-size", "900x160", "xc:white", "-fill", "black", "-pointsize", "64", "-annotate", "+20+100", "Uniform cost search"])
            .arg(&png)
            .status()
            .unwrap();
        assert!(drawn.success());
        let words = picture(&png).unwrap();
        assert!(words.to_lowercase().contains("uniform cost search"), "{words}");
    }

    #[test]
    fn a_word_document_and_a_powerpoint_read_through_pandoc_and_unzip_when_they_are_here() {
        if !here("pandoc") {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-library-office-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("in.md");
        std::fs::write(&md, "# Search\n\nBreadth-first search is complete when $b$ is finite.\n\n# Heuristics\n\n- Admissible\n\n::: notes\nSay it slowly.\n:::\n").unwrap();
        let docx = dir.join("notes.docx");
        assert!(Command::new("pandoc").arg(&md).arg("-o").arg(&docx).status().unwrap().success());
        let text = document(&docx).unwrap();
        assert!(text.contains("# Search") && text.contains("complete when $b$ is finite"), "{text}");
        let pptx = dir.join("deck.pptx");
        assert!(Command::new("pandoc").arg(&md).arg("-o").arg(&pptx).status().unwrap().success());
        if here("unzip") || Command::new("unzip").arg("-v").output().is_ok() {
            let pages = slides(&pptx).unwrap();
            assert!(pages.iter().any(|p| p.contains("Breadth-first search")), "{pages:?}");
            assert!(pages.iter().any(|p| p.contains("Notes: Say it slowly.")), "{pages:?}");
        }
    }
}
