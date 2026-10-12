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
"#;

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
    for (name, body) in [("latexmk", LATEXMK), ("pdflatex", "#!/bin/sh\n"), ("tlmgr", "#!/bin/sh\n")] {
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
