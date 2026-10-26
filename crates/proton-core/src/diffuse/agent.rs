//! The agent's Diffuse tools: the flows read — the one shown, its nodes,
//! their values and links, and what stops it running — with forge's nodes
//! and models; a flow made, opened and edited; and a run on forge, waited
//! for until its images are saved.
//!
//! An edit is the person's kind of edit: a call's ops are one step of their
//! undo, held until they save, on their screen at once. A node the agent
//! adds is placed for it when it gives no place — in the column its links
//! put it in, below what is there — and named by a `ref` of its choosing
//! that later ops in the same call can use in place of an id it cannot know
//! yet.
//!
//! The agent lives in a workspace and Diffuse in the session, so the
//! workspace defers these calls and the session answers them, as it does
//! the PDF tools (`Workspace::take_session_calls`). A run the agent waits for
//! is answered once it has ended.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use proton_diffuse::{Catalogue, Flow, FlowDoc, HealthState, Op, Problem, SeedMode};
use serde_json::Value;

use super::{find, inside, Diffuse, RunState, EXTENSION};
use crate::bus::{Command, Event, NoticeLevel};

/// The tools answered here, by name.
pub const TOOLS: [&str; 3] = ["flow_info", "flow_edit", "flow_run"];

/// The most ops one call applies.
const OPS: usize = 500;
/// The most choices a field's list shows.
const CHOICES: usize = 40;
/// The most models a list shows.
const MODELS: usize = 40;
/// The runs `flow_info` tells of, the newest.
const RUNS_TOLD: usize = 5;

/// What a call is answered with: now, or once run `n` has ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    Now(Result<String, String>),
    Wait(u32),
}

/// A value as a model reads it: a string quoted, the rest as JSON.
fn shown(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 120 {
        format!("{}…", text.chars().take(119).collect::<String>())
    } else {
        text
    }
}

/// The first warning or error among `events`.
fn refused(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::Notice { text, level: NoticeLevel::Warn | NoticeLevel::Error } => Some(text.clone()),
        _ => None,
    })
}

/// A flow in words: its nodes with their values and the links into them,
/// and what stops it running.
fn flow_words(path: &str, flow: &Flow, problems: &[Problem], catalogue: Option<&Catalogue>) -> String {
    let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(EXTENSION);
    let mut out = format!(
        "Its images go to {}/ in Diffuse's folder.",
        flow.outputs.clone().unwrap_or_else(|| format!("outputs/{stem}"))
    );
    if let Some(note) = &flow.note {
        let _ = write!(out, " Its note: {note}");
    }
    if flow.nodes.is_empty() {
        out.push_str("\nIt has no nodes yet.");
    } else {
        out.push_str("\nNodes, by id (a link into an input is written input ← node:output):");
    }
    for node in &flow.nodes {
        let title = catalogue.and_then(|c| c.node(&node.kind)).map(|k| k.title.as_str()).unwrap_or("not one forge has");
        let _ = write!(out, "\n- {} {} ({title}) at [{}, {}]", node.id, node.kind, node.at[0], node.at[1]);
        let values: Vec<String> = node.values.iter().map(|(k, v)| format!("{k}={}", shown(v))).collect();
        if !values.is_empty() {
            let _ = write!(out, ": {}", values.join(", "));
        }
        match node.seed {
            Some(SeedMode::Increment) => out.push_str("; its seed goes up by one after each run"),
            Some(SeedMode::Random) => out.push_str("; its seed is new after each run"),
            _ => {}
        }
        let prefix = format!("{}:", node.id);
        let into: Vec<String> = flow
            .links
            .iter()
            .filter_map(|l| l.to.strip_prefix(&prefix).map(|input| format!("{input} ← {}", l.from)))
            .collect();
        if !into.is_empty() {
            let _ = write!(out, "; {}", into.join(", "));
        }
        if node.bypass.0 {
            out.push_str("; bypassed: left out of runs");
        }
    }
    if problems.is_empty() {
        out.push_str("\nReady to run.");
    } else {
        out.push_str("\nWhat stops it running:");
        for p in problems {
            let at = match (&p.node, &p.input) {
                (Some(node), Some(input)) => format!("node {node}, {input}: "),
                (Some(node), None) => format!("node {node}: "),
                _ => String::new(),
            };
            let _ = write!(out, "\n- {at}{}", p.message);
        }
    }
    out
}

