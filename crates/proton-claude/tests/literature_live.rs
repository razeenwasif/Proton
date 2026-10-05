//! Research's literature, live: the real `claude` and `codex`, unattended as
//! a round runs them, given Proton's own `proton-mcp --literature` server
//! and asked to search a paper with it. They cost a little of each plan, so
//! they are run by hand, with the server built:
//!
//! ```text
//! cargo build -p proton-mcp
//! cargo test -p proton-claude --test literature_live -- --ignored --nocapture
//! ```
//!
//! What they guard is what changed under us once already: Claude Code's
//! `--safe-mode` came to drop even the servers `--mcp-config` names, so a
//! call given the papers was given nothing, and said so politely.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_claude::engine::{self, Engine, McpServer, Unattended};
use proton_claude::fold::{Activity, Item};
use proton_claude::{Conversation, Settings};

/// An investigation of one paper, with no PDF: its abstract is what is searched.
fn investigation(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-literature-live-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("literature")).unwrap();
    std::fs::write(
        dir.join("literature/P001.md"),
        "# P001: Ozone and the twilight zenith\n\n## Abstract\n\nThe Chappuis absorption band of ozone keeps the zenith sky blue at twilight.\n",
    )
    .unwrap();
    dir
}

fn server(dir: &std::path::Path) -> McpServer {
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/proton-mcp");
    let command = std::env::var_os("PROTON_MCP").map(PathBuf::from).unwrap_or(built);
    assert!(command.is_file(), "build it first: cargo build -p proton-mcp ({})", command.display());
    McpServer {
        name: "proton-literature".into(),
        command,
        args: vec!["--literature".into(), dir.display().to_string()],
        tools: vec!["search_literature".into(), "literature_status".into()],
    }
}

/// Ask, and wait for the turn to end: what it answered.
fn ask(chat: &mut dyn Conversation, text: &str) -> String {
    chat.send(text, &[]).expect("sent");
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut started = false;
    while Instant::now() < deadline {
        chat.poll();
        let activity = chat.fold().status().activity;
        started |= activity != Activity::Idle;
        if started && activity == Activity::Idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = chat.take();
    assert!(chat.fold().status().error.is_none(), "{:?}\n{:#?}", chat.fold().status().error, chat.fold().items());
    let tools: Vec<String> = chat.fold().items().iter().filter_map(|i| if let Item::Tool(t) = i { Some(t.name.clone()) } else { None }).collect();
    println!("tools it used: {tools:?}");
    chat.fold().outcome().map(|o| o.text.clone()).unwrap_or_default()
}

const TASK: &str = "Search the investigation's papers with the search_literature tool for 'chappuis ozone', then answer with only the \
                    citation the passage came with, in double brackets.";

fn run(engine: Engine, name: &str, settings: Settings) {
    let dir = investigation(name);
    let how = Unattended { instructions: "You are a research assistant working unattended.".into(), mcp: vec![server(&dir)], ..Unattended::default() };
    let mut chat = engine::unattended(engine, dir.clone(), settings, &how).expect("an unattended conversation");
    let said = ask(chat.as_mut(), TASK);
    println!("{engine:?} said: {said}");
    assert!(said.contains("literature/P001.md"), "it did not read the paper through Proton's server: {said}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore]
fn claude_code_reads_the_papers_through_protons_server() {
    run(Engine::Claude, "claude", Settings { model: Some("claude-haiku-4-5".into()), mode: "default".into(), effort: None });
}

#[test]
#[ignore]
fn codex_reads_the_papers_through_protons_server() {
    run(Engine::Codex, "codex", Settings { model: None, mode: "default".into(), effort: Some("low".into()) });
}
