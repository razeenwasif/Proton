//! Proton's library, offered to anything that speaks MCP.
//!
//! ## Why this exists
//!
//! The agent pane can search the user's notes and cite them. Nothing else can
//! — not the Claude CLI, not the Gemini CLI, not any of the other clients they
//! already pay for. That is the wrong way round: the library is the part of
//! Proton worth having everywhere, and it is also the part that costs nothing
//! to run, because retrieval here is BM25 over local files rather than a call
//! to anybody's API.
//!
//! So it is served over MCP, which is the supported way for those clients to
//! reach a capability the machine has. The model answering stays whichever one
//! the user is signed in to, on whatever plan they already pay for; this only
//! hands it better material to answer from.
//!
//! ## How it gets into an install
//!
//! As a bundle resource named in `tauri.conf.json`, not as a dependency of the
//! shell — nothing links against it, it is a separate process. So cargo does
//! not build it when the shell is built, and for one release it did not:
//! `tauri build` bundled whatever binary happened to be sitting in
//! `target/release` from some earlier run. A fix can be committed, built and
//! installed and still not reach the server that runs.
//!
//! The `beforeBuildCommand` in that config is what stops it. JSON cannot hold
//! the reason, so it is here.
//!
//! ## Running it
//!
//! ```bash
//! claude --mcp-config '{"mcpServers":{"proton":{"command":"proton-mcp","args":["--project","C:/Users/you/Project"]}}}'
//! ```
//!
//! The project decides which `.proton/settings.json` is read, and that file
//! decides which directories may be searched. Defaults to the working
//! directory, which is what a client launching it from a project will give.
//!
//! ## What it does not do
//!
//! It does not reach into a running Proton. It reads the same settings and the
//! same documents from disk and builds its own index, which means it sees
//! saved files rather than unsaved buffers. For a library of notes and papers
//! that is the same thing; for open buffers it would not be, which is why the
//! editing tools are deliberately not here. What is here is the part that is
//! purely a question about what the user has written down.

mod rpc;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rpc::Call;
use serde_json::{json, Value};

/// The revision of MCP this speaks.
///
/// Echoed back to a client that asks for something else, rather than refused:
/// the parts used here — `initialize`, `tools/list`, `tools/call` — have been
/// stable across revisions, and a version disagreement should not stop a
/// search from working.
const PROTOCOL: &str = "2025-06-18";

/// How long a search waits for the library to become usable at all.
///
/// The walk starts at boot and runs on its own thread, so `initialize` answers
/// at once and the client does not sit through it. A question asked in the
/// meantime waits here instead — a slow answer is worth having and "your notes
/// say nothing about that" is not.
const PATIENCE: Duration = Duration::from_secs(600);

/// How much longer it waits for the *whole* walk, PDFs included.
///
/// A walk answers twice: the notes in a few seconds, the PDFs after. With a
/// warm cache the second arrives about two seconds later, so waiting costs
/// nothing and the alternative is a first search that silently has no papers
/// in it. Cold, extracting a corpus of PDFs runs to minutes — far longer than
/// a client will hold a tool call open — so after this the search answers with
/// what it has and says what is missing.
const GRACE: Duration = Duration::from_secs(25);

/// How much of the library a search got to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ready {
    /// Everything the roots hold.
    All,
    /// The notes, with documents still being read.
    Some,
    /// Nothing yet.
    None,
}

fn main() {
    let project = project_from_args();
    let settings = proton_core::settings::Settings::load(&project);
    let roots = settings.library_roots(&project);

    // Reported on stderr, which the client shows as server logs. Never
    // stdout: that carries the protocol, and one stray line ends the session.
    eprintln!(
        "proton-mcp: project {}, {} library root{}",
        project.display(),
        roots.len(),
        if roots.len() == 1 { "" } else { "s" },
    );
    for root in &roots {
        eprintln!("  {}", root.display());
    }

    let mut shelf = proton_library::Shelf::new();
    shelf.cache_in(proton_library::pdf::cache_dir());
    shelf.set_pdfs(settings.bool("library.pdfs", true));
    // A client keeps this process alive for the length of a session, and notes
    // get written during one.
    shelf.set_watching(settings.bool("library.watch", true));
    // Whatever the editor's settings say, so the same library answers the
    // same way in both places. Off unless a model is named.
    let model = settings.string("library.embeddings", "");
    let embedder = (!model.is_empty()).then(|| {
        eprintln!("proton-mcp: embedding with {model}");
        proton_library::Embedder::new(&model).caching_in(proton_library::pdf::cache_dir())
    });
    shelf.embed_with(embedder.clone());
    // Starts the walk on a worker and returns. The first question waits for
    // it; `initialize` does not.
    shelf.set_roots(roots);

    serve(&mut shelf, embedder.as_ref());
}