/// A port's node swapped for the id its ref stands for.
fn port_of(port: &str, refs: &BTreeMap<String, String>) -> String {
    match port.split_once(':') {
        Some((node, name)) => format!("{}:{name}", refs.get(node).map(String::as_str).unwrap_or(node)),
        None => port.to_string(),
    }
}

/// An op as the agent wrote it, its refs swapped for ids.
fn with_ids(op: &mut Value, refs: &BTreeMap<String, String>) {
    let Some(fields) = op.as_object_mut() else { return };
    if let Some(Value::String(id)) = fields.get_mut("id") {
        if let Some(real) = refs.get(id.as_str()) {
            *id = real.clone();
        }
    }
    if let Some(Value::Array(ids)) = fields.get_mut("ids") {
        for id in ids.iter_mut() {
            if let Some(real) = id.as_str().and_then(|i| refs.get(i)) {
                *id = Value::from(real.clone());
            }
        }
    }
    for key in ["from", "to"] {
        if let Some(Value::String(port)) = fields.get_mut(key) {
            *port = port_of(port, refs);
        }
    }
}

/// Places for the nodes added with none: in the column their links put them
/// in — one right of the deepest that feeds them — below what is in that
/// column already.
fn places(flow: &Flow, unplaced: &[String], catalogue: &Catalogue) -> Vec<Op> {
    let graph = proton_diffuse::Graph {
        nodes: flow
            .nodes
            .iter()
            .map(|n| proton_diffuse::flow::GraphNode {
                id: n.id.clone(),
                kind: n.kind.clone(),
                values: n.values.clone(),
            })
            .collect(),
        links: flow.links.clone(),
    };
    let laid = proton_diffuse::layout::flow_from_graph(&graph, catalogue);
    // A flow made whole here is laid out whole.
    if flow.nodes.iter().all(|n| unplaced.contains(&n.id)) {
        return laid.nodes.iter().map(|n| Op::Place { id: n.id.clone(), at: n.at }).collect();
    }
    let mut placed: Vec<([f64; 2], f64)> = flow
        .nodes
        .iter()
        .filter(|n| !unplaced.contains(&n.id))
        .map(|n| (n.at, proton_diffuse::layout::height(catalogue, &n.kind)))
        .collect();
    let mut ops = Vec::new();
    for id in unplaced {
        let Some(node) = laid.nodes.iter().find(|n| &n.id == id) else { continue };
        let x = node.at[0];
        let below = placed
            .iter()
            .filter(|(at, _)| (at[0] - x).abs() < 220.0)
            .map(|(at, height)| at[1] + height + 24.0)
            .fold(16.0_f64, f64::max);
        let at = [x, below];
        placed.push((at, proton_diffuse::layout::height(catalogue, &node.kind)));
        ops.push(Op::Place { id: id.clone(), at });
    }
    ops
}

impl Diffuse {
    /// Diffuse in a few lines: its folder, forge, the flows and those open.
    fn state_words(&self, root: &Path) -> String {
        let mut out = format!("Diffuse's folder is {}.", crate::folder::tilde(root));
        match self.known.as_ref().map(|k| &k.health) {
            Some(h) if h.state == HealthState::Serving => {
                out.push_str(" forge is serving images");
                if let Some(status) = &h.status {
                    let gb = |b: u64| b as f64 / 1e9;
                    let _ = write!(
                        out,
                        " on {} ({:.1} of {:.1} GB used)",
                        status.gpu.name,
                        gb(status.gpu.used),
                        gb(status.gpu.total)
                    );
                }
                out.push('.');
            }
            Some(h) if h.state == HealthState::Looking => out.push_str(" forge has not been looked at yet."),
            Some(h) => {
                let _ = write!(out, " forge is not serving: {}", h.message);
            }
            None => out.push_str(" forge has not been looked at yet."),
        }
        let flows = find(root);
        if flows.is_empty() {
            out.push_str("\nNo flows yet; flow_edit with `new` makes one.");
        } else {
            let _ = write!(out, "\nFlows: {}.", flows.join(", "));
        }
        if !self.open.is_empty() {
            let open: Vec<String> = self
                .open
                .iter()
                .map(|(rel, doc)| format!("{rel}{}", if doc.is_dirty() { " (changes not saved)" } else { "" }))
                .collect();
            let _ = write!(out, "\nOpen: {}.", open.join(", "));
        }
        out
    }

