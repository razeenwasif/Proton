//! A LaTeX project typeset on save: its main file found from the file being
//! edited, latexmk run — a stand-in here — with Proton's TeX, the build's
//! folder outside the project, the PDF copied where the window may read it,
//! and the log read into errors where they are.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::mode::ModeId;
use proton_core::tex::TexBuild;
use proton_core::session::Session;
use proton_core::{Command, Workspace};
use proton_lab::tex::{Bundle, Marker, Platform, TexEnv, Tools};

/// latexmk, standing in: an error where the main file says `\foo`, else a
/// one-page PDF — written where `-outdir` says, as the real one does.
const LATEXMK: &str = r#"#!/bin/sh
out=""; main=""
for a in "$@"; do case "$a" in -outdir=*) out="${a#-outdir=}" ;; -*) ;; *) main="$a" ;; esac; done
stem=$(basename "$main" .tex)
if grep -q 'foo' "$main"; then
  printf '(%s\n./%s.tex:3: Undefined control sequence.\nl.3 \\foo\n)\n' "$main" "$stem" > "$out/$stem.log"
  exit 12
fi
printf '%%PDF-1.5\n%%stand-in\n' > "$out/$stem.pdf"
printf '(%s\nLaTeX Warning: Citation `knuth84'"'"' on page 1 undefined on input line 4.\n)\nOutput written on %s.pdf (1 page, 900 bytes).\n' "$main" "$stem" > "$out/$stem.log"
printf 'SyncTeX Version:1\nInput:1:%s\nUnit:1\nContent:\n{1\n(1,3:4736286,6578176:19734528,657817,131563\n)\n}1\n' "$main" > "$out/$stem.synctex.gz"
"#;

/// kpsewhich, standing in: TeX has every file asked for but natbib's.
const KPSEWHICH: &str = "#!/bin/sh\nfor f in \"$@\"; do case $f in natbib.sty) ;; *) echo /tex/dist/$f ;; esac; done\n";

struct Bench {
    base: PathBuf,
    project: PathBuf,
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A project with a main file and a chapter, and a TeX made — as far as its
/// marker says — whose latexmk is the stand-in.
fn bench(tag: &str) -> (Bench, TexEnv) {
    let base = std::env::temp_dir().join(format!("proton-tex-build-core-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("paper");
    std::fs::create_dir_all(project.join("chapters")).unwrap();
    std::fs::write(project.join("main.tex"), "\\documentclass{article}\n\\begin{document}\n\\input{chapters/intro}\n\\end{document}\n").unwrap();
    std::fs::write(project.join("chapters/intro.tex"), "Hello.\n").unwrap();
    let home = base.join("tex");
    let bin = home.join("TinyTeX/bin/universal-darwin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, body) in [("latexmk", LATEXMK), ("pdflatex", "#!/bin/sh\n"), ("tlmgr", "#!/bin/sh\n"), ("kpsewhich", KPSEWHICH)] {
        std::fs::write(bin.join(name), body).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::create_dir_all(home.join("texlab")).unwrap();
    std::fs::write(home.join("texlab/texlab"), "#!/bin/sh\n").unwrap();
    let marker = Marker { tinytex: "v2026.10".into(), texlab: "v5.26.0".into(), bundle: Bundle::Full, bin, texlab_bin: home.join("texlab/texlab"), size: 1, made: 1 };
    std::fs::write(home.join("ready.json"), serde_json::to_string(&marker).unwrap()).unwrap();
    let tools = Tools { curl: Some("/usr/bin/curl".into()), tar: Some("/usr/bin/tar".into()), perl: Some("/usr/bin/perl".into()), xz: None };
    let env = TexEnv::new(&home, std::sync::Arc::new(proton_lab::github::Offline), Some(Platform { tinytex: "darwin", texlab: "aarch64-macos" }), tools)
        .with_person(&base)
        .with_sandbox(proton_lab::Sandbox::None);
    (Bench { base, project }, env)
}

fn open(ws: &mut Workspace, path: &Path) {
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: path.display().to_string(), line: 0, column: 0 });
}

/// Beat until the build ends.
fn built(ws: &mut Workspace) -> (TexBuild, Option<proton_tex::log::Log>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let events = ws.apply(Command::Poll);
        let build = events.iter().find_map(|e| match e {
            Event::TexBuild { build } if build.state != "running" => Some(build.clone()),
            _ => None,
        });
        if let Some(build) = build {
            let log = events.iter().find_map(|e| match e {
                Event::TexLog { log, .. } => Some(log.clone()),
                _ => None,
            });
            return (build, log);
        }
        assert!(Instant::now() < deadline, "the build never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn saving_a_chapter_typesets_the_main_file_and_its_pdf_and_log_are_said() {
    let (b, env) = bench("saved");
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    open(&mut ws, &b.project.join("chapters/intro.tex"));
    let events = ws.apply(Command::Save);
    let running = events.iter().find_map(|e| match e {
        Event::TexBuild { build } => Some(build.clone()),
        _ => None,
    });
    let running = running.expect("a save typesets");
    assert_eq!((running.state.as_str(), running.main.as_deref(), running.engine.as_str()), ("running", Some("main.tex"), "pdfLaTeX"));
    assert!(!events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("format"))), "a .tex is not formatted on save");

    let (build, log) = built(&mut ws);
    assert_eq!((build.state.as_str(), build.pages, build.warnings, build.version), ("done", Some(1), 1, 1), "{build:?}");
    let pdf = PathBuf::from(build.pdf.clone().unwrap());
    assert!(pdf.is_file() && pdf.ends_with("view/main.pdf") && !pdf.starts_with(&b.project), "the PDF is outside the project: {pdf:?}");
    assert_eq!(ws.document_file(&pdf), Some(pdf.clone()), "the window may read it");
    let log = log.expect("the log is said");
    assert_eq!((log.entries[0].file.as_deref(), log.entries[0].line), (Some("main.tex"), Some(4)));
    let left: Vec<_> = std::fs::read_dir(&b.project).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(left.len(), 2, "nothing written into the project: {left:?}");
    // Until asked: then the PDF goes beside the main file.
    let events = ws.apply(Command::TexSavePdf);
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("saved main.pdf"))), "{events:?}");
    assert!(b.project.join("main.pdf").is_file());
}