/// `--project <path>`, or the working directory.
fn project_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--project" {
            if let Some(path) = args.next() {
                return PathBuf::from(path);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn serve(shelf: &mut proton_library::Shelf, embedder: Option<&proton_library::Embedder>) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Some(call) = Call::parse(&line) else {
            continue;
        };
        let Some(answer) = handle(&call, shelf, embedder) else {
            continue;
        };
        // Flushed every time. A buffered answer is an answer the client waits
        // for forever, and stdio has no other signal that a message is done.
        let _ = writeln!(out, "{answer}");
        let _ = out.flush();
    }
}

fn handle(
    call: &Call,
    shelf: &mut proton_library::Shelf,
    embedder: Option<&proton_library::Embedder>,
) -> Option<Value> {
    // A notification is acted on and never answered.
    if !call.wants_an_answer() {
        return None;
    }
    let id = call.id.clone()?;

    Some(match call.method.as_str() {
        "initialize" => rpc::result(
            &id,
            json!({
                "protocolVersion": call.params.get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "proton-library", "version": env!("CARGO_PKG_VERSION") },
            }),
        ),

        "tools/list" => rpc::result(&id, json!({ "tools": tools() })),

        "tools/call" => {
            let name = call
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = call
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match name {
                "search_library" => {
                    let (text, failed) = search(shelf, embedder, &args);
                    rpc::result(&id, rpc::tool_output(text, failed))
                }
                "library_status" => rpc::result(&id, rpc::tool_output(status(shelf), false)),
                "library_sources" => rpc::result(&id, rpc::tool_output(sources(shelf), false)),
                other => rpc::failure(&id, rpc::INVALID_PARAMS, &format!("no tool named {other}")),
            }
        }

        // Answered rather than ignored: a client that pings and hears nothing
        // decides the server is gone.
        "ping" => rpc::result(&id, json!({})),

        other => rpc::failure(&id, rpc::METHOD_NOT_FOUND, &format!("no method {other}")),
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "search_library",
            "description": "Search the user's own notes and documents — their vault, course \
                            notes, papers and slide decks — for passages about something. This \
                            is NOT the code in the current project: use your own file tools for \
                            that, and this for what the user has written down or kept. Ask in \
                            the words the notes would use, not the words of the question; \
                            matching is lexical, so the exact terms matter. Each result begins \
                            with a citation in double brackets. Quote it back exactly as given, \
                            [[like this]], whenever you use what a passage says — it is how the \
                            user checks you. A citation ending in L12-34 is lines of a note; one \
                            ending in p. 4 is a page of a PDF whose text was extracted and may \
                            be imperfect.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Terms to look for." },
                    "limit": { "type": "integer", "description": "Defaults to 6." },
                    "sources": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Narrow the search to these folders or documents, named                                         exactly as library_sources lists them. Use it when the                                         question belongs to one subject: the library spans                                         several, matching is by word rather than by topic, and                                         the same term means different things in different                                         fields. Omit to search everything."
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "library_sources",
            "description": "The folders the library is made of, with how many documents and                             passages are in each. Call this first when a question clearly                             belongs to one subject, then pass the folder to search_library as                             `sources` — it is how you ask a question of one field rather than                             of everything the user has ever written down.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "library_status",
            "description": "What the library holds and where it came from: how many documents \
                            and passages, from which directories, and whether it is still being \
                            read. Worth calling once if a search comes back empty and you want \
                            to know whether the library is empty or merely does not cover the \
                            subject.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn search(
    shelf: &mut proton_library::Shelf,
    embedder: Option<&proton_library::Embedder>,
    args: &Value,
) -> (String, bool) {
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return (
            "search_library: `query` is required and must be a string".into(),
            true,
        );
    };
    if query.trim().is_empty() {
        return ("search_library: the query is empty".into(), true);
    }
    if shelf.roots().is_empty() {
        return (
            "there is no library: no roots are listed under `library.roots` in this project's \
             .proton/settings.json, so there are no notes to search"
                .into(),
            true,
        );
    }
    let ready = ready(shelf);
    if ready == Ready::None {
        return (
            "the library is still being read — ask again in a moment".into(),
            true,
        );
    }
    // Said on any answer that could be incomplete, rather than only when
    // nothing was found: a plausible-looking result drawn from half a library
    // is more misleading than an empty one.
    let partial = match ready {
        Ready::Some => {
            "\n\n---\n\n(Some documents are still being read — mostly PDFs, which take a \
             while the first time. Ask again shortly if this looks thin.)"
        }
        _ => "",
    };

    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(6)
        .clamp(1, 25);

    let scope = proton_library::Scope::of(
        args.get("sources")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    // Blocking on the embed is right here in a way it is not in the editor:
    // this process answers one question at a time down a pipe, and there is no
    // window to freeze. A model that is not running costs a moment and then
    // the lexical answer, which is the answer this tool gave before vectors
    // existed.
    let vector = embedder
        .filter(|_| shelf.has_vectors())
        .and_then(|one| one.vectors(&[query.to_string()]).ok())
        .and_then(|mut all| all.pop())
        .unwrap_or_default();
    let hits = match vector.is_empty() {
        true => shelf.search_in(query, limit, &scope),
        false => shelf.search_both(query, &vector, limit, &scope),
    };
    if hits.is_empty() {
        return (
            format!(
                "nothing in the library about {query:?}{}{partial}",
                match scope.is_everything() {
                    true => format!(" ({} passages searched)", shelf.len()),
                    // Said, because the likeliest reason an answer is empty is
                    // that the question was put to the wrong shelf.
                    false => format!(
                        " within {} — try again without `sources` to search everything",
                        scope.prefixes().join(", ")
                    ),
                }
            ),
            false,
        );
    }
    // The citation on its own line above the passage, so what to quote back is
    // unambiguous even after the text runs to sixty lines.
    let out = hits
        .iter()
        .map(|hit| format!("[[{}]]\n{}", hit.cite, hit.passage.text))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    (format!("{out}{partial}"), false)
}