    /// The flow `path` in words — open, or read from its file.
    fn flow_about(&self, root: &Path, path: &str) -> Result<String, String> {
        let catalogue = self.known.as_ref().and_then(|k| k.catalogue.as_ref());
        let models = self.known.as_ref().and_then(|k| k.models.as_ref());
        let (flow, dirty, held) = match self.open.iter().find(|(rel, _)| rel == path) {
            Some((_, doc)) => (doc.get().clone(), doc.is_dirty(), true),
            None => (FlowDoc::read_sealed(&inside(root, path)?, self.seal.as_ref())?.get().clone(), false, false),
        };
        let problems = catalogue.map(|c| proton_diffuse::check(&flow, c, models)).unwrap_or_default();
        let how = match (held, self.shown.as_deref() == Some(path), dirty) {
            (false, _, _) => "not open",
            (true, true, true) => "on the person's screen, with changes not saved",
            (true, true, false) => "on the person's screen",
            (true, false, true) => "open, not shown, with changes not saved",
            (true, false, false) => "open, not shown",
        };
        Ok(format!("Flow {path} ({how}). {}", flow_words(path, &flow, &problems, catalogue)))
    }

    /// What `@flow` sends: Diffuse, and the flow on screen.
    pub fn about(&self, root: &Path) -> String {
        if self.locked() {
            return "Diffuse is locked: the person has not unlocked it, so nothing of its flows can be said.".into();
        }
        let mut out = self.state_words(root);
        match &self.shown {
            Some(path) => {
                out.push('\n');
                out.push_str(&self.flow_about(root, path).unwrap_or_else(|why| why));
            }
            None => out.push_str("\nNo flow is on the person's screen."),
        }
        out.push_str("\nflow_info lists the nodes forge offers and their fields.");
        out
    }

    /// One of the agent's Diffuse tools, and what the window draws again.
    pub fn tool(&mut self, root: &Path, name: &str, input: &Value) -> (Said, Vec<Event>) {
        if self.locked() {
            let why = "Diffuse is locked: its files are sealed until the person unlocks it with Touch ID or their password, in Diffuse.";
            return (Said::Now(Err(why.into())), Vec::new());
        }
        match name {
            "flow_info" => (Said::Now(self.tool_info(root, input)), Vec::new()),
            "flow_edit" => {
                let mut events = Vec::new();
                let answer = self.tool_edit(root, input, &mut events);
                (Said::Now(answer), events)
            }
            "flow_run" => self.tool_run(root, input),
            _ => (Said::Now(Err(format!("{name}: not a Diffuse tool"))), Vec::new()),
        }
    }