#[test]
fn an_error_is_said_where_it_is_and_the_last_pdf_is_kept() {
    let (b, env) = bench("error");
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    open(&mut ws, &b.project.join("main.tex"));
    ws.apply(Command::TexCompile);
    let (first, _) = built(&mut ws);
    assert_eq!(first.state, "done");
    std::fs::write(b.project.join("main.tex"), "\\documentclass{article}\n\\begin{document}\n\\foo\n\\end{document}\n").unwrap();
    ws.apply(Command::TexCompile);
    let (second, log) = built(&mut ws);
    assert_eq!((second.state.as_str(), second.errors), ("failed", 1), "{second:?}");
    assert_eq!(second.pdf, first.pdf, "the PDF before is still the one shown");
    let error = &log.unwrap().entries[0];
    assert_eq!((error.file.as_deref(), error.line, error.context.as_deref()), (Some("main.tex"), Some(3), Some("\\foo")));
}

#[test]
fn without_tex_or_a_main_file_it_says_so_rather_than_typesetting() {
    let (b, env) = bench("none");
    std::fs::remove_file(b.base.join("tex/ready.json")).unwrap();
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    let events = ws.apply(Command::TexCompile);
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("Install TeX"))), "{events:?}");

    let (b2, env2) = bench("nomain");
    std::fs::write(b2.project.join("main.tex"), "no document here\n").unwrap();
    let mut ws2 = Workspace::rooted(&b2.project).with_tex_env(env2);
    ws2.apply(Command::SetMode { mode: ModeId::Tex });
    let events = ws2.apply(Command::TexCompile);
    assert!(events.iter().any(|e| matches!(e, Event::TexBuild { build } if build.state == "noMain")), "{events:?}");
}

#[test]
fn a_package_the_document_lacks_is_found_installed_when_agreed_and_the_document_typeset_again() {
    let (b, env) = bench("package");
    let bin = b.base.join("tex/TinyTeX/bin/universal-darwin");
    // latexmk lacks tikz-cd until tlmgr has installed it.
    let latexmk = r#"#!/bin/sh
out=""; main=""
for a in "$@"; do case "$a" in -outdir=*) out="${a#-outdir=}" ;; -*) ;; *) main="$a" ;; esac; done
stem=$(basename "$main" .tex)
if [ ! -f "$(dirname "$0")/tikz-cd-installed" ]; then
  printf './%s.tex:2: LaTeX Error: File `tikz-cd.sty'"'"' not found.\n' "$stem" > "$out/$stem.log"; exit 12
fi
printf '%%PDF-1.5\n' > "$out/$stem.pdf"; printf 'Output written on %s.pdf (1 page).\n' "$stem" > "$out/$stem.log"
"#;
    let tlmgr = r#"#!/bin/sh