fn status(shelf: &mut proton_library::Shelf) -> String {
    if shelf.roots().is_empty() {
        return "no library roots are listed in this project's .proton/settings.json".into();
    }
    let done = ready(shelf) == Ready::All;
    let roots = shelf
        .roots()
        .iter()
        .map(|r| format!("  {}", r.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let missing = match shelf.missing().is_empty() {
        true => String::new(),
        false => format!(
            "\nnamed but not found:\n{}",
            shelf
                .missing()
                .iter()
                .map(|r| format!("  {}", r.display()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    format!(
        "{} passages from {} documents{}\nroots:\n{roots}{missing}",
        shelf.len(),
        shelf.documents(),
        match done {
            true => String::new(),
            false => " (still reading)".into(),
        },
    )
}

fn sources(shelf: &mut proton_library::Shelf) -> String {
    if ready(shelf) == Ready::None {
        return "the library is still being read — ask again in a moment".into();
    }
    let sources = shelf.sources();
    if sources.is_empty() {
        return "the library is empty".into();
    }
    let rows = sources
        .iter()
        .map(|s| {
            format!(
                "  {}  ({} document{}, {} passages)",
                s.path,
                s.documents,
                if s.documents == 1 { "" } else { "s" },
                s.passages
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    format!(
        "pass any of these to search_library as `sources`:
{rows}"
    )
}

/// Wait for the walk, and say how much of it landed.
///
/// Polled rather than blocked on a channel because that is the only interface
/// the shelf offers — it was built for an editor with a heartbeat and no
/// threads of its own, and this borrows it unchanged rather than growing a
/// second way to wait.
fn ready(shelf: &mut proton_library::Shelf) -> Ready {
    let usable = Instant::now() + PATIENCE;
    while !shelf.is_built() {
        shelf.poll();
        if Instant::now() >= usable {
            return Ready::None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    // Usable now. Give the slow half a little longer, because with a warm
    // cache it is seconds away, and answering without the papers in it would
    // be answering from half a library without saying so.
    let whole = Instant::now() + GRACE;
    while shelf.is_building() {
        shelf.poll();
        if Instant::now() >= whole {
            return Ready::Some;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ready::All
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf() -> proton_library::Shelf {
        proton_library::Shelf::new()
    }

    fn call(method: &str, params: Value) -> Call {
        Call::parse(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
            .unwrap()
    }

    #[test]
    fn initialize_agrees_with_whatever_revision_the_client_asked_for() {
        // The parts used here have been stable across revisions, and a version
        // disagreement should not stop a search from working.
        let out = handle(
            &call("initialize", json!({ "protocolVersion": "2024-11-05" })),
            &mut shelf(),
            None,
        )
        .unwrap();
        assert_eq!(out["result"]["protocolVersion"], json!("2024-11-05"));
        assert!(out["result"]["capabilities"]["tools"].is_object());
        assert_eq!(out["result"]["serverInfo"]["name"], json!("proton-library"));
    }

    #[test]
    fn the_tools_are_listed_with_schemas() {
        let out = handle(&call("tools/list", json!({})), &mut shelf(), None).unwrap();
        let tools = out["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            ["search_library", "library_sources", "library_status"]
        );
        // The one that makes narrowing usable: the model cannot pass a folder
        // it has no way to learn the name of.
        let search = &tools[0];
        assert!(search["inputSchema"]["properties"]["sources"].is_object());
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], json!("object"));
            // The description is the only thing the model has to decide
            // whether this is the right place to look.
            assert!(tool["description"].as_str().unwrap().len() > 80);
        }
    }

    #[test]
    fn a_notification_is_never_answered() {
        // Replying to one is a protocol error, not a harmless extra.
        let note =
            Call::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(handle(&note, &mut shelf(), None).is_none());
    }

    #[test]
    fn an_unknown_method_is_an_error_and_an_unknown_tool_is_too() {
        let out = handle(&call("wat", json!({})), &mut shelf(), None).unwrap();
        assert_eq!(out["error"]["code"], json!(rpc::METHOD_NOT_FOUND));

        let out = handle(
            &call("tools/call", json!({ "name": "delete_everything" })),
            &mut shelf(),
            None,
        )
        .unwrap();
        assert_eq!(out["error"]["code"], json!(rpc::INVALID_PARAMS));
    }

    #[test]
    fn searching_with_no_roots_says_so_rather_than_finding_nothing() {
        // "Your notes say nothing about that" and "you have not told me where
        // your notes are" are different answers, and only one is true.
        let out = handle(
            &call(
                "tools/call",
                json!({ "name": "search_library", "arguments": { "query": "sparsity" } }),
            ),
            &mut shelf(),
            None,
        )
        .unwrap();
        let text = out["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("no library"), "{text}");
        assert_eq!(out["result"]["isError"], json!(true));
    }

    #[test]
    fn a_missing_query_is_a_result_rather_than_a_broken_conversation() {
        // The model has to read it and try again; a protocol error would end
        // the exchange instead.
        let out = handle(
            &call("tools/call", json!({ "name": "search_library" })),
            &mut shelf(),
            None,
        )
        .unwrap();
        assert!(out.get("error").is_none(), "{out}");
        assert_eq!(out["result"]["isError"], json!(true));
    }

    #[test]
    fn a_scope_that_matches_nothing_says_how_to_widen_it() {
        // The likeliest reason a scoped answer is empty is that the question
        // was put to the wrong shelf, and the model has to be told it can ask
        // again rather than concluding the user has written nothing.
        let dir = std::env::temp_dir().join(format!(
            "proton-mcp-scope-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ml")).unwrap();
        std::fs::write(
            dir.join("ml/One.md"),
            "# One
gradients and descent
",
        )
        .unwrap();

        let mut shelf = proton_library::Shelf::new();
        shelf.set_roots(vec![dir]);
        while !shelf.is_built() {
            shelf.poll();
            std::thread::sleep(Duration::from_millis(10));
        }

        let out = handle(
            &call(
                "tools/call",
                json!({
                    "name": "search_library",
                    "arguments": { "query": "gradients", "sources": ["physics"] }
                }),
            ),
            &mut shelf,
            None,
        )
        .unwrap();
        let text = out["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("within physics"), "{text}");
        assert!(text.contains("without `sources`"), "{text}");
    }

    #[test]
    fn ping_is_answered() {
        let out = handle(&call("ping", json!({})), &mut shelf(), None).unwrap();
        assert!(out["result"].is_object());
    }
}
