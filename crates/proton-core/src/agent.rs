//! The agent, as the editor sees it.
//!
//! `proton-agent` owns the connection and knows nothing about buffers. This
//! owns the conversation, the visible transcript, and the tools — which are
//! the editor's own capabilities handed to the model, so `grep` here is the
//! same index Telescope searches rather than a second implementation of it.
//!
//! One rule shapes the whole file: **every tool result goes back in a single
//! user message.** The model issues parallel calls in one assistant turn, and
//! splitting the answers across several user messages trains it out of doing
//! that. So results are collected until every call in the turn has one, and
//! only then does the conversation move.

use proton_agent::wire::{Block, Message, Role, StopReason, Tool, Usage};
use proton_agent::{Delta, Profile, Reply, Request, Turn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/* ── what the front end draws ─────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Hash)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    You,
    Proton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Hash)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CallState {
    /// Announced, arguments still streaming.
    Starting,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Hash)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments, rendered for a human — `"sats.sort"`, `src/lib.rs`.
    pub detail: String,
    /// What the tool answered, in a line. Not the full result: a grep can
    /// return a thousand lines and the transcript is not where you read them.
    pub result: String,
    pub state: CallState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Hash)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub speaker: Speaker,
    pub text: String,
    /// Summarised reasoning. Rendered in its own panel, collapsed by default —
    /// it is context for a reader who wants it, not part of the answer.
    pub thinking: String,
    pub calls: Vec<ToolCall>,
    /// The turn is still arriving.
    pub streaming: bool,
}

impl Entry {
    fn new(speaker: Speaker) -> Entry {
        Entry {
            speaker,
            text: String::new(),
            thinking: String::new(),
            calls: Vec::new(),
            streaming: false,
        }
    }
}

/// How the agent is doing, for the composer and the statusline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Idle,
    /// Waiting on the model.
    Thinking,
    /// Running the tools it asked for.
    Working,
}

/* ── a tool the agent may call ────────────────────────────────────── */

/// A call the workspace has been asked to make.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub input: Value,
}

pub struct Agent {
    /// The real conversation, in the API's shape. Distinct from the
    /// transcript: this is what gets sent, that is what gets drawn, and they
    /// are not the same thing — a tool result belongs in one and not the other.
    conversation: Vec<Message>,
    transcript: Vec<Entry>,
    turn: Option<Turn>,
    /// Calls from the current turn, and their answers as they come in.
    outstanding: Vec<Pending>,
    answers: Vec<Block>,
    usage: Usage,
    activity: Activity,
    /// The stable half of the prompt. Built once, because anything that
    /// changes between turns invalidates the cached prefix.
    system: Vec<String>,
    /// Which service to ask, and which model. From the settings file, and
    /// re-read when it changes rather than at startup only — switching
    /// provider should take effect on the next question, not the next launch.
    profile: Profile,
    /// The project, for a provider that works in it directly.
    root: Option<std::path::PathBuf>,
    /// Whether that provider may change files there without asking.
    allow_edits: bool,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("turns", &self.conversation.len())
            .field("activity", &self.activity)
            .field("outstanding", &self.outstanding.len())
            .finish()
    }
}

impl Default for Agent {
    fn default() -> Self {
        Agent {
            conversation: Vec::new(),
            transcript: Vec::new(),
            turn: None,
            outstanding: Vec::new(),
            answers: Vec::new(),
            usage: Usage::default(),
            activity: Activity::Idle,
            system: Vec::new(),
            profile: Profile::agent(),
            root: None,
            allow_edits: false,
        }
    }
}

impl Agent {
    pub fn new(project: &str) -> Agent {
        Agent {
            system: vec![
                PROMPT.to_string(),
                // Second block, and last, so the cache breakpoint covers it.
                format!("# Project\n\nThe workspace root is `{project}`."),
            ],
            root: Some(project.into()),
            ..Agent::default()
        }
    }