    fn tool_info(&self, root: &Path, input: &Value) -> Result<String, String> {
        let mut out = self.state_words(root);
        let path = input.get("flow").and_then(Value::as_str).map(str::to_string).or_else(|| self.shown.clone());
        match &path {
            Some(path) => {
                out.push('\n');
                out.push_str(&self.flow_about(root, path)?);
            }
            None => out.push_str("\nNo flow is on the person's screen."),
        }
        let known = self.known.as_ref();
        let Some(catalogue) = known.and_then(|k| k.catalogue.as_ref()) else {
            out.push_str("\nforge's nodes are not known yet: it has not been reached.");
            return Ok(out);
        };
        let models = known.and_then(|k| k.models.as_ref());
        match input.get("nodes").and_then(Value::as_array) {
            Some(asked) => {
                for name in asked.iter().filter_map(Value::as_str) {
                    let Some(kind) = catalogue.node(name) else {
                        let _ = write!(out, "\n\nforge has no node {name}.");
                        continue;
                    };
                    let _ = write!(out, "\n\n{} \"{}\" ({})", kind.name, kind.title, kind.group);
                    for i in &kind.inputs {
                        match &i.socket {
                            Some(socket) => {
                                let _ = write!(
                                    out,
                                    "\n- input {} takes {socket}{}",
                                    i.name,
                                    if i.optional { ", optional" } else { "" }
                                );
                            }
                            None => {
                                let _ = write!(out, "\n- field {}: {}", i.name, i.kind.as_deref().unwrap_or("value"));
                                match (&i.min, &i.max) {
                                    (Some(min), Some(max)) => {
                                        let _ = write!(out, ", {min} to {max}");
                                    }
                                    (Some(min), None) => {
                                        let _ = write!(out, ", at least {min}");
                                    }
                                    (None, Some(max)) => {
                                        let _ = write!(out, ", at most {max}");
                                    }
                                    (None, None) => {}
                                }
                                // A whole number's step of one says nothing.
                                if let Some(step) = i.step.as_ref().filter(|s| s.as_f64() != Some(1.0)) {
                                    let _ = write!(out, ", a multiple of {step}");
                                }
                                if let Some(default) = &i.default {
                                    let _ = write!(out, ", {} if not set", shown(default));
                                }
                                if let Some(from) = &i.from {
                                    match catalogue.choices(from, models) {
                                        Some(list) if list.len() > CHOICES => {
                                            let _ = write!(
                                                out,
                                                ", one of: {}, … ({} in all)",
                                                list[..CHOICES].join(", "),
                                                list.len()
                                            );
                                        }
                                        Some(list) => {
                                            let _ = write!(out, ", one of: {}", list.join(", "));
                                        }
                                        None => {
                                            let _ = write!(out, ", one of forge's {from} (not known yet)");
                                        }
                                    }
                                }
                                if i.kind.as_deref() == Some("upload") {
                                    out.push_str(" — a picture in Diffuse's folder, such as inputs/name.png, which the person chooses");
                                }
                            }
                        }
                    }
                    for o in &kind.outputs {
                        let _ = write!(out, "\n- output {} gives {}", o.name, o.socket);
                    }
                    if kind.is_output() {
                        out.push_str("\n- an output node: what a run is run for");
                    }
                }
            }
            None => {
                out.push_str(
                    "\n\nThe nodes forge offers (flow_info with `nodes` gives their fields), each: inputs → outputs:",
                );
                for kind in &catalogue.nodes {
                    let ins: Vec<String> = kind
                        .inputs
                        .iter()
                        .filter_map(|i| i.socket.as_ref().map(|s| format!("{} {s}", i.name)))
                        .collect();
                    let outs: Vec<String> = kind.outputs.iter().map(|o| format!("{} {}", o.name, o.socket)).collect();
                    let fields: Vec<&str> = kind.fields().map(|f| f.name.as_str()).collect();
                    let _ = write!(
                        out,
                        "\n- {} \"{}\" ({}): {} → {}{}",
                        kind.name,
                        kind.title,
                        kind.group,
                        if ins.is_empty() { "nothing".to_string() } else { ins.join(", ") },
                        if outs.is_empty() { "an image kept".to_string() } else { outs.join(", ") },
                        if fields.is_empty() { String::new() } else { format!("; fields {}", fields.join(", ")) }
                    );
                }
                if let Some(models) = models {
                    out.push_str("\nModels on forge:");
                    for (list, items) in &models.0 {
                        let named: Vec<String> = items
                            .iter()
                            .take(MODELS)
                            .map(|m| match &m.family {
                                Some(family) => format!("{} ({family})", m.name),
                                None => m.name.clone(),
                            })
                            .collect();
                        let more = if items.len() > MODELS {
                            format!(", … ({} in all)", items.len())
                        } else {
                            String::new()
                        };
                        let _ = write!(
                            out,
                            "\n- {list}: {}{more}",
                            if named.is_empty() { "none".into() } else { named.join(", ") }
                        );
                    }
                }
                let starters: Vec<String> = proton_diffuse::templates::TEMPLATES
                    .iter()
                    .map(|(name, _, about)| format!("{name} ({about})"))
                    .collect();
                let _ = write!(out, "\nStarters a new flow can begin from: {}.", starters.join(", "));
            }
        }
        let told: Vec<String> = self
            .runs
            .iter()
            .rev()
            .take(RUNS_TOLD)
            .map(|r| {
                let mut line = format!("run {} of {}: {:?}", r.id, r.flow, r.state).to_lowercase();
                if let Some(message) = &r.message {
                    let _ = write!(line, ", {message}");
                }
                for o in r.outputs.iter().filter(|o| o.kept) {
                    let _ = write!(line, ", made {}", o.path);
                }
                line
            })
            .collect();
        if !told.is_empty() {
            let _ = write!(out, "\nRuns, newest first: {}.", told.join("; "));
        }
        Ok(out)
    }

