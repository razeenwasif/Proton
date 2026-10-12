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

/// A TeX that never reaches the network, in a folder of its own.
fn offline_tex(home: &std::path::Path) -> proton_lab::tex::TexEnv {
    let tools = proton_lab::tex::Tools { curl: Some("/usr/bin/curl".into()), tar: Some("/usr/bin/tar".into()), perl: Some("/usr/bin/perl".into()), xz: Some("/usr/bin/xz".into()) };
    let platform = proton_lab::tex::Platform { tinytex: "darwin", texlab: "aarch64-macos" };
    proton_lab::tex::TexEnv::new(home, std::sync::Arc::new(proton_lab::github::Offline), Some(platform), tools).with_person(&std::env::temp_dir())
}

fn tex_state(events: &[Event]) -> Option<proton_lab::tex::TexState> {
    events.iter().rev().find_map(|e| match e {
        Event::TexEnv { env } => Some(env.clone()),
        _ => None,
    })
}

#[test]
fn latex_says_where_tex_stands_and_asks_before_installing_anything() {
    use proton_lab::tex::{Bundle, TexState};
    let home = std::env::temp_dir().join(format!("proton-tex-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let mut ws = Workspace::new().with_tex_env(offline_tex(&home));
    let events = ws.apply(Command::SetMode { mode: ModeId::Tex });
    assert!(matches!(tex_state(&events), Some(TexState::Missing { needs, .. }) if needs.is_empty()), "{events:?}");
    assert!(!home.exists(), "nothing is made by looking");

    // The first say-so asks GitHub — here, nobody — and says what came of it.
    let events = ws.apply(Command::TexPlan { bundle: Bundle::Full });
    assert_eq!(tex_state(&events), Some(TexState::Planning));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let failed = loop {
        if let Some(state @ TexState::Failed { .. }) = tex_state(&ws.apply(Command::Poll)) {
            break state;
        }
        assert!(std::time::Instant::now() < deadline, "the plan never ended");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(matches!(&failed, TexState::Failed { step, why, .. } if step == "plan" && why.contains("not asked")), "{failed:?}");
    let events = ws.apply(Command::TexCreate);
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("no plan"))), "installing needs a plan: {events:?}");
    let events = ws.apply(Command::TexCancel);
    assert!(matches!(tex_state(&events), Some(TexState::Missing { .. })));
    assert!(tex_state(&ws.apply(Command::Poll)).is_none(), "nothing new is drawn on a quiet beat");
}