    pub fn transcript(&self) -> &[Entry] {
        &self.transcript
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    pub fn is_busy(&self) -> bool {
        self.activity != Activity::Idle
    }

    /// The tools the agent may call.
    ///
    /// Sorted by name at the request layer, so the order here is for reading.
    /// Descriptions are written for the model, not for us: they are the only
    /// thing it has to decide between `grep` and `find_files`.
    pub fn tools() -> Vec<Tool> {
        vec![
            Tool {
                name: "read_file".into(),
                description: "Read a file from the workspace. Prefer a line range for large \
                              files; the whole file is returned when none is given."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path relative to the workspace root." },
                        "from": { "type": "integer", "description": "First line, 1-based." },
                        "to": { "type": "integer", "description": "Last line, inclusive." }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "grep".into(),
                description: "Search the workspace for a literal string. Returns matching \
                              lines with their paths. Use this to find where something is \
                              used; use find_files to find a file by name."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "description": "Defaults to 40." }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "find_files".into(),
                description: "Find files by fuzzy name match, the way the file picker does. \
                              An empty query lists the tree."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "description": "Defaults to 40." }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "latex_log".into(),
                description: "What the LaTeX project's last build said: the main file, the engine, and \
                              each error with the source lines round it, what TeX lacked, and the first \
                              warnings. Read-only. Use it before fixing a document that does not typeset; \
                              the build runs again when a fix is saved."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool {
                name: "open_buffer".into(),
                description: "Open a file in the editor so the user can see it. Use when \
                              pointing at something worth looking at, not to read it."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "edit_file".into(),
                description: "Propose a change to a file. The edit is STAGED for the user to \
                              review, hunk by hunk — it is not applied and nothing is written \
                              to disk, so say what you changed rather than claiming it is \
                              done. `old_text` must appear exactly once in the file: include \
                              enough surrounding lines to make it unique, and read the file \
                              first so it matches byte for byte. To create a new file, give \
                              an empty `old_text`. Call this once per edit; several calls in \
                              one turn are fine and stack up into one review."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path relative to the workspace root." },
                        "old_text": {
                            "type": "string",
                            "description": "The exact text to replace, including indentation. \
                                            Empty to create a new file."
                        },
                        "new_text": {
                            "type": "string",
                            "description": "What to put there. Empty to delete the old text."
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
            // Notebooks. `read_file` and `edit_file` do technically work on
            // an `.ipynb` — it is a JSON file — but only in the sense that a
            // model can be asked to hand-edit escaped JSON with `source` as a
            // list of lines and get the commas right. These address cells,
            // which is the unit the file is actually made of.
            Tool {
                name: "read_notebook".into(),
                description: "Read a Jupyter notebook as numbered cells: kind, source, and what each one\
                             last printed. Use this rather than read_file for an .ipynb - read_file returns\
                             the JSON, which is not what you want to reason about."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the .ipynb, relative to the workspace root." },
                        "from": { "type": "integer", "description": "First cell, 0-based. Defaults to the first." },
                        "to": { "type": "integer", "description": "Last cell, inclusive. Defaults to the last." },
                        "outputs": { "type": "boolean", "description": "Include each cell's output. Defaults to true." }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "edit_cell".into(),
                description: "Replace the source of one cell in a notebook, or insert or delete a cell. The\
                             edit is STAGED for the user to review - it is not applied and nothing is\
                             written to disk, so say what you changed rather than claiming it is done. Cell\
                             numbers are 0-based and are the ones read_notebook reported. Several calls in\
                             one turn are fine and stack up into one review."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the .ipynb." },
                        "cell": { "type": "integer", "description": "Which cell, 0-based." },
                        "source": { "type": "string", "description": "The new source. Omit when deleting." },
                        "action": {
                            "type": "string",
                            "enum": ["replace", "insert", "delete"],
                            "description": "replace the cell's source (the default), insert a new cell before it, or delete it."
                        },
                        "kind": {
                            "type": "string",
                            "enum": ["code", "markdown"],
                            "description": "For insert. Defaults to code."
                        }
                    },
                    "required": ["path", "cell"]
                }),
            },
            Tool {
                name: "run_cell".into(),
                description: "Run one cell of a notebook in its kernel and wait for what it prints. The\
                             kernel keeps its state between calls, so a cell that defines something is\
                             visible to the next one, exactly as it is for a person running the notebook\
                             by hand. This RUNS code rather than staging it: there is no review step, so\
                             read the cell before running it and do not run one whose effects you would\
                             not want. Cell numbers are 0-based and are the ones read_notebook reported."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the .ipynb." },
                        "cell": { "type": "integer", "description": "Which cell, 0-based." }
                    },
                    "required": ["path", "cell"]
                }),
            },
            // The user's own writing, which is not in the project and is not
            // reachable by any of the tools above. `grep` searches the
            // repository; this searches the notes they have been keeping for
            // years, and answers with somewhere to look rather than with a
            // claim.
            Tool {
                name: "search_library".into(),
                description: "Search the user's own notes and documents — their vault, their \
                              course notes, their papers and slide decks — for passages about \
                              something. This is NOT the project: use grep and read_file for \
                              code, and this for what the user has written down or kept. Ask in \
                              the words the notes would use, not the words of the question. \
                              Each result begins with a citation in double brackets: quote it \
                              back exactly as given, [[like this]], whenever you use what a \
                              passage says. The user clicks it to open the document where the \
                              passage is, so an answer without citations is one they cannot \
                              check. A citation ending in L12-34 is lines of a note; one \
                              ending in p. 4 is a page of a PDF, whose text was extracted and \
                              may be imperfect — quote it as what the page says, not as \
                              something you read cleanly."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Terms to look for. Matching is lexical, so the exact \
                                            words matter: prefer the terms the notes would use."
                        },
                        "limit": { "type": "integer", "description": "Defaults to 6." },
                        "sources": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Narrow to these folders of the library, named as \
                                            list_sources gives them. Use it when the question \
                                            belongs to one subject: the library spans several, \
                                            matching is by word rather than by topic, and the \
                                            same term means different things in different \
                                            fields. Omit to use whatever the user has selected \
                                            in the sources panel, which is usually right."
                        }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "list_sources".into(),
                description: "The folders the user's library is made of, with how many documents \
                              and passages are in each. Call this when a question clearly \
                              belongs to one subject and you want to narrow search_library to \
                              it — you cannot name a folder you have not seen listed."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            // PDFs: the one on top in the PDF mode. They are the session's,
            // not a project's, so the session answers these (`pdf::agent`).
            Tool {
                name: "pdf_info".into(),
                description: "The PDF on top in the PDF mode: its name, how many pages it has, the page \
                              the person is reading, whether it has changes not saved, its properties, \
                              bookmarks, form fields and comments, and the other PDFs open. Read-only. \
                              Call it first when asked about the PDF or the paper the person is reading."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool {
                name: "pdf_read".into(),
                description: "The text of pages of the PDF on top, each headed by its number. With no \
                              pages given, the page the person is reading; with only `from`, ten pages \
                              from there. At most 30 pages at once. A page that is a picture, with no \
                              text, says so."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "from": { "type": "integer", "description": "First page, from 1." },
                        "to": { "type": "integer", "description": "Last page, inclusive." }
                    }
                }),
            },
            Tool {
                name: "pdf_search".into(),
                description: "Find words in the PDF on top, case and line breaks ignored: each place \
                              they are, with its page and the words round it. Use it to find which \
                              page to read, or the exact words to quote to pdf_annotate."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "description": "How many places to show. Defaults to 20." }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "pdf_annotate".into(),
                description: "Comment on the PDF on top: highlight, underline or strike out a passage, \
                              found by quoting its words as pdf_read or pdf_search gives them, or put a \
                              note on a page, beside a quoted passage or in its top corner. The comment \
                              is signed as the agent's and is held until the person saves: it shows on \
                              their screen at once and they can undo it, so say what you marked rather \
                              than that the file has changed."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "page": { "type": "integer", "description": "The page, from 1." },
                        "kind": { "type": "string", "enum": ["highlight", "underline", "strike", "note"], "description": "Defaults to highlight." },
                        "quote": { "type": "string", "description": "The passage's words, exactly. Required to mark text; for a note, what it goes beside." },
                        "occurrence": { "type": "integer", "description": "Which time the quote is on the page, from 1. Defaults to 1." },
                        "text": { "type": "string", "description": "What the comment says. Required for a note." },
                        "color": { "type": "string", "description": "#rrggbb. Highlights default to yellow." }
                    },
                    "required": ["page"]
                }),
            },
            Tool {
                name: "pdf_pages".into(),
                description: "Rearrange the PDF on top: turn, delete or move pages, held until the person \
                              saves and undone as their own changes are; or take pages out into a new \
                              PDF, which is written at once, opened, and on top after. Pages are \
                              numbered from 1, as the person sees them."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["rotate", "delete", "move", "extract"] },
                        "pages": { "type": "array", "items": { "type": "integer" }, "description": "The pages, from 1." },
                        "by": { "type": "integer", "description": "Rotate: degrees clockwise, 90, -90 or 180. Defaults to 90." },
                        "before": { "type": "integer", "description": "Move: the page they go before; one past the last for the end." },
                        "path": { "type": "string", "description": "Extract: the new PDF, as a path; ~ is home, and a relative one is in the project." }
                    },
                    "required": ["action", "pages"]
                }),
            },
            // The game open in Game mode: the workspace's own (`game::agent`).
            Tool {
                name: "game_info".into(),
                description: "The game open in Game mode. With no document: its name, view and controller \
                              (jump height, run speed, coyote and buffer ticks), abilities, where it starts, \
                              its rooms with their sizes, spawns and entities and which is on screen, its \
                              prefabs, what is not saved, its problems, and the runs the person played. With \
                              a document (game, world, room:<id> or prefab:<id>): that file's JSON as it is \
                              now, unsaved changes included — the paths game_edit takes are read from it. \
                              Read-only; call it first when asked about the game."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "document": { "type": "string", "description": "game, world, room:<id> or prefab:<id>. Omit for the summary." }
                    }
                }),
            },
            Tool {
                name: "game_edit".into(),
                description: "Change one of the open game's documents by ops, as the person's own edits are \
                              made: {\"op\": \"set\", \"path\": [...], \"value\": ...}, {\"op\": \"insert\", \"path\": \
                              [list], \"value\": ..., \"at\": n}, {\"op\": \"remove\", \"path\": [...]}, {\"op\": \
                              \"move\", \"path\": [...], \"to\": n}. A path names keys, and list members by \
                              \"#id\" (e.g. [\"collision\", \"#ledge\", \"points\", \"1\"]) or by index. The ops \
                              apply together or not at all: a batch that leaves a document invalid is refused \
                              whole. The change is held until the person saves: it shows on their screen at \
                              once and is one step of their undo, so say what you changed rather than that a \
                              file was written."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "document": { "type": "string", "description": "game, world, room:<id> or prefab:<id>." },
                        "ops": { "type": "array", "items": { "type": "object" }, "description": "The ops, applied in order, together." }
                    },
                    "required": ["document", "ops"]
                }),
            },
            Tool {
                name: "game_check".into(),
                description: "What is wrong with the open game: doors to rooms or spawns that are not there, \
                              gates and pickups naming abilities the game does not have, entities that are \
                              neither built in nor a prefab, rooms with no collision or no spawn, and whether \
                              it can be played at all. Read-only."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool {
                name: "game_play".into(),
                description: "Play the open game headless, by the same simulation the person plays, and say \
                              what happened: where the player ended, where it was hurt, which rooms it went \
                              into and which abilities it gained, with its place every few ticks. Inputs are \
                              held for a number of ticks (the game runs at its own tick, usually 60 a \
                              second): [{\"hold\": [\"right\"], \"ticks\": 60}, {\"hold\": [\"right\", \"jump\"], \
                              \"ticks\": 20}]; the actions are left, right, up, down, jump, attack and dash. Or \
                              replay a run the person played, by its number. When the game has changes not \
                              saved, the same inputs are also played on the game as last saved, so you can \
                              tell what an edit did to a route. The run is drawn over the room on the \
                              person's screen."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "room": { "type": "string", "description": "The room to start in. Defaults to where the game starts." },
                        "at": { "type": "array", "items": { "type": "integer" }, "description": "[x, y] in pixels of the room, the player's feet. Defaults to the room's first spawn." },
                        "abilities": { "type": "array", "items": { "type": "string" }, "description": "Abilities the run starts with, by id." },
                        "inputs": { "type": "array", "items": { "type": "object" }, "description": "What is held, and for how many ticks." },
                        "recording": { "type": "integer", "description": "A run the person played, by its number, in place of inputs." }
                    }
                }),
            },
            Tool {
                name: "game_import".into(),
                description: "Bring a layered .psd, or a .png, from the open game's folder into a room. Each \
                              group becomes a plane at the depth its name gives in square brackets — \
                              \"Far hills [800]\" behind, \"Leaves [-150]\" in front, \"[still]\" for a sky \
                              that does not move, \"[play]\" for art that joins the plane the game is played \
                              on — and each layer an image, trimmed, cut into pieces when large. The images \
                              are written into the game's art folder at once; the room's change is held \
                              until the person saves and is one step of their undo. Importing the same file \
                              again replaces what it made before. Says what was left out (effects, \
                              adjustment layers, hidden layers) and why."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "The file, in the game's folder, as game_info's summary or the Assets tab names it." },
                        "room": { "type": "string", "description": "The room. Defaults to the one it went into before, else the one on screen." },
                        "scale": { "type": "number", "description": "Room pixels to a source pixel. Defaults to 1, or what it was before." },
                        "normals": { "type": "boolean", "description": "Also make normal maps from brightness, for lights to shade with." }
                    },
                    "required": ["source"]
                }),
            },
            // The canvas open in Canvas mode: the workspace's own (`raster::agent`).
            Tool {
                name: "canvas_info".into(),
                description: "The canvas open in Canvas mode: its name and size, whether it is pixel art or a \
                              painting, its palette by swatch number, its layers top first with their ids, where \
                              each has pixels, its opacity, blend and locks, and what is selected — or, with none \
                              open, the project's canvases. With `pixels`, a part of it as a grid of letters, one \
                              a colour with a legend, '.' for clear: how to see pixel art before changing it. x \
                              runs right and y down from 0 at the top left. Read-only; call it first."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pixels": {
                            "type": "object",
                            "description": "Show pixels: everything laid together, or one layer.",
                            "properties": {
                                "layer": { "type": "integer", "description": "A layer's id. Omit for everything laid together." },
                                "rect": { "type": "array", "items": { "type": "integer" }, "description": "[x, y, width, height]; omit for the whole canvas. At most 128 × 128 for pixel art, 64 × 64 for a painting." }
                            }
                        }
                    }
                }),
            },
            Tool {
                name: "canvas_new".into(),
                description: "Make a canvas in the project and open it on the person's screen: pixel art (a small \
                              grid, a palette of sixteen to start; 32 × 32 if no size is given) or a painting \
                              (1024 × 1024). It is written as a .canvas folder at once."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "preset": { "type": "string", "enum": ["pixel", "paint"] },
                        "width": { "type": "integer" },
                        "height": { "type": "integer", "description": "Defaults to the width." }
                    },
                    "required": ["name"]
                }),
            },
            Tool {
                name: "canvas_draw".into(),
                description: "Draw on the open canvas, as the person draws: a list of marks, in pixels from the top \
                              left — {\"pixels\": [[x, y], ...], \"color\": c} sets each pixel; {\"line\": \
                              [[x, y], ...], \"color\": c, \"size\": n} a pencil through the points, steps without \
                              corners; {\"rect\": [x, y, w, h]} and {\"ellipse\": [x, y, w, h]} with \"fill\": \
                              true for solid; {\"fill\": [x, y], \"color\": c} a bucket; {\"erase\": [x, y, w, \
                              h]}; or one of the engine's ops as the window sends them. A colour c is \"#rrggbb\", \
                              \"#rrggbbaa\", [r, g, b, a] or a palette swatch's number. On the top layer unless a \
                              layer (id or name) or a new_layer is given. Every mark keeps to what is selected. \
                              The marks are one step of the person's undo, applied together or not at all, on \
                              their screen at once and held until the person saves."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "marks": { "type": "array", "items": { "type": "object" } },
                        "layer": { "description": "A layer's id or name. Defaults to the top layer." },
                        "new_layer": { "type": "string", "description": "Make a layer of this name on top, and draw on it." }
                    },
                    "required": ["marks"]
                }),
            },
            Tool {
                name: "canvas_generate".into(),
                description: "Make pixels from a seed on the open canvas, or change a layer by a filter, or add a \
                              colour ramp to the palette — procedural, the same seed making the same pixels. \
                              generator: {\"kind\": \"sprite\", \"template\": \"ship\"|\"creature\"|\"item\", \
                              \"scale\": n, \"color\": c} (mirrored, outlined, shaded; centred if no \"at\"), \
                              {\"kind\": \"noise\", \"noise\": \"value\"|\"gradient\", \"cell\": px, \"octaves\": \
                              n, \"colors\": [c, ...], \"mix\": \"dither\"|\"nearest\"|\"smooth\"} or {\"kind\": \
                              \"cave\", \"density\": 0-100, \"steps\": n, \"cell\": px, \"wrap\": bool, \"color\": \
                              c}; a seed is chosen if none is given. filter: {\"kind\": \"outline\", \"color\": c}, \
                              {\"kind\": \"shadow\", \"dx\": n, \"dy\": n, \"color\": c, \"blur\": n}, {\"kind\": \
                              \"pixelate\", \"size\": n} or {\"kind\": \"quantize\", \"colors\": n, \"palette\": \
                              bool, \"dither\": 0|2|4|8, \"keep\": bool}. ramp: {\"base\": c, \"steps\": n}. A \
                              generator or filter works on the top layer unless layer or new_layer says, and \
                              keeps to what is selected. One step of the person's undo, on their screen at once \
                              and held until the person saves."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "generator": { "type": "object" },
                        "filter": { "type": "object" },
                        "ramp": { "type": "object" },
                        "layer": { "description": "A layer's id or name. Defaults to the top layer." },
                        "new_layer": { "type": "string", "description": "Make a layer of this name on top, and work on it." }
                    }
                }),
            },
            Tool {
                name: "canvas_export".into(),
                description: "Write the open canvas out: a PNG beside its folder (flattened, each pixel scale × \
                              scale), or a layered PSD; or, with game, into that game's art folder — layered, \
                              each layer a plane at the depth its name gives in square brackets, or flat — and \
                              into its room when that game is open in Game mode. Files are written at once."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "format": { "type": "string", "enum": ["png", "psd"] },
                        "scale": { "type": "integer", "description": "PNG: each pixel this many pixels square, 1 to 16." },
                        "game": { "type": "string", "description": "The game's folder in the project, as canvas_info or Game mode names it; \"\" for a project that is itself a game." },
                        "layers": { "type": "boolean", "description": "With game: as layers (the default), or flat." }
                    }
                }),
            },
            Tool {
                name: "diagnostics".into(),
                description: "Errors and warnings the language server has reported for a \
                              file, or for the active buffer when no path is given."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            },
        ]
    }

    /* ── driving a turn ───────────────────────────────────────────── */

    /// Put a question to the agent. Returns an error only if the turn could
    /// not be started at all — a missing credential, usually.
    pub fn ask(&mut self, text: &str) -> Result<(), String> {
        if self.is_busy() {
            return Err("the agent is still working on the last one".into());
        }

        self.conversation
            .push(Message::user(vec![Block::text(text)]));
        self.transcript.push(Entry {
            text: text.to_string(),
            ..Entry::new(Speaker::You)
        });
        // One entry for the whole reply, however many round trips it takes.
        // "Answer, call a tool, answer again" is one thing the agent did, and
        // splitting it in the transcript would show the reader plumbing.
        self.transcript.push(Entry {
            streaming: true,
            ..Entry::new(Speaker::Proton)
        });
        self.begin()
    }

    /// Send the conversation as it stands and start streaming the reply.
    fn begin(&mut self) -> Result<(), String> {
        let request = Request {
            profile: self.profile.clone(),
            system: self.system.clone(),
            tools: Agent::tools(),
            messages: self.conversation.clone(),
            workspace: self.root.clone(),
            allow_edits: self.allow_edits,
        };
        self.turn = Some(proton_agent::send(request)?);
        self.activity = Activity::Thinking;
        self.say(|e| e.streaming = true);
        Ok(())
    }

    /// Stop the turn in flight. The partial answer stays on screen — it was
    /// true when it arrived — but nothing is appended to the conversation,
    /// because a half turn is one the API will reject next time.
    pub fn cancel(&mut self) {
        if let Some(turn) = self.turn.take() {
            turn.cancel();
        }
        self.outstanding.clear();
        self.answers.clear();
        self.activity = Activity::Idle;
        if let Some(entry) = self.transcript.last_mut() {
            entry.streaming = false;
        }
    }

    /// Drain the turn. Returns the tool calls the workspace should run.
    pub fn poll(&mut self) -> Vec<Pending> {
        let Some(turn) = &mut self.turn else {
            return Vec::new();
        };
        let replies = turn.poll();
        let finished = turn.is_done();
        let mut to_run = Vec::new();

        for reply in replies {
            match reply {
                Reply::History(messages) => self.conversation.extend(messages),
                Reply::Started { .. } => {}

                Reply::Delta(Delta::Text(text)) => self.say(|e| e.text.push_str(&text)),
                Reply::Delta(Delta::Thinking(text)) => self.say(|e| e.thinking.push_str(&text)),

                Reply::Delta(Delta::ToolStarted { id, name }) => {
                    let call = ToolCall {
                        id,
                        name,
                        detail: String::new(),
                        result: String::new(),
                        state: CallState::Starting,
                    };
                    self.say(move |e| e.calls.push(call));
                }

                Reply::Delta(Delta::ToolReady { id, name, input }) => {
                    if self.turn.as_ref().is_some_and(Turn::hosts_tools) {
                        self.activity = Activity::Working;
                    }
                    let detail = summarise(&input);
                    let for_entry = (id.clone(), detail);
                    self.say(move |e| {
                        if let Some(call) = e.calls.iter_mut().find(|c| c.id == for_entry.0) {
                            call.detail = for_entry.1.clone();
                            call.state = CallState::Running;
                        }
                    });
                    to_run.push(Pending { id, name, input });
                }

                // A tool the provider ran itself. One row, made on the first
                // report and brought up to date by the next — and never handed
                // to the workspace, because it has already happened.
                Reply::Delta(Delta::Hosted {
                    id,
                    name,
                    input,
                    outcome,
                }) => {
                    let detail = summarise(&input);
                    let (state, result) = match outcome {
                        None => (CallState::Running, String::new()),
                        Some(Ok(())) => (CallState::Done, String::new()),
                        Some(Err(why)) => (CallState::Failed, short(&why)),
                    };
                    self.say(move |e| match e.calls.iter_mut().find(|c| c.id == id) {
                        Some(call) => {
                            call.detail = detail.clone();
                            call.state = state;
                            call.result = result.clone();
                        }
                        None => e.calls.push(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            detail: detail.clone(),
                            result: result.clone(),
                            state,
                        }),
                    });
                }

                Reply::Delta(Delta::Done { .. }) => {}

                Reply::Finished {
                    message,
                    stop,
                    usage,
                } => {
                    self.usage.merge(usage);
                    self.conversation.push(message);
                    self.say(|e| e.streaming = false);

                    if stop == Some(StopReason::ToolUse) {
                        // The turn is not over: it is waiting on us.
                        self.activity = Activity::Working;
                    } else {
                        self.activity = Activity::Idle;
                        if stop == Some(StopReason::Refusal) {
                            self.say(|e| {
                                e.text.push_str("\n\n_The model declined to answer this._")
                            });
                        }
                    }
                }

                Reply::Failed { text } => {
                    to_run.clear();
                    self.outstanding.clear();
                    self.answers.clear();
                    self.activity = Activity::Idle;
                    self.say(move |e| {
                        e.streaming = false;
                        if !e.text.is_empty() {
                            e.text.push_str("\n\n");
                        }
                        e.text.push_str(&format!("_{text}_"));
                    });
                }

                Reply::Cancelled => {
                    to_run.clear();
                    self.outstanding.clear();
                    self.answers.clear();
                    self.activity = Activity::Idle;
                    self.say(|e| e.streaming = false);
                }
            }
        }

        if finished {
            self.turn = None;
        }
        self.outstanding.extend(to_run.iter().cloned());
        to_run
    }

    /// Record what a tool answered.
    ///
    /// Nothing is sent until every outstanding call has an answer — see the
    /// note at the top of the file.
    pub fn answer(&mut self, id: &str, result: Result<String, String>) {
        let hosted = self
            .turn
            .as_ref()
            .filter(|turn| turn.hosts_tools())
            .map(|turn| turn.answer_tool(id, result.clone()));
        let (body, failed) = match result {
            Ok(body) => (body, false),
            Err(message) => (message, true),
        };

        let line = first_line(&body);
        let id_owned = id.to_string();
        self.say(move |e| {
            if let Some(call) = e.calls.iter_mut().find(|c| c.id == id_owned) {
                call.result = line.clone();
                call.state = if failed {
                    CallState::Failed
                } else {
                    CallState::Done
                };
            }
        });

        // A failed tool still answers. Dropping the block leaves a `tool_use`
        // with no result, which is a malformed conversation the API rejects —
        // the failure has to be reported *as* the result.
        if hosted.is_none() {
            self.answers.push(if failed {
                Block::error(id, body)
            } else {
                Block::result(id, body)
            });
        }
        self.outstanding.retain(|p| p.id != id);
        if let Some(sent) = hosted {
            if let Err(why) = sent {
                self.cancel();
                self.say(|e| e.text.push_str(&format!("\n\n{why}")));
            } else if self.outstanding.is_empty() {
                self.activity = Activity::Thinking;
            }
        }
    }

    /// Every call answered? Then continue the turn.
    ///
    /// The results go back as one user message, in the order the model asked
    /// for them.
    pub fn resume(&mut self) -> Result<bool, String> {
        if self.activity != Activity::Working || !self.outstanding.is_empty() {
            return Ok(false);
        }
        if self.answers.is_empty() {
            return Ok(false);
        }
        let answers = std::mem::take(&mut self.answers);
        self.conversation.push(Message::user(answers));

        // What the model said before it reached for a tool, and what it says
        // afterwards, are two paragraphs of one answer — not one run-on
        // sentence with a tool call hidden in the middle of it.
        self.say(|e| {
            if !e.text.is_empty() {
                e.text.push_str("\n\n");
            }
        });
        self.begin()?;
        Ok(true)
    }

    /// Edit the assistant entry currently being written.
    fn say(&mut self, edit: impl FnOnce(&mut Entry)) {
        if let Some(entry) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|e| e.speaker == Speaker::Proton)
        {
            edit(entry);
        }
    }

    /// For tests and for the statusline: how many turns are on the wire.
    /// Load a saved conversation, rebuilding what is drawn from what was sent.
    ///
    /// The transcript is not stored — the conversation is the document, and a
    /// second record of the same thing would be free to disagree with it. So
    /// it is rebuilt here, which is a function that has to be right rather
    /// than a file that has to be kept in step.
    ///
    /// The rebuild follows the live path exactly: one user entry per question,
    /// and *one* agent entry for the whole reply however many tool round trips
    /// it took, because that is one thing the agent did.
    pub fn restore(&mut self, messages: Vec<Message>) {
        self.transcript.clear();
        self.outstanding.clear();
        self.answers.clear();
        self.turn = None;
        self.activity = Activity::Idle;

        // What each call answered, so a restored transcript shows its results
        // rather than a row of calls that look like they never returned.
        let results: std::collections::HashMap<&str, (&str, bool)> = messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| match block {
                Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => Some((tool_use_id.as_str(), (content.as_str(), *is_error))),
                _ => None,
            })
            .collect();

        for message in &messages {
            match message.role {
                Role::User => {
                    // A tool result is a user-role message on the wire and is
                    // not something the user said. Showing it as one would put
                    // words in their mouth.
                    let text: String = message
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            Block::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(
                            "
",
                        );
                    if !text.is_empty() {
                        self.transcript.push(Entry {
                            text,
                            ..Entry::new(Speaker::You)
                        });
                    }
                }
                Role::Assistant => {
                    // Continue the open agent entry rather than starting one,
                    // so a reply that called two tools and spoke three times
                    // is one entry.
                    if !matches!(
                        self.transcript.last(),
                        Some(Entry {
                            speaker: Speaker::Proton,
                            ..
                        })
                    ) {
                        self.transcript.push(Entry::new(Speaker::Proton));
                    }
                    let Some(entry) = self.transcript.last_mut() else {
                        continue;
                    };
                    for block in &message.content {
                        match block {
                            Block::Text { text } => entry.text.push_str(text),
                            Block::Thinking { thinking, .. } => entry.thinking.push_str(thinking),
                            Block::ToolUse {
                                id, name, input, ..
                            } => {
                                let (result, failed) =
                                    results.get(id.as_str()).copied().unwrap_or(("", false));
                                entry.calls.push(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    detail: summarise(input),
                                    result: short(result),
                                    // A call with no result in the file is one
                                    // whose turn was cut off. Saying so is
                                    // better than showing it as finished.
                                    state: match (result.is_empty(), failed) {
                                        (true, _) => CallState::Running,
                                        (false, true) => CallState::Failed,
                                        (false, false) => CallState::Done,
                                    },
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        self.conversation = messages;
    }

    /// The conversation as it stands, for saving.
    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    /// Start again with nothing.
    pub fn clear(&mut self) {
        self.restore(Vec::new());
    }

    /// Point the agent at a provider and model.
    ///
    /// Takes effect on the next turn. The one in flight keeps the profile it
    /// started with, because a conversation half-answered by one model and
    /// half by another is not a conversation either of them could continue.
    pub fn use_provider(&mut self, provider: proton_agent::Provider, model: &str) {
        self.profile = Profile::for_provider(provider, model, false);
    }

    /// Whether a provider that works in the project itself — Antigravity — may
    /// change files there without asking. Off unless the settings say so.
    pub fn allow_edits(&mut self, allow: bool) {
        self.allow_edits = allow;
    }

    /// Where a credential for the current provider would come from.
    ///
    /// Reported rather than tested: this reads the environment and the CLI's
    /// profile, which is exactly what `send` will do, so it says what will
    /// happen without spending a request to find out. Never the credential
    /// itself — this string is drawn on screen and would end up in a
    /// screenshot.
    pub fn credential(&self) -> String {
        if self.profile.provider == proton_agent::Provider::Codex {
            return match proton_agent::codex::executable() {
                Ok(_) => {
                    "Uses your Codex sign-in; connection is verified when you send a message".into()
                }
                Err(why) => why,
            };
        }
        if self.profile.provider == proton_agent::Provider::Antigravity {
            return match proton_agent::antigravity::executable() {
                Ok(_) => "Uses your Antigravity sign-in; connection is verified when you send a \
                          message"
                    .into(),
                Err(why) => why,
            };
        }
        match proton_agent::auth::resolve_for(self.profile.provider) {
            Ok(found) => match found.source {
                proton_agent::auth::Source::ApiKey => {
                    format!("{} is set", self.profile.provider.key_vars()[0])
                }
                proton_agent::auth::Source::AuthToken => "ANTHROPIC_AUTH_TOKEN is set".into(),
                proton_agent::auth::Source::OauthProfile => "signed in with the Claude CLI".into(),
                proton_agent::auth::Source::CodexLogin => "signed in with Codex".into(),
                proton_agent::auth::Source::AntigravityLogin => {
                    "signed in with Antigravity".into()
                }
            },
            // The error already names what to set, which is the only thing
            // worth saying about a credential that is not there.
            Err(why) => why,
        }
    }

    /// Which service is answering, for the pane to say so.
    pub fn provider(&self) -> proton_agent::Provider {
        self.profile.provider
    }

    pub fn model(&self) -> &str {
        &self.profile.model
    }

    pub fn turns(&self) -> usize {
        self.conversation
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .count()
    }
}

/// A tool's arguments, in a line.
fn summarise(input: &Value) -> String {
    let Some(object) = input.as_object() else {
        return String::new();
    };
    object
        .iter()
        .map(|(key, value)| match value {
            Value::String(s) => format!("{key}={}", short(s)),
            other => format!("{key}={}", short(&other.to_string())),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One line, and not a long one.
///
/// `edit_file` carries whole blocks of source in its arguments, and a
/// transcript row that unrolls one is a row that pushes the answer off the
/// screen. The diff below it is where you read the change.
fn short(value: &str) -> String {
    const MOST: usize = 48;
    let line = value.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(MOST).collect();
    if line.chars().count() > MOST || value.lines().nth(1).is_some() {
        out.push('…');
    }
    out
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let extra = text.lines().count().saturating_sub(1);
    match extra {
        0 => line.to_string(),
        n => format!("{line} (+{n} more)"),
    }
}

const PROMPT: &str = "\
You are Proton's agent, working inside the editor the user is looking at.

Read before you answer. `grep` and `read_file` are cheap and guessing is not — \
a wrong claim about code the user can see costs more than the tool call would \
have. Call tools in parallel when the calls do not depend on each other.

Be concrete. Name files and line numbers. When you propose a change, say what \
it changes and why the current code does the wrong thing, rather than \
restating the fix as a description of itself.

Use `edit_file` to propose changes rather than printing code for the user to \
copy. What it produces is staged for review, hunk by hunk, and nothing is \
written until they accept it — so describe what you changed and leave the \
deciding to them. Never say a change is done.

When the question is about what the user knows rather than what the code does \
— their notes, their reading, a course they took — search the library. Cite \
what you find, in the double brackets the results give you, and put the \
citation next to the claim it supports rather than in a list at the end. If \
the library has nothing on it, say so and answer from what you know; a \
citation to a passage that does not say the thing is worse than no citation.

Keep answers short. The user is reading this in a side panel next to the code, \
not in a document.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_summary_reads_as_arguments_not_json() {
        // This goes in the transcript beside the tool's name, where
        // `{"query":"sats.sort"}` is noise.
        assert_eq!(
            summarise(&json!({ "query": "sats.sort" })),
            "query=sats.sort"
        );
        assert_eq!(summarise(&json!({ "limit": 40 })), "limit=40");
        assert_eq!(summarise(&json!(null)), "");
    }

    #[test]
    fn a_result_is_summarised_to_one_line_with_a_count() {
        // A grep can return a thousand lines; the transcript is not where
        // they are read.
        assert_eq!(first_line("only one"), "only one");
        assert_eq!(first_line("first\nsecond\nthird"), "first (+2 more)");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn the_tool_list_is_stable_and_documented() {
        // Both properties matter: the order feeds a cached prefix, and the
        // descriptions are the only thing the model has to choose between
        // `grep` and `find_files`.
        let tools = Agent::tools();
        assert_eq!(tools.len(), 27);
        // `search_library` has to say how to cite, because the bracketed
        // citation is the only thing that makes a result clickable — and an
        // answer about somebody's notes that they cannot check against the
        // note is the failure this whole tool exists to avoid.
        let library = tools.iter().find(|t| t.name == "search_library").unwrap();
        assert!(
            library.description.contains("[[like this]]"),
            "it must show the citation format: {}",
            library.description
        );
        // `edit_file` has to say that it stages rather than applies. A model
        // told only "edit a file" reports the job as done, and the user reads
        // that as a file that has changed.
        // Both of them: a model told only "edit a cell" reports the job as
        // done, and the user reads that as a notebook that has changed.
        for name in ["edit_file", "edit_cell"] {
            let editing = tools.iter().find(|t| t.name == name).unwrap();
            assert!(
                editing.description.contains("STAGED"),
                "{name} must say it does not apply: {}",
                editing.description
            );
        }
        // The PDF tools that change it have to say the change waits for the
        // person: told only "rotate the pages", a model reports a file
        // rewritten, and the person believes it.
        for name in ["pdf_annotate", "pdf_pages", "game_edit", "game_import", "canvas_draw", "canvas_generate"] {
            let changing = tools.iter().find(|t| t.name == name).unwrap();
            assert!(changing.description.contains("held until the person saves"), "{name}: {}", changing.description);
        }
        // Every one the session answers is declared, and none it does not.
        let declared: Vec<&str> = tools.iter().map(|t| t.name.as_str()).filter(|n| n.starts_with("pdf_")).collect();
        assert_eq!(declared, crate::pdf::TOOLS);
        let declared: Vec<&str> = tools.iter().map(|t| t.name.as_str()).filter(|n| n.starts_with("game_")).collect();
        assert_eq!(declared, crate::game::TOOLS);
        let declared: Vec<&str> = tools.iter().map(|t| t.name.as_str()).filter(|n| n.starts_with("canvas_")).collect();
        assert_eq!(declared, crate::raster::TOOLS);
        for tool in &tools {
            // A description written with line continuations and mangled into
            // one long line reads to the model as a run of whitespace in the
            // middle of a sentence. It happened to two of these.
            assert!(
                !tool.description.contains("   "),
                "{} has a run of spaces in its description: {}",
                tool.name,
                tool.description
            );
            assert!(
                tool.description.len() > 40,
                "{} needs a real description",
                tool.name
            );
            assert_eq!(tool.input_schema["type"], "object");
        }
        assert_eq!(
            Agent::tools(),
            tools,
            "the list must not vary between calls"
        );
    }

    #[test]
    fn results_are_held_until_every_call_has_one() {
        // The rule the whole file is shaped around: parallel calls come back
        // in one assistant message and their results have to go back in one
        // user message.
        let mut agent = Agent::new(".");
        agent.activity = Activity::Working;
        agent.outstanding = vec![
            Pending {
                id: "a".into(),
                name: "grep".into(),
                input: json!({}),
            },
            Pending {
                id: "b".into(),
                name: "read_file".into(),
                input: json!({}),
            },
        ];

        agent.answer("a", Ok("one hit".into()));
        assert!(!agent.resume().unwrap(), "one answer of two must not send");

        agent.answer("b", Err("no such file".into()));
        assert_eq!(agent.answers.len(), 2, "a failure still answers");
    }

    #[test]
    fn a_failed_tool_answers_rather_than_going_missing() {
        // A `tool_use` with no `tool_result` is a malformed conversation, and
        // the API rejects the whole thing rather than the one call.
        let mut agent = Agent::new(".");
        agent.answer("t1", Err("permission denied".into()));

        assert_eq!(
            agent.answers,
            vec![Block::error("t1", "permission denied")],
            "the failure has to be reported as the result"
        );
    }

    #[test]
    fn asking_twice_at_once_is_refused_rather_than_interleaved() {
        let mut agent = Agent::new(".");
        agent.activity = Activity::Thinking;
        assert!(agent.ask("second question").is_err());
    }

    #[test]
    fn cancelling_keeps_what_arrived_but_does_not_commit_it() {
        // A partial answer was true when it arrived and is worth keeping on
        // screen. Appending it to the conversation is not: a turn cut in half
        // is one the API refuses next time.
        let mut agent = Agent::new(".");
        agent.transcript.push(Entry {
            text: "half an ans".into(),
            streaming: true,
            ..Entry::new(Speaker::Proton)
        });
        agent.activity = Activity::Thinking;

        agent.cancel();
        assert_eq!(agent.activity, Activity::Idle);
        assert_eq!(agent.transcript.last().unwrap().text, "half an ans");
        assert!(!agent.transcript.last().unwrap().streaming);
        assert_eq!(agent.conversation.len(), 0, "nothing was committed");
    }
}