    /// The flow a call works on: the one named, opened if it is not, or the
    /// one shown.
    fn target(&mut self, root: &Path, input: &Value, events: &mut Vec<Event>) -> Result<String, String> {
        let path = match input.get("flow").and_then(Value::as_str) {
            Some(path) => path.to_string(),
            None => self
                .shown
                .clone()
                .ok_or("no flow is on the person's screen: name one with `flow`, or make one with `new`")?,
        };
        if self.open.iter().all(|(rel, _)| rel != &path) {
            let opened = self.apply(root, Command::DiffuseOpen { path: path.clone() });
            if let Some(why) = refused(&opened) {
                return Err(why);
            }
            events.extend(opened);
        }
        Ok(path)
    }

    fn tool_edit(&mut self, root: &Path, input: &Value, events: &mut Vec<Event>) -> Result<String, String> {
        let mut said = Vec::new();
        if let Some(new) = input.get("new") {
            let name = new.get("name").and_then(Value::as_str).ok_or("flow_edit: `new.name` is required")?.to_string();
            let template = new.get("starter").and_then(Value::as_str).map(str::to_string);
            let made = self.apply(root, Command::DiffuseNew { name, template: template.clone() });
            if let Some(why) = refused(&made) {
                return Err(format!("flow_edit made nothing: {why}"));
            }
            events.extend(made);
            let path = self.shown.clone().unwrap_or_default();
            said.push(match template {
                Some(t) => format!("made {path} from the {t} starter and put it on the person's screen"),
                None => format!("made {path}, empty, and put it on the person's screen"),
            });
        }
        let path = self.target(root, input, events)?;
        if input.get("flow").is_some() && self.shown.as_deref() != Some(path.as_str()) {
            self.shown = Some(path.clone());
            said.push(format!("put {path} on the person's screen"));
        }
        let mut added = Vec::new();
        if let Some(list) = input.get("ops").and_then(Value::as_array) {
            if list.len() > OPS {
                return Err(format!("flow_edit: at most {OPS} ops a call"));
            }
            let catalogue = self.catalogue();
            if catalogue.nodes.is_empty() {
                return Err("flow_edit: forge's nodes are not known yet, so nothing can be checked; ask the person to look at forge (Diffuse's Server tab)".into());
            }
            let doc = &self.open.iter().find(|(rel, _)| rel == &path).ok_or("the flow is not open")?.1;
            // Each op tried on a copy as it comes, so a ref can name the node
            // an earlier one added; then all of them applied as one step.
            let mut probe = doc.get().clone();
            let mut refs = BTreeMap::new();
            let mut ops = Vec::new();
            let mut unplaced = Vec::new();
            for (i, raw) in list.iter().enumerate() {
                let fail = |why: String| format!("flow_edit changed nothing: op {}: {why}", i + 1);
                let mut v = raw.clone();
                let named =
                    v.as_object_mut().and_then(|o| o.remove("ref")).and_then(|r| r.as_str().map(str::to_string));
                with_ids(&mut v, &refs);
                let adding = v.get("op").and_then(Value::as_str) == Some("add");
                let place = adding && v.get("at").is_none();
                if place {
                    v["at"] = serde_json::json!([0, 0]);
                }
                let op: Op = serde_json::from_value(v).map_err(|e| fail(format!("not an op: {e}")))?;
                let applied =
                    proton_diffuse::op::apply(&mut probe, std::slice::from_ref(&op), &catalogue).map_err(fail)?;
                if let Some(id) = applied.added.first() {
                    if let Some(name) = named {
                        refs.insert(name, id.clone());
                    }
                    if place {
                        unplaced.push(id.clone());
                    }
                }
                ops.push(op);
            }
            if !unplaced.is_empty() {
                ops.extend(places(&probe, &unplaced, &catalogue));
            }
            let doc = &mut self.open.iter_mut().find(|(rel, _)| rel == &path).ok_or("the flow is not open")?.1;
            let applied = doc.apply(&ops, &catalogue).map_err(|why| format!("flow_edit changed nothing: {why}"))?;
            if applied.changed {
                said.push(format!(
                    "applied {} op{} to {path} as one step of the person's undo",
                    list.len(),
                    if list.len() == 1 { "" } else { "s" }
                ));
            } else {
                said.push(format!("{path} was already so: nothing changed"));
            }
            added = applied.added;
            if !added.is_empty() {
                let by_ref: Vec<String> = refs.iter().map(|(name, id)| format!("{name} is {id}")).collect();
                said.push(format!(
                    "added node{} {}{}",
                    if added.len() == 1 { "" } else { "s" },
                    added.join(", "),
                    if by_ref.is_empty() { String::new() } else { format!(" ({})", by_ref.join(", ")) }
                ));
            }
        }
        if input.get("save").and_then(Value::as_bool) == Some(true) {
            let doc = &mut self.open.iter_mut().find(|(rel, _)| rel == &path).ok_or("the flow is not open")?.1;
            doc.save()?;
            said.push(format!("saved {path}"));
        }
        if said.is_empty() {
            return Err("flow_edit: give `new`, `flow`, `ops` or `save`".into());
        }
        let dirty = self.open.iter().find(|(rel, _)| rel == &path).is_some_and(|(_, doc)| doc.is_dirty());
        events.push(self.view_event(root, added));
        let mut out = said.join("; ");
        if let Some(first) = out.get(..1) {
            out = first.to_uppercase() + &out[1..];
        }
        out.push('.');
        if dirty {
            out.push_str(" It is on the person's screen now and held until the person saves (⌘S) — say what you changed rather than that a file was written.");
        }
        out.push('\n');
        out.push_str(&self.flow_about(root, &path)?);
        Ok(out)
    }