case "$1" in
  search) printf 'tlmgr: package repository https://mirror (verified)\ntikz-cd:\n\ttexmf-dist/tex/latex/tikz-cd/tikz-cd.sty\n' ;;
  install) touch "$(dirname "$0")/tikz-cd-installed" ;;
esac
"#;
    std::fs::write(bin.join("latexmk"), latexmk).unwrap();
    std::fs::write(bin.join("tlmgr"), tlmgr).unwrap();
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    open(&mut ws, &b.project.join("main.tex"));
    ws.apply(Command::TexCompile);
    let (build, log) = built(&mut ws);
    assert_eq!(build.state, "failed");
    assert_eq!(log.unwrap().missing, ["tikz-cd.sty"]);

    let packages = |events: &[Event]| events.iter().rev().find_map(|e| match e {
        Event::TexPackages { packages } => Some(packages.clone()),
        _ => None,
    });
    let events = ws.apply(Command::TexFindPackages { files: vec!["tikz-cd.sty".into()] });
    assert!(matches!(packages(&events), Some(proton_lab::tex::tlmgr::Packages::Finding { .. })), "{events:?}");
    let deadline = Instant::now() + Duration::from_secs(10);
    let found = loop {
        if let Some(found @ proton_lab::tex::tlmgr::Packages::Found { .. }) = packages(&ws.apply(Command::Poll)) {
            break found;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let proton_lab::tex::tlmgr::Packages::Found { packages: found, .. } = found else { unreachable!() };
    assert_eq!(found[0].name, "tikz-cd");
    assert!(!bin.join("tikz-cd-installed").exists(), "nothing installed before the second say-so");

    ws.apply(Command::TexInstallPackages { packages: vec!["tikz-cd".into()] });
    let (again, _) = built(&mut ws);
    assert_eq!(again.state, "done", "typeset again once the package is in: {again:?}");
}

#[test]
fn a_line_is_found_in_the_pdf_and_a_point_in_the_pdf_found_in_the_source() {
    let (b, env) = bench("sync");
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    open(&mut ws, &b.project.join("main.tex"));
    let early = ws.apply(Command::TexSyncForward);
    assert!(early.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("Typeset"))), "{early:?}");
    ws.apply(Command::TexCompile);
    built(&mut ws);
    // The cursor on the third line, which made the box at the top of page 1.
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("main.tex").display().to_string(), line: 2, column: 0 });
    let events = ws.apply(Command::TexSyncForward);
    let found = events.iter().find_map(|e| match e {
        Event::TexSync { found } => Some(found.clone()),
        _ => None,
    });
    let found = found.unwrap_or_else(|| panic!("{events:?}"));
    assert_eq!(found.page, 1);
    assert!((found.rects[0].y - 90.0).abs() < 0.5 && (found.rects[0].w - 300.0).abs() < 0.5, "{found:?}");

    // From the PDF back: the cursor goes to that line, in LaTeX.
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("chapters/intro.tex").display().to_string(), line: 0, column: 0 });
    ws.apply(Command::TexSyncInverse { page: 1, x: 100.0, y: 95.0 });
    let buf = ws.editor().buffer();
    assert!(buf.path().is_some_and(|p| p.ends_with("main.tex")), "{:?}", buf.path());
    assert_eq!(buf.position(buf.cursor.offset).line, 2);
    assert_eq!(ws.mode(), ModeId::Tex);
}

fn said(events: &[Event], what: &str) -> bool {
    events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains(what)))
}

#[test]
fn a_new_project_from_a_template_is_opened_in_latex_on_its_main_file_and_typeset() {
    let (b, env) = bench("new");
    let mut session = Session::new(&b.project).with_tex_env(env);
    session.apply(Command::SetMode { mode: ModeId::Tex });
    let parent = b.base.join("papers");
    let events = session.apply(Command::TexNew { template: "article".into(), parent: parent.display().to_string(), name: "An essay".into() });
    let folder = parent.join("An essay");
    assert!(said(&events, "Made"), "{events:?}");
    assert_eq!(session.workspaces().len(), 2, "a workspace of its own");
    let ws = session.active();
    assert_eq!(ws.root(), std::fs::canonicalize(&folder).unwrap());
    assert_eq!(ws.mode(), ModeId::Tex);
    assert_eq!(ws.editor().buffer().path().map(|p| p.file_name().unwrap().to_owned()), Some("main.tex".into()));
    assert!(std::fs::read_to_string(folder.join("main.tex")).unwrap().contains("\\title{An essay}"));
    let running = events.iter().find_map(|e| match e {
        Event::TexBuild { build } => Some(build.clone()),
        _ => None,
    });
    assert_eq!(running.map(|r| r.state), Some("running".into()), "nothing lacking: typeset at once — {events:?}");
    assert!(events.iter().any(|e| matches!(e, Event::TexOutline { main: Some(m), outline } if m == "main.tex" && outline.headings.len() == 2)), "{events:?}");

    // The same folder again: refused, and nothing opened.
    let events = session.apply(Command::TexNew { template: "cv".into(), parent: parent.display().to_string(), name: "An essay".into() });
    assert!(said(&events, "not empty"), "{events:?}");
    assert_eq!(session.workspaces().len(), 2);
}

