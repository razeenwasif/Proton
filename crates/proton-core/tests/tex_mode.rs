//! LaTeX is Codex's editor beside a PDF: text is typed, undone and opened
//! there as it is in Codex, and opening a file from LaTeX stays in LaTeX.

use proton_core::bus::Event;
use proton_core::mode::ModeId;
use proton_core::{Command, Key, Workspace};

fn keys(ws: &mut Workspace, typed: &str) {
    let mut rest = typed;
    while !rest.is_empty() {
        let key = if let Some(tail) = rest.strip_prefix("<Esc>") {
            rest = tail;
            Key::named("Escape")
        } else {
            let c = rest.chars().next().unwrap();
            rest = &rest[c.len_utf8()..];
            Key::char(c)
        };
        ws.apply(Command::Key { key });
    }
}

fn text(ws: &Workspace) -> String {
    ws.editor().buffer().contents()
}

#[test]
fn text_is_typed_and_undone_in_latex_as_in_codex_and_not_in_design() {
    let mut ws = Workspace::new();
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    keys(&mut ws, "i\\section{Intro}<Esc>");
    assert_eq!(text(&ws).trim_end(), "\\section{Intro}", "typed in LaTeX");
    keys(&mut ws, "u");
    assert_eq!(text(&ws).trim_end(), "", "undone in LaTeX");

    let mut design = Workspace::new();
    design.apply(Command::SetMode { mode: ModeId::Design });
    keys(&mut design, "ihello<Esc>");
    assert_eq!(text(&design).trim_end(), "", "Design does not edit text");
}

#[test]
fn a_file_jumped_to_from_latex_opens_in_latex() {
    let dir = std::env::temp_dir().join(format!("proton-tex-mode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.tex");
    std::fs::write(&main, "\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n").unwrap();
    let mut ws = Workspace::new();
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    let events = ws.apply(Command::JumpTo { mode: ModeId::Tex, path: main.display().to_string(), line: 2, column: 0 });
    assert!(events.iter().any(|e| matches!(e, Event::Buffers { .. })), "{events:?}");
    assert_eq!(ws.mode(), ModeId::Tex, "it stays in LaTeX");
    let buf = ws.editor().buffer();
    assert_eq!(buf.position(buf.cursor.offset).line, 2);
    keys(&mut ws, "AHi<Esc>");
    assert!(text(&ws).contains("Hello.Hi"), "and is edited there: {}", text(&ws));
    let _ = std::fs::remove_dir_all(dir);
}