    fn tool_run(&mut self, root: &Path, input: &Value) -> (Said, Vec<Event>) {
        if let Some(id) = input.get("cancel").and_then(Value::as_u64) {
            let id = id as u32;
            return match self.runs.iter().find(|r| r.id == id) {
                None => (Said::Now(Err(format!("there is no run {id}"))), Vec::new()),
                Some(r) if r.state.ended() => (Said::Now(Err(format!("run {id} has ended already"))), Vec::new()),
                Some(_) => {
                    let events = self.apply(root, Command::DiffuseCancel { id });
                    (Said::Now(Ok(format!("Asked forge to stop run {id}."))), events)
                }
            };
        }
        let mut events = Vec::new();
        let path = match self.target(root, input, &mut events) {
            Ok(path) => path,
            Err(why) => return (Said::Now(Err(why)), events),
        };
        let before = self.next_run;
        let ran = self.apply(root, Command::DiffuseRun { path: path.clone() });
        if self.next_run == before {
            let why = refused(&ran).unwrap_or_else(|| format!("{path} did not run"));
            events.extend(ran);
            return (Said::Now(Err(why)), events);
        }
        events.extend(ran);
        let id = self.next_run;
        if input.get("wait").and_then(Value::as_bool) == Some(false) {
            let said = format!("Sent {path} to forge as run {id}; flow_info says how it goes, and flow_run with cancel: {id} stops it.");
            return (Said::Now(Ok(said)), events);
        }
        (Said::Wait(id), events)
    }

    /// What a run the agent waited for came to, once it has ended.
    pub fn waited(&self, id: u32) -> Option<Result<String, String>> {
        let Some(r) = self.runs.iter().find(|r| r.id == id) else {
            return Some(Err(format!("run {id} is no longer in the Queue")));
        };
        if !r.state.ended() {
            return None;
        }
        let kept: Vec<String> =
            r.outputs.iter().filter(|o| o.kept).map(|o| format!("{} ({} × {})", o.path, o.width, o.height)).collect();
        let at = |p: &super::RunProblem| match &p.node {
            Some(node) => format!("node {node}: {}", p.message),
            None => p.message.clone(),
        };
        Some(match r.state {
            RunState::Done => Ok(format!(
                "Run {id} of {} done{}. {} The person sees {} in Diffuse's Gallery; a seed set to move after a run has moved, an edit they can undo.",
                r.flow,
                r.seconds.map(|s| format!(" in {s:.1} s")).unwrap_or_default(),
                if kept.is_empty() { "It kept no image: the flow has no Save Image.".to_string() } else { format!("Saved in Diffuse's folder: {}.", kept.join(", ")) },
                if kept.len() == 1 { "it" } else { "them" }
            )),
            RunState::Cancelled => Err(format!("Run {id} of {} was stopped before it finished.", r.flow)),
            _ => {
                let mut why = format!("Run {id} of {} failed: {}", r.flow, r.message.clone().unwrap_or_else(|| "forge did not say why".into()));
                let problems: Vec<String> = r.problems.iter().filter(|p| p.node.is_some()).map(at).collect();
                if !problems.is_empty() {
                    let _ = write!(why, " ({})", problems.join("; "));
                }
                Err(why)
            }
        })
    }
}
