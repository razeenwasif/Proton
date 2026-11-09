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
    /// A change it wrote that the pane can take back.
    #[serde(default)]
    pub undo: bool,
    /// A command it proposes, waiting for the person to run it or not.
    #[serde(default)]
    pub asks: bool,
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
    /// What went with a question besides its words — the file open, a
    /// mention's context — by name: sent to the model, drawn as chips.
    #[serde(default)]
    pub attached: Vec<String>,
}

impl Entry {
    fn new(speaker: Speaker) -> Entry {
        Entry {
            speaker,
            text: String::new(),
            thinking: String::new(),
            calls: Vec::new(),
            streaming: false,
            attached: Vec::new(),
        }
    }
}

/// The headings context is appended under, after a question's words: the
/// file open, and each mention's.
const CONTEXT: [&str; 7] = ["The file open in the editor", "@selection", "@dataframe", "@sheet", "@circuit", "@logic", "@cad"];

/// A question as it was typed, and the names of what was appended to it:
/// for the bubble and the task list, which show what you wrote, while the
/// model is sent the whole.
pub fn split_context(text: &str) -> (String, Vec<String>) {
    let starts = |at: usize| CONTEXT.iter().find(|h| text[at..].starts_with(&format!("\n\n# {h}\n")));
    let Some(first) = text.match_indices("\n\n# ").map(|(i, _)| i).find(|&i| starts(i).is_some()) else {
        return (text.to_string(), Vec::new());
    };
    let mut attached = Vec::new();
    for (i, _) in text.match_indices("\n\n# ").filter(|&(i, _)| i >= first) {
        let Some(heading) = starts(i) else { continue };
        let label = match *heading {
            // The file by its name in backticks on the line after.
            "The file open in the editor" => text[i..]
                .lines()
                .nth(3)
                .and_then(|l| l.split('`').nth(1))
                .map(|path| path.to_string())
                .unwrap_or_else(|| "the file open".into()),
            mention => mention.to_string(),
        };
        if !attached.contains(&label) {
            attached.push(label);
        }
    }
    (text[..first].to_string(), attached)
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
                name: "run_command".into(),
                description: "Run a shell command in the project folder — to build, test, install a \
                              dependency, or look at what a tool says. The user is ASKED first: \
                              it runs only if they click Run, and you get its exit code and \
                              output (its end, if long) back, or are told they declined. Give \
                              one command line; keep it to what the task needs, and say in \
                              `why` what it is for. It runs non-interactively, with no input."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The command line, as typed in a terminal." },
                        "why": { "type": "string", "description": "What it is for, in a few words, shown to the user." }
                    },
                    "required": ["command"]
                }),
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
                description: "Change a file in the project, or create one. The change is \
                              normally written to disk at once, and the user can undo it from \
                              the pane; if they review changes first, or the file has unsaved \
                              work of theirs, or it is outside the project, it is STAGED for \
                              them to review instead and nothing is written. The result says \
                              which — report it as it says. `old_text` must appear exactly \
                              once in the file: include enough surrounding lines to make it \
                              unique, and read the file first so it matches byte for byte. To \
                              create a new file, give an empty `old_text`. Call this once per \
                              edit; several calls in one turn are fine."
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
            // Excel's workbook. Edits are held until the person saves, and
            // each call is one step they can undo.
            Tool {
                name: "sheet_info".into(),
                description: "The workbook open in Excel mode: its sheets and the cells each uses, \
                              frozen panes, filters, colour rules, charts and names. With none open, \
                              the project's workbooks."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool {
                name: "sheet_open".into(),
                description: "Open a workbook in Excel mode: a .sheet folder, .xlsx, .csv or .tsv in the project."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "As sheet_info lists it." } },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "sheet_read".into(),
                description: "Read a range of the open workbook as a table of the values its cells come to; \
                              with formulas, the formulas too. Up to 5000 cells a call."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "range": { "type": "string", "description": "A1:D20, or A:C for whole columns." },
                        "sheet": { "type": "string", "description": "The sheet's name; the one on screen if not given." },
                        "formulas": { "type": "boolean" }
                    },
                    "required": ["range"]
                }),
            },
            Tool {
                name: "sheet_write".into(),
                description: "Write cells of the open workbook — values or Excel formulas starting with = — \
                              as one step the person can undo, held until they save. Give `cells`, or \
                              `at` and `rows` for a block. Answers with what the written cells come to."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "cells": { "type": "array", "items": { "type": "object", "properties": {
                            "at": { "type": "string" }, "input": {} }, "required": ["at", "input"] } },
                        "at": { "type": "string", "description": "The block's top left cell." },
                        "rows": { "type": "array", "items": { "type": "array" } },
                        "sheet": { "type": "string" }
                    }
                }),
            },
            Tool {
                name: "sheet_chart".into(),
                description: "Chart a range of the open workbook — its first row the series' names, its \
                              first column the categories — beside it on the sheet."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "range": { "type": "string" },
                        "kind": { "type": "string", "enum": ["line", "bar", "scatter", "pie"] },
                        "title": { "type": "string" },
                        "at": { "type": "string", "description": "The cell its top left sits on." },
                        "sheet": { "type": "string" }
                    },
                    "required": ["range"]
                }),
            },
            // Circuit's project. Edits are held until the person saves, and
            // each call is one step they can undo.
            Tool {
                name: "circuit_info".into(),
                description: "The circuit project open in Circuit mode: its circuits (the top one marked), each one's \
                              inputs, outputs and size, its test tables, state machines and memory files, and \
                              whether it is running. With none open, the projects and Digital .dig files in the folder."
                    .into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool {
                name: "circuit_open".into(),
                description: "Open a .circuit project, import a Digital .dig file (with the circuits it uses) as one, \
                              or with `new` make a project of that name."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "As circuit_info lists it." },
                        "new": { "type": "string", "description": "A name for a new project." }
                    }
                }),
            },
            Tool {
                name: "circuit_read".into(),
                description: "A circuit as its .circ text — `part <id> <kind> at x,y [rot n] [mirror] key=value…` and \
                              `wire x1,y1 x2,y2`, in grid steps — then where each part's pins are, inputs and \
                              outputs, as the simulator joins them. Wires join only at their ends: a wire to a pin \
                              ends on the pin. The circuit on screen unless named."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "circuit": { "type": "string" } }
                }),
            },
            Tool {
                name: "circuit_edit".into(),
                description: "Change a circuit, as one step the person can undo, held until they save. `ops` in order: \
                              {op:'place', kind, x, y, rot?, mirror?, settings?:[[key,value]]} (kinds: in out clock button \
                              const ground vdd led probe tunnel split text not and or nand nor xor xnor mux demux decoder \
                              priority add subtract multiply divide compare negate shifter register dff tff jkff rsff counter \
                              ram ram2 rom lut driver driverinv pullup pulldown bitext bitsel seg7 hexdisplay rgbled reset \
                              break, or sub:<circuit>; settings such as bits, inputs, label, sel, value, net, in, out, wide, \
                              flip, addr, file); {op:'wire', points:[[x,y],…]} (each leg straight; a wire's end on another's \
                              middle joins it); {op:'move', ids, wires, dx, dy}; {op:'rotate', ids}; {op:'mirror', ids}; \
                              {op:'delete', ids, wires:[[x1,y1,x2,y2]]}; {op:'set', id, newId?, settings}; {op:'paste', text, \
                              dx, dy} (.circ text). Answers with the parts made and their pins. `create: true` makes \
                              the circuit first if the project has none of that name."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "circuit": { "type": "string" },
                        "create": { "type": "boolean" },
                        "ops": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["ops"]
                }),
            },
            Tool {
                name: "circuit_test".into(),
                description: "Run a circuit's test tables — all, or the one labelled — or a table given here in Digital's \
                              language (a line of signal names, then a row of values each: numbers, X, Z, C to pulse a \
                              clock; loop(i, n) … end loop) without keeping it. Answers each table's rows and the rows \
                              that failed with what came instead. `keep` with `label` keeps the given table."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "circuit": { "type": "string" },
                        "label": { "type": "string" },
                        "table": { "type": "string" },
                        "keep": { "type": "boolean" }
                    }
                }),
            },
            Tool {
                name: "circuit_analyse".into(),
                description: "A circuit's truth table (one without flip-flops, of 16 input bits or fewer), or that of \
                              expressions (`Y = A·B + C'; Z = A ^ B`): each output's least sum of products and product \
                              of sums, and the table itself for six inputs or fewer."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "circuit": { "type": "string" },
                        "expression": { "type": "string" }
                    }
                }),
            },
            Tool {
                name: "circuit_synthesise".into(),
                description: "Build a new circuit from expressions or from a circuit's truth table, laid out as AND/OR \
                              gates, NAND only or NOR only, its table attached as the test \"truth\". A held edit."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "expression": { "type": "string" },
                        "circuit": { "type": "string" },
                        "style": { "type": "string", "enum": ["andOr", "nand", "nor"] }
                    },
                    "required": ["name"]
                }),
            },
            // Logic's files. Writes are held until the person saves.
            Tool {
                name: "logic_read".into(),
                description: "The file open in Logic mode (with its lines numbered and its mistakes, or a proof's check) \
                              and the project's .logic, .proof and .l4f files; or, with `path`, that file — the open one \
                              as typed, unsaved changes and all."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            },
            Tool {
                name: "logic_write".into(),
                description: "Change a Logic file, or make one: `text` the file whole, or `old_text` (once in the file) \
                              and `new_text`. It is opened in Logic and held until the person saves; answers with its \
                              mistakes, or a proof's check. Proton's language: sorts `person = Ann | Bo`, `age = 1..3`, \
                              `thing = size 4` (four elements), `box = size 2..5` (as many as a model needs); symbols \
                              `of: person -> age  one-to-one` (onto, partial, symmetric), `likes: person, person -> bool`, \
                              `p, q: bool`; constraints, one a line: `not`, `and`, `or`, `->`, `<->`, `xor`, `=`, `!=`, \
                              `<`, `+`, `all x: …`, `some x in person: …`, `exactly 2 x: …`; questions `? p -> q`, \
                              `? p, p -> q |= q`, `? A == B`. `#` comments. A .proof file is a header — `system fitch` \
                              (or sequent, resolution), `premises A, B`, `goal C` — then its numbered lines: Fitch's \
                              `3  | p  assume`, `5  q  ->E 1, 3`; a sequent `2  p, q |- p  Ax`, cited lines after the \
                              rule; a clause `4  not P(x) or Q(x)  premise 1` / `goal` / `resolve 2, 3` / `factor 4`."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "In the project, ending .logic, .proof or .l4f." },
                        "text": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "logic_ask".into(),
                description: "Logic's calculator: a formula's truth table, whether it is a tautology, a contradiction or \
                              contingent, and its forms (least sum of products, CNF, DNF, NNF, NAND, NOR); whether an \
                              argument `A, B |= C` is valid, with a countermodel or a tableau's verdict for first-order \
                              ones; whether `A == B`. `question` is one statement (declarations may come above it, \
                              their last statement asked); or `line` of the file open (or of `path`), the cursor's \
                              when left out. `more` finds least forms for up to 16 propositions."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "line": { "type": "integer" },
                        "path": { "type": "string" },
                        "l4f": { "type": "boolean", "description": "The question is in Logic4Fun's language." },
                        "more": { "type": "boolean" }
                    }
                }),
            },
            Tool {
                name: "logic_solve".into(),
                description: "The models of a theory — the file open in Logic, `path`, or `theory` given here — found by \
                              Proton's model finder: the first `keep` (3 unless asked, 20 at most) with each symbol's \
                              values, `count` to count them all; or, when there is none, the lines that cannot all hold. \
                              The file open shows them too. Ten seconds at most."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "theory": { "type": "string" },
                        "path": { "type": "string" },
                        "l4f": { "type": "boolean" },
                        "keep": { "type": "integer" },
                        "count": { "type": "boolean" }
                    }
                }),
            },
            Tool {
                name: "logic_prove".into(),
                description: "A proof found, as a .proof file Proton has checked — of `argument` (`A, B |= C`, or a formula) \
                              in `system`: fitch (natural deduction, the default), sequent (Gentzen's LK) or resolution \
                              (clause form shown step by step, then a refutation) — or of the argument at `line` of the \
                              file open (or of `path`). A .proof file is checked line by line instead, each mistake said; \
                              `find` finds one for its header. `proof` checks a .proof file's text given here."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "argument": { "type": "string" },
                        "system": { "type": "string", "enum": ["fitch", "sequent", "resolution"] },
                        "line": { "type": "integer" },
                        "path": { "type": "string" },
                        "proof": { "type": "string" },
                        "find": { "type": "boolean" }
                    }
                }),
            },
            // CAD's files. Writes are held until the person saves.
            Tool {
                name: "cad_read".into(),
                description: "The part, assembly, drawing, schematic or board open in CAD mode — its lines numbered, what it builds (each \
                              body's volume, area and size; an assembly's instances, where they are and what holds \
                              them, its bill of materials and tubes; a drawing's views and dimensions; a schematic's parts, nets and \
                              rules check; a board's parts, pads, nets and design rules) and its mistakes, each at its \
                              line — and the project's .part, .assembly, .drawing, .schematic and .board files; or, with `path`, \
                              that file the same way, the open one as typed."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            },
            Tool {
                name: "cad_write".into(),
                description: "Change a CAD file, or make one: `text` the file whole, or `old_text` (once in the file) and \
                              `new_text`. It is opened in CAD and held until the person saves; answers with what it builds \
                              and its mistakes. One statement a line, millimetres, `#` comments. A .part: `part \"Name\"`, \
                              `units mm`, `material aluminium-6061`, `param t = 3`; `sketch NAME on XY|XZ|YZ|FACE [offset D]` \
                              … `end` holding shapes — `rect r corner (x, y) size = (w, h)`, `circle c center (x, y) \
                              radius = r` (or `diameter`), `line l (x, y) (x, y)`, `arc` — numbers where a shape is now, \
                              `= v` a dimension held; constraints `horizontal`, `vertical`, `coincident`, `parallel`, \
                              `perpendicular`, `tangent`, `equal`, `concentric`, `fix`, `hdistance A B = d`, `vdistance`, \
                              `distance`, `angle`; points `r.bl`, `r.tr`, `c.center`, `l.start`, `origin`. Features: \
                              `extrude NAME from SKETCH distance D [symmetric] [join|cut|intersect BODY]` (or `through`), \
                              `revolve NAME from SKETCH about xaxis|yaxis|LINE [angle A]`, `hole NAME on FACE at (x, y), \
                              P… diameter D|thread M3|clearance M3 depth D|through [counterbore D depth H|countersink D \
                              angle A]`, `pattern NAME of F linear (x, y, z) count N` (or `circular about AXIS count N`), \
                              `mirror NAME of F about PLANE`, `union|cut|intersect BODY with OTHER`, `fillet NAME on EDGES \
                              radius R`, `chamfer NAME on EDGES distance D`, `shell NAME of BODY thickness T [open FACE]`, \
                              `sweep NAME from SKETCH along PATH`, `loft NAME from A to B`, `sheet NAME from \
                              SKETCH thickness T bend R width W [k 0.44] [side right]` (sheet metal bent along the \
                              sketch's lines, its flat pattern written as DXF). Faces: `plate.top`, \
                              `plate.bottom`, `plate.side(r.left)` (a rect's left/right/top/bottom), `plate.side(h1)` (a \
                              circle's wall), a hole's `NAME.side(wall1)`; edges `F & G` or `plate.edges(parallel z)`, \
                              `boss.top.edges(convex, round)`. An .assembly: `assembly \"Name\"`, `units mm`, `clearance \
                              1`, `part NAME from \"FILE.part\"` — or a maker's `\"pump.step\"` or `\"fan.stl\"`, its \
                              faces named face1, face2 … where it gives none — (or `assembly NAME from …`), `ground NAME`, `place NAME at \
                              (x, y, z) turn (a, b, c)`, `mate NAME planar|flush|distance|angle|parallel|perpendicular|concentric| \
                              cylindrical|revolute|slider|fastened A to B [= v | offset v]` between faces \
                              `INSTANCE.FACE`, `explode NAME INST by (x, y, z), …`, `route NAME hard|soft 14/16 from \
                              INST.socket to INST.socket [via (x, y, z), …] [bend R]`. A .drawing: `drawing \"T\"`, `of \
                              \"FILE\"`, `sheet A3`, `scale 1/2`, `style paper|blueprint`, `view NAME \
                              front|back|top|bottom|left|right|iso at (x, y) [scale S] [hidden] [exploded NAME]`, `section \
                              NAME along x|y|z = V at (x, y)`, `detail NAME of VIEW at (x, y) radius R scale S place (x, \
                              y)`, `dim NAME VIEW distance A to B|diameter A|radius A|angle A to B|clearance P to Q`, `note \
                              NAME VIEW \"text\" at FACE`, `balloons VIEW`, `bom at (x, y)` — sheet millimetres from its \
                              bottom left. A .schematic: `schematic \"T\"`, `sheet A4`, `part REF KIND [value V] [footprint F] \
                              [at (x, y)] [turn 90]` — KIND resistor, capacitor, capacitor-polarised, inductor, diode, \
                              led, zener, fuse, switch, crystal, npn, pnp, nmos, pmos, `header 4` or `header 2x5`, `kicad \
                              \"Library:Name\"` from KiCad's libraries in the project or installed, `ic \
                              \"NAME\" pins (1 NAME KIND, …)` with pin KINDs input, output, bidir, tristate, passive, \
                              power-in, power-out, open-collector, open-emitter, nc, and the PC's connectors atx-24, \
                              eps-8, pcie-6, pcie-8, 12v-2x6, sata-power, fan-4pin, argb-5v, usb2-header, usb3-header, \
                              usb-c-header; `net NAME [power] REF.PIN, REF.PIN … [current A]` (a pin by number or by \
                              its only name; `power` when a cable brings it), `nc REF.PIN, …`; footprints (IPC-7351B) \
                              0201–2512, SOT-23(-5, -6), SOIC-N, TSSOP-N, LQFP-N, QFN-N, header-RxN(-2.0), \
                              mini-fit-jr-2xN, kk-254-N or a KiCad library's `Library:Name`, the PC's connectors taking theirs; its rules check, pins \
                              against pads too, comes back with it. A .board (made from a schematic): `board \"T\"`, \
                              `of \"x.schematic\"`, `layers 2|4`, `copper 1` (oz), `rules clearance 0.2 track 0.25 via \
                              0.6/0.3 edge 0.3 rise 10`, `outline rect (0, 0) size (60, 40) [corner 2]` (or `points (x, \
                              y) …`, or `from \"case.part\" sketch NAME`), `hole at (x, y) drill 3.2`, `place REF at \
                              (x, y) [turn 90] [back]`, `track NET F.Cu|In1.Cu|In2.Cu|B.Cu [width W] (x, y) (x, y) …`, \
                              `via NET at (x, y) [size 0.6/0.3]`, `zone NET LAYER (x, y) … [clearance C] [thermal T]` — \
                              mm, y up; cad_read gives every pad's place and net; its design rules (clearance, shorts, \
                              edge, IPC-2221 width for each net's current, what is left to route) come back with it. The library's parts (boards, cards, fans, radiators, fittings, the FormD \
                              T1) are in the project's library/ folder when made from CAD's home page."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "In the project, ending .part, .assembly, .drawing, .schematic or .board." },
                        "text": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "cad_measure".into(),
                description: "Measure the part or assembly open in CAD, or `path`: with nothing more, every body's (or \
                              every part's, where it is) volume, area, centre and size; `of` a body, an instance or a \
                              face — its kind, radius or diameter, axis, area; `of` and `to` two faces — flat faces' \
                              gap or angle, axes' distance, an axis's distance from a face — or two bodies or \
                              instances, the nearest they come, or that they overlap. Faces by name: `plate.top`, \
                              `plate.side(h1)`; an assembly's `INSTANCE.FACE`. A maker's .step or .stl is measured too."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "of": { "type": "string" },
                        "to": { "type": "string" }
                    }
                }),
            },
            Tool {
                name: "cad_check".into(),
                description: "Check the file open in CAD, or `path`: a part's sketches solved and bodies closed; an \
                              assembly's mates (what conflicts, what is still free), where its parts overlap or come \
                              nearer than its clearance, its tubes and the loop's order — the fit shown on its page \
                              too; a drawing's statements that cannot be drawn. With `t1`, the build against the \
                              FormD T1's published limits: `gpu` {length, height, slots}, `cooler` (the CPU cooler's \
                              or block's height, mm), `radiators` [{spot: top|bottom|side, size: 120|240|280, \
                              thickness: with its fans, mm}], `psu` (sfx or sfx-l), `water` (a loop or an AIO)."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "t1": {
                            "type": "object",
                            "properties": {
                                "gpu": { "type": "object", "properties": { "length": { "type": "number" }, "height": { "type": "number" }, "slots": { "type": "number" } } },
                                "cooler": { "type": "number" },
                                "radiators": { "type": "array", "items": { "type": "object", "properties": { "spot": { "type": "string", "enum": ["top", "bottom", "side"] }, "size": { "type": "number" }, "thickness": { "type": "number" } } } },
                                "psu": { "type": "string" },
                                "water": { "type": "boolean" }
                            }
                        }
                    }
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
        // What was typed, in the bubble; what went with it, as its chips.
        let (said, attached) = split_context(text);
        self.transcript.push(Entry {
            text: said,
            attached,
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
                        undo: false,
                        asks: false,
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
                            undo: false,
                        asks: false,
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
                        let (said, attached) = split_context(&text);
                        self.transcript.push(Entry {
                            text: said,
                            attached,
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
                                    undo: false,
                        asks: false,
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

    /// Say of a call whether what it wrote can still be taken back, and
    /// what it came to, wherever in the conversation it is.
    pub fn mark(&mut self, id: &str, undo: bool, result: Option<&str>) {
        for entry in &mut self.transcript {
            if let Some(call) = entry.calls.iter_mut().find(|c| c.id == id) {
                call.undo = undo;
                if let Some(result) = result {
                    call.result = result.to_string();
                }
            }
        }
    }

    /// Say of a call whether it is waiting for the person to say yes, and
    /// what is happening to it meanwhile.
    pub fn asking(&mut self, id: &str, asks: bool, result: &str) {
        for entry in &mut self.transcript {
            if let Some(call) = entry.calls.iter_mut().find(|c| c.id == id) {
                call.asks = asks;
                call.result = result.to_string();
            }
        }
    }

    /// Show a call's argument whole: a command is read before it is run,
    /// and the row's one-line summary would cut a long one short.
    pub fn show_whole(&mut self, id: &str, detail: &str) {
        for entry in &mut self.transcript {
            if let Some(call) = entry.calls.iter_mut().find(|c| c.id == id) {
                call.detail = detail.to_string();
            }
        }
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
    fn a_question_is_shown_as_typed_and_what_went_with_it_by_name() {
        let sent = "why the red line?\n\n# The file open in the editor\n`src/queue.h` (all 2 lines); the cursor is on line 1.\n\n```h\nint x;\n```\n\n# @selection\n`queue.h` lines 1-1\n\n```\nint x;\n```";
        assert_eq!(split_context(sent), ("why the red line?".to_string(), vec!["src/queue.h".to_string(), "@selection".to_string()]));
        // A heading of the person's own is part of what they wrote.
        let own = "notes:\n\n# Plan\nstep one";
        assert_eq!(split_context(own), (own.to_string(), Vec::new()));
        // Restored from a conversation's file, as typed too.
        let mut agent = Agent::new("/tmp");
        agent.restore(vec![Message::user(vec![Block::text(sent)])]);
        assert_eq!(agent.transcript()[0].text, "why the red line?");
        assert_eq!(agent.transcript()[0].attached.len(), 2);
    }

    #[test]
    fn the_tool_list_is_stable_and_documented() {
        // Both properties matter: the order feeds a cached prefix, and the
        // descriptions are the only thing the model has to choose between
        // `grep` and `find_files`.
        let tools = Agent::tools();
        assert_eq!(tools.len(), 33);
        // A command is asked about first, and the tool says so: a model that
        // thought it ran unasked would report work nobody approved.
        let command = tools.iter().find(|t| t.name == "run_command").unwrap();
        assert!(command.description.contains("ASKED first"), "{}", command.description);
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
        // `edit_file` has to say when it stages rather than writes, and that
        // its result says which: a model that assumes either reports what
        // did not happen. `edit_cell` stages always: a model told only "edit
        // a cell" reports the job as done, and the user reads that as a
        // notebook that has changed.
        for name in ["edit_file", "edit_cell"] {
            let editing = tools.iter().find(|t| t.name == name).unwrap();
            assert!(
                editing.description.contains("STAGED"),
                "{name} must say it does not apply: {}",
                editing.description
            );
        }
        // Logic's writes are held, and it says so: a model told only "write
        // a file" reports the file as written, and it is not, yet.
        for name in ["logic_write", "cad_write"] {
            let writing = tools.iter().find(|t| t.name == name).unwrap();
            assert!(writing.description.contains("held until the person saves"), "{}", writing.description);
        }
        // The modes that left took their tools with them.
        let gone = ["pdf_", "latex_log", "game_", "canvas_", "flow_"];
        assert!(!tools.iter().any(|t| gone.iter().any(|g| t.name.starts_with(g))), "a tool of a mode that left is still offered");
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