#[test]
fn a_template_that_wants_what_tex_lacks_offers_it_first_and_is_typeset_once_installed() {
    let (b, env) = bench("template");
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    let events = ws.apply(Command::SetMode { mode: ModeId::Tex });
    let templates = events.iter().find_map(|e| match e {
        Event::TexTemplates { templates, .. } => Some(templates.clone()),
        _ => None,
    });
    assert_eq!(templates.map(|t| t.len()), Some(6), "entering LaTeX shows the gallery's templates");

    // The paper wants natbib, which this TeX lacks: offered, not typeset.
    let events = ws.apply(Command::TexAddTemplate { template: "paper".into() });
    assert!(said(&events, "added paper/main.tex, paper/references.bib"), "the project's own main.tex is not written over: {events:?}");
    assert_eq!(std::fs::read_to_string(b.project.join("main.tex")).unwrap().lines().count(), 4);
    assert!(ws.editor().buffer().path().is_some_and(|p| p.ends_with("paper/main.tex")));
    let missing = events.iter().find_map(|e| match e {
        Event::TexLog { log, .. } => Some(log.missing.clone()),
        _ => None,
    });
    assert_eq!(missing, Some(vec!["natbib.sty".to_string()]), "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, Event::TexBuild { .. })), "not typeset while it lacks a package");

    // Installed — tlmgr standing in — and the paper is typeset, not the project's main.tex.
    ws.apply(Command::TexInstallPackages { packages: vec!["natbib".into()] });
    let deadline = Instant::now() + Duration::from_secs(10);
    let running = loop {
        let events = ws.apply(Command::Poll);
        if let Some(build) = events.iter().find_map(|e| match e {
            Event::TexBuild { build } => Some(build.clone()),
            _ => None,
        }) {
            break build;
        }
        assert!(Instant::now() < deadline, "not typeset once installed");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(running.main.as_deref(), Some("paper/main.tex"));
    let (build, _) = built(&mut ws);
    assert_eq!(build.state, "done", "{build:?}");
}

#[test]
fn the_agent_reads_the_last_build_s_errors_with_the_source_round_them_unsaved_edits_and_all() {
    let (b, env) = bench("agent");
    let mut ws = Workspace::rooted(&b.project).with_tex_env(env);
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    let before = ws.run_tool("latex_log", &serde_json::json!({})).unwrap();
    assert!(before.contains("Nothing has been typeset"), "{before}");

    // The stand-in fails at line 3 of a main file that says \foo.
    std::fs::write(b.project.join("main.tex"), "\\documentclass{article}\n\\begin{document}\n\\foo\n\\end{document}\n").unwrap();
    open(&mut ws, &b.project.join("main.tex"));
    ws.apply(Command::Save);
    let (build, _) = built(&mut ws);
    assert_eq!(build.errors, 1, "{build:?}");
    // Edited since, not saved: the agent sees what is in the editor.
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("main.tex").display().to_string(), line: 3, column: 0 });
    for key in "OUnsaved.".chars() {
        ws.apply(Command::Key { key: proton_core::Key::char(key) });
    }
    ws.apply(Command::Key { key: proton_core::Key::named("Escape") });
    let text = ws.run_tool("latex_log", &serde_json::json!({})).unwrap();
    assert!(text.starts_with("`main.tex`, typeset with pdfLaTeX in"), "{text}");
    assert!(text.contains("made no PDF: 1 error"), "{text}");
    assert!(text.contains("1. `main.tex` line 3: Undefined control sequence."), "{text}");
    assert!(text.contains("TeX stopped at: \\foo"), "{text}");
    assert!(text.contains(">   3  \\foo") && text.contains("Unsaved."), "the source round it, as the editor has it: {text}");
}
