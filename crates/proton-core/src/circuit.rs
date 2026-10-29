//! Circuit mode: the circuit project open in this project, and its saves.
//!
//! `proton-circuit` owns the format and the simulator. What lives here is
//! the editor's half: which `Name.circuit/` folder is open, which of its
//! circuits is on screen, whether it has changed since it was saved, and the
//! projects a folder holds, for the home page shown when none is.
//!
//! The window is sent a circuit as it draws it: each part with its pins
//! already placed, as the simulator will join them, so a pin drawn is a pin
//! that connects. `generation` numbers each project opened, so an answer
//! about the last one is never taken for this.
//!
//! Edits are held until ⌘S, as Codex's are, and the quit prompt names an
//! unsaved project. Each edit — the ops the window sends together, a
//! circuit added, the top changed — is a step to undo, kept as the project
//! before and after it: a circuit is small beside what that costs.
//!
//! Running is of the circuit shown, built afresh when it is started and
//! after any edit, its inputs set again as they were. A click sets an
//! input and the circuit settles; with `hold`, the change waits and gate
//! steps take it a delay at a time. A quick run takes the simulator to a
//! thread of its own and flips the clocks as fast as they go, nothing
//! drawn, until a Break rises or it is stopped, and says how fast it went.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use proton_circuit::circ::{self, Drawing, Part, Project, Wire};
use proton_circuit::sim::{NetId, PartId};
use proton_circuit::{edit, Sim, Value};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

/// A circuit project's folder ends so.
pub const EXTENSION: &str = "circuit";

/// A circuit project found in the folder, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitFound {
    /// As the project names it.
    pub path: String,
    pub name: String,
}

/// A pin as drawn, in grid steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitPin {
    pub x: i32,
    pub y: i32,
    pub input: bool,
}

/// A part as drawn: where it is, how it is turned, its settings, and its
/// pins where the simulator joins them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitPartView {
    pub id: String,
    /// `and`, `in`, `mux`… or `sub:<circuit>`.
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub rot: u8,
    pub mirror: bool,
    /// Its settings by their `.circ` names, sorted.
    pub settings: Vec<(String, String)>,
    pub pins: Vec<CircuitPin>,
    /// Why its pins could not be placed, when they could not.
    pub problem: Option<String>,
}

/// One change to a drawing. The window sends a list of them as one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CircuitOp {
    /// A new part, its id made from its kind.
    Place { kind: String, x: i32, y: i32, rot: u8, mirror: bool, settings: Vec<(String, String)> },
    /// Parts and wires moved by (dx, dy).
    Move { ids: Vec<String>, wires: Vec<[i32; 4]>, dx: i32, dy: i32 },
    /// Parts turned a quarter, each about its own origin.
    Rotate { ids: Vec<String> },
    /// Parts mirrored, or put back.
    Mirror { ids: Vec<String> },
    Delete { ids: Vec<String>, wires: Vec<[i32; 4]> },
    /// A wire through these points, each leg straight.
    Wire { points: Vec<[i32; 2]> },
    /// A part's settings, all of them, and a new id if it is renamed.
    Set { id: String, new_id: Option<String>, settings: Vec<(String, String)> },
    /// `.circ` text put in, moved by (dx, dy).
    Paste { text: String, dx: i32, dy: i32 },
}

/// The project open, and the circuit of it on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitProjectView {
    /// As the project names it.
    pub path: String,
    pub name: String,
    /// The circuit that runs when the project does.
    pub top: String,
    pub circuits: Vec<String>,
    /// The circuit on screen.
    pub shown: String,
    pub parts: Vec<CircuitPartView>,
    /// Each wire's ends: x, y, x, y in grid steps.
    pub wires: Vec<[i32; 4]>,
    /// The labels of the shown circuit's test tables.
    pub tests: Vec<String>,
    /// The project's memory contents, `data/<name>.hex`, by name.
    pub data: Vec<String>,
    pub generation: u32,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The parts the last edit made — placed or pasted — for the window to
    /// select.
    pub made: Vec<String>,
    /// What can be placed in the circuit shown: every kind, and each other
    /// circuit that does not place it, as `sub:<name>`.
    pub palette: Vec<String>,
}

/// A value on a wire or a part, as drawn: its width, its bits in hex, and
/// whether it is high-Z — all of it, or some.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitValue {
    pub bits: u8,
    pub hex: String,
    pub z: bool,
    pub some_z: bool,
}

impl From<Value> for CircuitValue {
    fn from(v: Value) -> Self {
        let mask = proton_circuit::value::mask(v.bits);
        CircuitValue { bits: v.bits, hex: format!("{:x}", v.get()), z: v.z & mask == mask, some_z: v.z & mask != 0 }
    }
}

/// A quick run: going or ended, how many clock cycles, and how fast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitQuickView {
    pub running: bool,
    #[ts(type = "number")]
    pub cycles: u64,
    /// Cycles a second.
    pub hz: f64,
    /// Why it ended, once it has.
    pub ended: Option<String>,
}

/// The circuit running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitRunView {
    pub generation: u32,
    pub circuit: String,
    /// Each wire's value, in the view's order; none on a wire no pin is on,
    /// or while a quick run has the simulator.
    pub wires: Vec<Option<CircuitValue>>,
    /// Inputs, clocks and buttons, and outputs, LEDs and probes, by id.
    pub values: Vec<(String, CircuitValue)>,
    /// Parts waiting for a gate step.
    pub waiting: u32,
    /// The parts the last gate step worked.
    pub stepped: Vec<String>,
    /// Why it is not running: it would not settle, two drivers disagree.
    pub error: Option<String>,
    pub quick: Option<CircuitQuickView>,
}

enum QuickNews {
    Cycles(u64),
    Done { sim: Box<Sim>, cycles: u64, ended: String },
}

/// A quick run on its thread; stopped when this is dropped.
struct Quick {
    stop: Arc<AtomicBool>,
    news: mpsc::Receiver<QuickNews>,
    cycles: u64,
    started: Instant,
}

impl Drop for Quick {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Run {
    circuit: String,
    /// None while a quick run has it, or when it would not start.
    sim: Option<Sim>,
    error: Option<String>,
    /// Each drawn part by id: the part it became, and its kind.
    parts: HashMap<String, (PartId, String)>,
    wires: Vec<Option<NetId>>,
    clocks: Vec<PartId>,
    breaks: Vec<PartId>,
    /// What each input was set to, to set again when it is built afresh.
    inputs: BTreeMap<String, u64>,
    stepped: Vec<String>,
    quick: Option<Quick>,
    last_quick: Option<CircuitQuickView>,
}

/// The kinds a click sets, and those that show a value.
const SETTABLE: [&str; 3] = ["in", "clock", "button"];
const SHOWN: [&str; 6] = ["out", "led", "probe", "seg7", "hexdisplay", "rgbled"];

/// A step to undo: the project before and after, and the circuit shown.
#[derive(Debug, Clone)]
struct Step {
    before: Project,
    after: Project,
    shown: String,
}

/// No more steps than this are kept to undo.
const MOST_STEPS: usize = 300;

struct Open {
    path: PathBuf,
    shown_path: String,
    name: String,
    project: Project,
    /// As it was last read or written: unsaved when they differ.
    saved: Project,
    shown: String,
    generation: u32,
    done: Vec<Step>,
    undone: Vec<Step>,
    run: Option<Run>,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("shown_path", &self.shown_path).field("generation", &self.generation).finish()
    }
}

/// The circuit project open in a project.
#[derive(Debug, Default)]
pub struct Circuits {
    open: Option<Open>,
    generations: u32,
}

impl Circuits {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![home_event(root), self.project_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        let runs = matches!(
            command,
            Command::CircuitEdit { .. } | Command::CircuitUndo | Command::CircuitRedo | Command::CircuitShow { .. } | Command::CircuitAdd { .. }
        );
        let mut events = self.apply_inner(root, command);
        // A circuit changed, or another shown, while running: built afresh.
        if runs && self.open.as_ref().is_some_and(|o| o.run.is_some()) {
            if let Some(open) = &mut self.open {
                let inputs = open.run.take().map(|r| r.inputs).unwrap_or_default();
                open.run = Some(start(open, inputs));
            }
            events.push(self.run_event());
        }
        events
    }

    fn apply_inner(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::CircuitNew { name } => self.new_project(root, &name),
            Command::CircuitOpen { path } => {
                let dir = match inside(root, &path) {
                    Ok(dir) => dir,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.path == dir) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open circuit's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match circ::read(&dir) {
                    Ok(project) => {
                        self.show(root, dir, project);
                        self.events(root)
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::CircuitClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the circuit has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.events(root)
            }
            Command::CircuitSave => {
                let Some(open) = &mut self.open else { return Vec::new() };
                match circ::write_project(&open.path, &open.project) {
                    Ok(()) => {
                        open.saved = open.project.clone();
                        self.events(root)
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::CircuitShow { circuit } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if !open.project.circuits.contains_key(&circuit) {
                    return vec![notice(format!("there is no circuit {circuit}"), NoticeLevel::Warn)];
                }
                open.shown = circuit;
                vec![self.project_event()]
            }
            Command::CircuitAdd { name } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                let name = name.trim().to_string();
                if !circ::is_name(&name) {
                    return vec![notice(format!("“{name}” cannot name a circuit: letters, digits, spaces, - and _"), NoticeLevel::Warn)];
                }
                if open.project.circuits.contains_key(&name) {
                    return vec![notice(format!("there is a circuit {name} already"), NoticeLevel::Warn)];
                }
                let mut after = open.project.clone();
                after.circuits.insert(name.clone(), Drawing::default());
                open.shown = name;
                record(open, after);
                vec![self.project_event()]
            }
            Command::CircuitSetTop { circuit } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if !open.project.circuits.contains_key(&circuit) {
                    return vec![notice(format!("there is no circuit {circuit}"), NoticeLevel::Warn)];
                }
                if open.project.top != circuit {
                    let mut after = open.project.clone();
                    after.top = circuit;
                    record(open, after);
                }
                vec![self.project_event()]
            }
            Command::CircuitEdit { generation, circuit, ops } => {
                let open = match self.checked(generation, &circuit) {
                    Ok(open) => open,
                    Err(events) => return events,
                };
                let mut drawing = open.project.circuits[&circuit].clone();
                let made = match apply_ops(&mut drawing, &circuit, &open.project.circuits, ops) {
                    Ok(made) => made,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn), self.project_event()],
                };
                if drawing != open.project.circuits[&circuit] {
                    let mut after = open.project.clone();
                    after.circuits.insert(circuit.clone(), drawing);
                    open.shown = circuit;
                    record(open, after);
                }
                vec![self.view_event(made)]
            }
            Command::CircuitUndo => self.step(true),
            Command::CircuitRedo => self.step(false),
            Command::CircuitStart { generation } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let inputs = open.run.take().map(|r| r.inputs).unwrap_or_default();
                open.run = Some(start(open, inputs));
                vec![self.run_event()]
            }
            Command::CircuitStop => {
                let Some(open) = &mut self.open else { return Vec::new() };
                match &mut open.run {
                    // A quick run stops; the circuit goes on running where it got to.
                    Some(run) if run.quick.is_some() => {
                        if let Some(q) = &run.quick {
                            q.stop.store(true, Ordering::Relaxed);
                        }
                        Vec::new()
                    }
                    _ => {
                        open.run = None;
                        vec![self.run_event()]
                    }
                }
            }
            Command::CircuitSet { generation, id, value, hold } => {
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                let Some((part, kind)) = run.parts.get(&id).cloned() else {
                    return vec![notice(format!("there is no part {id} to set"), NoticeLevel::Warn)];
                };
                if !SETTABLE.contains(&kind.as_str()) {
                    return vec![notice(format!("{id} is not an input"), NoticeLevel::Warn)];
                }
                let Some(v) = number(&value) else {
                    return vec![notice(format!("“{value}” is not a number"), NoticeLevel::Warn)];
                };
                run.inputs.insert(id, v);
                drive(run, &[(part, v)], hold);
                vec![self.run_event()]
            }
            Command::CircuitClock { generation, hold } => {
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                if run.clocks.is_empty() {
                    return vec![notice("this circuit has no clock".into(), NoticeLevel::Warn)];
                }
                let Some(sim) = &run.sim else { return Vec::new() };
                let flips: Vec<(PartId, u64)> = run.clocks.iter().map(|c| (*c, sim.output(*c, 0).get() ^ 1)).collect();
                for (id, (part, _)) in &run.parts {
                    if let Some((_, v)) = flips.iter().find(|(c, _)| c == part) {
                        run.inputs.insert(id.clone(), *v);
                    }
                }
                drive(run, &flips, hold);
                vec![self.run_event()]
            }
            Command::CircuitStep { generation } => {
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                if let Some(sim) = &mut run.sim {
                    match sim.step() {
                        Ok(names) => run.stepped = names,
                        Err(e) => {
                            run.error = Some(e.to_string());
                            run.sim = None;
                        }
                    }
                }
                vec![self.run_event()]
            }
            Command::CircuitQuick { generation } => {
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                if run.clocks.is_empty() {
                    return vec![notice("a quick run flips the clocks, and this circuit has none".into(), NoticeLevel::Warn)];
                }
                let Some(mut sim) = run.sim.take() else { return Vec::new() };
                if sim.waiting() > 0 {
                    let _ = sim.settle();
                }
                let stop = Arc::new(AtomicBool::new(false));
                let (tell, news) = mpsc::channel();
                let (clocks, breaks, halt) = (run.clocks.clone(), run.breaks.clone(), stop.clone());
                std::thread::spawn(move || quick_run(sim, clocks, breaks, halt, tell));
                run.quick = Some(Quick { stop, news, cycles: 0, started: Instant::now() });
                run.last_quick = None;
                vec![self.run_event()]
            }
            Command::CircuitSaveMemory { generation, id, name } => {
                let name = name.trim().trim_start_matches("data/").trim_end_matches(".hex").to_string();
                if !circ::is_name(&name) || name.contains(' ') {
                    return vec![notice(format!("“{name}” cannot name a data file: letters, digits, - and _"), NoticeLevel::Warn)];
                }
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                let Some((part, kind)) = run.parts.get(&id).cloned() else {
                    return vec![notice(format!("there is no part {id}"), NoticeLevel::Warn)];
                };
                if kind != "ram" && kind != "ram2" {
                    return vec![notice(format!("{id} is not a RAM"), NoticeLevel::Warn)];
                }
                let text = proton_circuit::hex::write(run.sim.as_ref().map(|s| s.memory(part)).unwrap_or_default());
                let Some(open) = &mut self.open else { return Vec::new() };
                let mut after = open.project.clone();
                after.data.insert(name.clone(), text);
                record(open, after);
                vec![notice(format!("{id}'s contents are kept as data/{name}.hex, saved with ⌘S"), NoticeLevel::Info), self.project_event()]
            }
            Command::CircuitCopy { generation, circuit, ids, wires } => {
                let open = match self.checked(generation, &circuit) {
                    Ok(open) => open,
                    Err(events) => return events,
                };
                let wires: Vec<Wire> = wires.iter().map(|w| Wire::new((w[0], w[1]), (w[2], w[3]))).collect();
                vec![Event::CircuitClip { text: edit::copy(&open.project.circuits[&circuit], &ids, &wires) }]
            }
            _ => Vec::new(),
        }
    }

    /// The project open, if the window's numbers are for it: its
    /// generation and a circuit it has.
    fn checked(&mut self, generation: u32, circuit: &str) -> Result<&mut Open, Vec<Event>> {
        let Some(open) = &mut self.open else {
            return Err(vec![notice("no circuit project is open".into(), NoticeLevel::Warn)]);
        };
        if generation != open.generation {
            return Err(vec![notice("that project is no longer open".into(), NoticeLevel::Warn)]);
        }
        if !open.project.circuits.contains_key(circuit) {
            return Err(vec![notice(format!("there is no circuit {circuit}"), NoticeLevel::Warn)]);
        }
        Ok(open)
    }

    /// The run of the project open, if the window's number is for it and
    /// a quick run does not have it.
    fn running(&mut self, generation: u32) -> Result<&mut Run, Vec<Event>> {
        let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else {
            return Err(vec![notice("that project is no longer open".into(), NoticeLevel::Warn)]);
        };
        let Some(run) = &mut open.run else {
            return Err(vec![notice("the circuit is not running".into(), NoticeLevel::Warn)]);
        };
        if run.quick.is_some() {
            return Err(vec![notice("a quick run is under way: ⎋ stops it".into(), NoticeLevel::Warn)]);
        }
        if run.sim.is_none() {
            return Err(vec![notice(run.error.clone().unwrap_or_else(|| "the circuit is not running".into()), NoticeLevel::Warn)]);
        }
        Ok(run)
    }

    /// News of a quick run: how far it has got, and its end.
    pub fn poll(&mut self) -> Vec<Event> {
        let Some(run) = self.open.as_mut().and_then(|o| o.run.as_mut()) else { return Vec::new() };
        let Some(q) = &mut run.quick else { return Vec::new() };
        let mut changed = false;
        let mut done = None;
        while let Ok(news) = q.news.try_recv() {
            changed = true;
            match news {
                QuickNews::Cycles(n) => q.cycles = n,
                QuickNews::Done { sim, cycles, ended } => done = Some((sim, cycles, ended)),
            }
        }
        if let Some((sim, cycles, ended)) = done {
            let secs = q.started.elapsed().as_secs_f64().max(1e-9);
            run.last_quick = Some(CircuitQuickView { running: false, cycles, hz: cycles as f64 / secs, ended: Some(ended) });
            run.quick = None;
            // The clocks are where the run left them.
            for (id, (part, kind)) in &run.parts {
                if kind == "clock" {
                    run.inputs.insert(id.clone(), sim.output(*part, 0).get());
                }
            }
            run.sim = Some(*sim);
        } else if !changed {
            return Vec::new();
        }
        vec![self.run_event()]
    }

    pub fn run_event(&self) -> Event {
        let Some(open) = &self.open else { return Event::CircuitRun { run: None } };
        let Some(run) = &open.run else { return Event::CircuitRun { run: None } };
        let quick = match &run.quick {
            Some(q) => Some(CircuitQuickView { running: true, cycles: q.cycles, hz: q.cycles as f64 / q.started.elapsed().as_secs_f64().max(1e-9), ended: None }),
            None => run.last_quick.clone(),
        };
        let (wires, values, waiting) = match &run.sim {
            Some(sim) => {
                let wires = run.wires.iter().map(|n| n.map(|n| CircuitValue::from(sim.value(n)))).collect();
                let mut values: Vec<(String, CircuitValue)> = run
                    .parts
                    .iter()
                    .filter_map(|(id, (part, kind))| {
                        if SETTABLE.contains(&kind.as_str()) {
                            Some((id.clone(), sim.output(*part, 0).into()))
                        } else if SHOWN.contains(&kind.as_str()) {
                            Some((id.clone(), sim.shown(*part).into()))
                        } else {
                            None
                        }
                    })
                    .collect();
                values.sort_by(|a, b| a.0.cmp(&b.0));
                (wires, values, sim.waiting() as u32)
            }
            None => (Vec::new(), Vec::new(), 0),
        };
        Event::CircuitRun {
            run: Some(Box::new(CircuitRunView {
                generation: open.generation,
                circuit: run.circuit.clone(),
                wires,
                values,
                waiting,
                stepped: run.stepped.clone(),
                error: run.error.clone(),
                quick,
            })),
        }
    }

    /// Undo, or redo: the project as the step found or left it.
    fn step(&mut self, back: bool) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let Some(step) = (if back { open.done.pop() } else { open.undone.pop() }) else { return Vec::new() };
        open.project = if back { step.before.clone() } else { step.after.clone() };
        if open.project.circuits.contains_key(&step.shown) {
            open.shown = step.shown.clone();
        } else if !open.project.circuits.contains_key(&open.shown) {
            open.shown = open.project.top.clone();
        }
        if back {
            open.undone.push(step);
        } else {
            open.done.push(step);
        }
        vec![self.project_event()]
    }

    fn new_project(&mut self, root: &Path, name: &str) -> Vec<Event> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return vec![notice("a new circuit needs a name".into(), NoticeLevel::Warn)];
        }
        if self.dirty() {
            return vec![notice("save or discard the open circuit's changes before making another".into(), NoticeLevel::Warn)];
        }
        let folder = format!("{}.{EXTENSION}", slug(&name));
        let dir = match inside(root, &folder) {
            Ok(dir) => dir,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if dir.exists() {
            return vec![notice(format!("{folder} is already there"), NoticeLevel::Warn)];
        }
        let project = Project::new();
        if let Err(why) = circ::write_project(&dir, &project) {
            return vec![notice(why, NoticeLevel::Warn)];
        }
        self.show(root, dir, project);
        let mut events = self.events(root);
        events.push(notice(format!("made {name} in {folder}"), NoticeLevel::Info));
        events
    }

    fn show(&mut self, root: &Path, path: PathBuf, project: Project) {
        self.generations += 1;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let rel = path.strip_prefix(root).or_else(|_| path.strip_prefix(&real));
        self.open = Some(Open {
            shown_path: rel.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string()),
            path,
            name,
            shown: project.top.clone(),
            saved: project.clone(),
            project,
            generation: self.generations,
            done: Vec::new(),
            undone: Vec::new(),
            run: None,
        });
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.project != o.saved)
    }

    /// The project with unsaved changes, as the folder names it.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(open) if open.project != open.saved => vec![open.shown_path.clone()],
            _ => Vec::new(),
        }
    }

    /// Save it, as quitting does; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if open.project == open.saved {
            return Vec::new();
        }
        match circ::write_project(&open.path, &open.project) {
            Ok(()) => {
                open.saved = open.project.clone();
                Vec::new()
            }
            Err(why) => vec![format!("{}: {why}", open.shown_path)],
        }
    }

    pub fn project_event(&self) -> Event {
        self.view_event(Vec::new())
    }

    fn view_event(&self, made: Vec<String>) -> Event {
        let Some(open) = &self.open else { return Event::CircuitProject { project: None } };
        let p = &open.project;
        let drawing = p.circuits.get(&open.shown).cloned().unwrap_or_default();
        let mut pins: std::collections::HashMap<String, Result<Vec<circ::PinAt>, String>> = p.pins(&open.shown).into_iter().collect();
        let parts = drawing
            .parts
            .iter()
            .map(|part| {
                let placed = pins.remove(&part.id).unwrap_or_else(|| Ok(Vec::new()));
                let (pins, problem) = match placed {
                    Ok(pins) => (pins.into_iter().map(|p| CircuitPin { x: p.x, y: p.y, input: p.input }).collect(), None),
                    Err(why) => (Vec::new(), Some(why)),
                };
                CircuitPartView {
                    id: part.id.clone(),
                    kind: part.kind.clone(),
                    x: part.x,
                    y: part.y,
                    rot: part.rot,
                    mirror: part.mirror,
                    settings: part.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    pins,
                    problem,
                }
            })
            .collect();
        Event::CircuitProject {
            project: Some(Box::new(CircuitProjectView {
                path: open.shown_path.clone(),
                name: open.name.clone(),
                top: p.top.clone(),
                circuits: p.circuits.keys().cloned().collect(),
                shown: open.shown.clone(),
                parts,
                wires: drawing.wires.iter().map(|w| [w.a.0, w.a.1, w.b.0, w.b.1]).collect(),
                tests: p.tests.get(&open.shown).map(|t| t.keys().cloned().collect()).unwrap_or_default(),
                data: p.data.keys().cloned().collect(),
                generation: open.generation,
                dirty: open.project != open.saved,
                can_undo: !open.done.is_empty(),
                can_redo: !open.undone.is_empty(),
                made,
                palette: circ::kinds()
                    .map(str::to_string)
                    .chain(p.circuits.keys().map(|n| format!("sub:{n}")).filter(|k| {
                        let probe = Part { id: "P".into(), kind: k.clone(), x: 0, y: 0, rot: 0, mirror: false, attrs: Default::default() };
                        edit::check_part(&probe, &open.shown, &p.circuits).is_ok()
                    }))
                    .collect(),
            })),
        }
    }
}

/// The circuit shown, built and powered on, its inputs set as they were.
fn start(open: &Open, inputs: BTreeMap<String, u64>) -> Run {
    let circuit = open.shown.clone();
    let mut run = Run {
        circuit: circuit.clone(),
        sim: None,
        error: None,
        parts: HashMap::new(),
        wires: Vec::new(),
        clocks: Vec::new(),
        breaks: Vec::new(),
        inputs: BTreeMap::new(),
        stepped: Vec::new(),
        quick: None,
        last_quick: None,
    };
    let built = match open.project.build(&circuit) {
        Ok(b) => b,
        Err(why) => {
            run.error = Some(why);
            return run;
        }
    };
    let drawing = &open.project.circuits[&circuit];
    for (part, made) in circ::ordered(drawing).into_iter().zip(&built.top_parts) {
        if let Some(id) = made {
            run.parts.insert(part.id.clone(), (*id, part.kind.clone()));
            if part.kind == "clock" {
                run.clocks.push(*id);
            }
        }
    }
    run.wires = built.top_wires.clone();
    run.breaks = built.breaks.clone();
    match Sim::new(built.netlist, 0x5eed) {
        Ok(mut sim) => {
            for (id, v) in inputs {
                if let Some((part, kind)) = run.parts.get(&id) {
                    if SETTABLE.contains(&kind.as_str()) {
                        if let Err(e) = sim.set(*part, v) {
                            run.error = Some(e.to_string());
                            return run;
                        }
                        run.inputs.insert(id, v);
                    }
                }
            }
            run.sim = Some(sim);
        }
        Err(e) => run.error = Some(e.to_string()),
    }
    run
}

/// Inputs set, settled — or, `hold`, waiting for gate steps.
fn drive(run: &mut Run, sets: &[(PartId, u64)], hold: bool) {
    let Some(sim) = &mut run.sim else { return };
    run.stepped.clear();
    for (part, v) in sets {
        if hold {
            sim.hold(*part, *v);
        } else if let Err(e) = sim.set(*part, *v) {
            run.error = Some(e.to_string());
            run.sim = None;
            return;
        }
    }
}

/// A number as typed: decimal, `0x…`, `0b…`, or `0o…`.
fn number(s: &str) -> Option<u64> {
    let s = s.trim().replace('_', "");
    let (digits, radix) = match s.get(..2) {
        Some("0x" | "0X") => (&s[2..], 16),
        Some("0b" | "0B") => (&s[2..], 2),
        Some("0o" | "0O") => (&s[2..], 8),
        _ => (s.as_str(), 10),
    };
    u64::from_str_radix(digits, radix).ok()
}

/// The clocks flipped as fast as they go until a Break rises, the run is
/// stopped, or something goes wrong; its news sent as it goes.
fn quick_run(mut sim: Sim, clocks: Vec<PartId>, breaks: Vec<PartId>, stop: Arc<AtomicBool>, tell: mpsc::Sender<QuickNews>) {
    let mut cycles = 0u64;
    let mut told = Instant::now();
    let mut was: Vec<bool> = breaks.iter().map(|b| sim.shown(*b).get() == 1).collect();
    let ended = 'run: loop {
        if stop.load(Ordering::Relaxed) {
            break "stopped".to_string();
        }
        for _ in 0..2 {
            for c in &clocks {
                let v = sim.output(*c, 0).get() ^ 1;
                if let Err(e) = sim.set(*c, v) {
                    break 'run e.to_string();
                }
            }
            for (i, b) in breaks.iter().enumerate() {
                let now = sim.shown(*b).get() == 1;
                if now && !was[i] {
                    cycles += 1;
                    break 'run "a Break rose".to_string();
                }
                was[i] = now;
            }
        }
        cycles += 1;
        if told.elapsed() >= Duration::from_millis(100) {
            told = Instant::now();
            if tell.send(QuickNews::Cycles(cycles)).is_err() {
                break "the project closed".to_string();
            }
        }
    };
    let _ = tell.send(QuickNews::Done { sim: Box::new(sim), cycles, ended });
}

/// A change made a step: the project as it was kept to undo to.
fn record(open: &mut Open, after: Project) {
    let before = std::mem::replace(&mut open.project, after.clone());
    open.done.push(Step { before, after, shown: open.shown.clone() });
    if open.done.len() > MOST_STEPS {
        open.done.remove(0);
    }
    open.undone.clear();
}

/// The ops applied to a drawing, all or none: each part they touch is
/// checked against the project. The ids of the parts made.
fn apply_ops(d: &mut Drawing, circuit: &str, circuits: &std::collections::BTreeMap<String, Drawing>, ops: Vec<CircuitOp>) -> Result<Vec<String>, String> {
    let mut made = Vec::new();
    let mut touched: Vec<String> = Vec::new();
    let wire = |w: &[i32; 4]| Wire::new((w[0], w[1]), (w[2], w[3]));
    let find = |d: &mut Drawing, id: &str| -> Result<usize, String> { d.parts.iter().position(|p| p.id == id).ok_or_else(|| format!("there is no part {id}")) };
    for op in ops {
        match op {
            CircuitOp::Place { kind, x, y, rot, mirror, settings } => {
                let id = edit::fresh_id(d, edit::prefix(&kind));
                let mut attrs: std::collections::BTreeMap<String, String> = settings.into_iter().filter(|(_, v)| !v.is_empty()).collect();
                // An input or output is named for tests and for the circuits that place this one.
                if matches!(kind.as_str(), "in" | "out" | "clock") && !attrs.contains_key("label") {
                    attrs.insert("label".into(), id.clone());
                }
                d.parts.push(Part { id: id.clone(), kind, x, y, rot: rot % 4, mirror, attrs });
                touched.push(id.clone());
                made.push(id);
            }
            CircuitOp::Move { ids, wires, dx, dy } => {
                for id in &ids {
                    let i = find(d, id)?;
                    d.parts[i].x += dx;
                    d.parts[i].y += dy;
                    touched.push(id.clone());
                }
                let moving: Vec<Wire> = wires.iter().map(wire).collect();
                let kept: Vec<Wire> = d.wires.iter().copied().filter(|w| !moving.contains(w)).collect();
                let moved: Vec<Wire> = d.wires.iter().copied().filter(|w| moving.contains(w)).collect();
                d.wires = kept;
                for w in moved {
                    edit::add_wire(d, (w.a.0 + dx, w.a.1 + dy), (w.b.0 + dx, w.b.1 + dy));
                }
            }
            CircuitOp::Rotate { ids } => {
                for id in ids {
                    let i = find(d, &id)?;
                    edit::rotate(&mut d.parts[i]);
                }
            }
            CircuitOp::Mirror { ids } => {
                for id in ids {
                    let i = find(d, &id)?;
                    d.parts[i].mirror = !d.parts[i].mirror;
                }
            }
            CircuitOp::Delete { ids, wires } => {
                d.parts.retain(|p| !ids.contains(&p.id));
                let gone: Vec<Wire> = wires.iter().map(wire).collect();
                d.wires.retain(|w| !gone.contains(w));
            }
            CircuitOp::Wire { points } => {
                if points.len() < 2 {
                    return Err("a wire runs between two points at least".into());
                }
                for leg in points.windows(2) {
                    let (a, b) = ((leg[0][0], leg[0][1]), (leg[1][0], leg[1][1]));
                    if a != b && a.0 != b.0 && a.1 != b.1 {
                        return Err("a wire runs straight across or down".into());
                    }
                    edit::add_wire(d, a, b);
                }
            }
            CircuitOp::Set { id, new_id, settings } => {
                let i = find(d, &id)?;
                let new_id = new_id.map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && *n != id);
                if let Some(n) = &new_id {
                    if d.parts.iter().any(|p| p.id == *n) {
                        return Err(format!("there is a part {n} already"));
                    }
                }
                let p = &mut d.parts[i];
                p.attrs = settings.into_iter().map(|(k, v)| (k.trim().to_string(), v)).filter(|(k, v)| !k.is_empty() && !v.is_empty()).collect();
                if let Some(n) = new_id {
                    p.id = n;
                }
                touched.push(p.id.clone());
            }
            CircuitOp::Paste { text, dx, dy } => {
                let ids = edit::paste(d, &text, dx, dy)?;
                touched.extend(ids.iter().cloned());
                made.extend(ids);
            }
        }
    }
    for id in &touched {
        if let Some(p) = d.parts.iter().find(|p| p.id == *id) {
            edit::check_part(p, circuit, circuits)?;
        }
    }
    // Held as it is written, so a step back compares equal to what was saved.
    *d = circ::parse(&circ::write(d)).map_err(|e| format!("the circuit would not read back: {e}"))?;
    Ok(made)
}

/// A `.circuit` folder named relative to the project, refused if it is not
/// one or leads outside.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if named.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
        return Err(format!("{rel} is not a circuit: a circuit is a .{EXTENSION} folder"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(path)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "circuit".into()
    } else {
        out
    }
}

pub fn home_event(root: &Path) -> Event {
    Event::CircuitHome { projects: find(root) }
}

/// The circuit projects in a folder, three folders deep.
fn find(root: &Path) -> Vec<CircuitFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) || !path.is_dir() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push(CircuitFound { path: rel, name: name.trim_end_matches(&format!(".{EXTENSION}")).to_string() });
            } else if depth < 3 {
                stack.push((path, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-circuit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn view(events: &[Event]) -> Option<CircuitProjectView> {
        events.iter().rev().find_map(|e| match e {
            Event::CircuitProject { project } => project.as_deref().cloned(),
            _ => None,
        })
    }

    #[test]
    fn a_project_is_made_found_drawn_held_and_saved() {
        let root = scratch("made");
        let mut c = Circuits::default();
        let made = c.apply(&root, Command::CircuitNew { name: "Full Adder".into() });
        let v = view(&made).unwrap();
        assert_eq!((v.path.as_str(), v.top.as_str(), v.dirty), ("full-adder.circuit", "main", false));
        assert!(root.join("full-adder.circuit/project.json").exists());
        let found = match &c.events(&root)[0] {
            Event::CircuitHome { projects } => projects.clone(),
            _ => unreachable!(),
        };
        assert_eq!(found, vec![CircuitFound { path: "full-adder.circuit".into(), name: "full-adder".into() }]);

        // A circuit with parts, drawn with its pins placed.
        std::fs::write(root.join("full-adder.circuit/circuits/main.circ"), "part A in at 0,0 label=A\npart U and at 2,0\npart Z frob at 9,9\nwire 0,0 2,0\n").unwrap();
        c.apply(&root, Command::CircuitClose { discard: false });
        let v = view(&c.apply(&root, Command::CircuitOpen { path: "full-adder.circuit".into() })).unwrap();
        let and = v.parts.iter().find(|p| p.id == "U").unwrap();
        assert_eq!(and.pins, vec![CircuitPin { x: 2, y: 0, input: true }, CircuitPin { x: 2, y: 2, input: true }, CircuitPin { x: 5, y: 1, input: false }]);
        assert!(v.parts.iter().find(|p| p.id == "Z").unwrap().problem.as_deref().unwrap().contains("frob"));
        assert_eq!(v.wires, vec![[0, 0, 2, 0]]);

        // A circuit added is held until ⌘S.
        let v = view(&c.apply(&root, Command::CircuitAdd { name: "half".into() })).unwrap();
        assert!(v.dirty && v.shown == "half");
        assert_eq!(c.unsaved(), vec!["full-adder.circuit".to_string()]);
        assert!(!root.join("full-adder.circuit/circuits/half.circ").exists());
        assert!(c.apply(&root, Command::CircuitClose { discard: false }).iter().any(|e| matches!(e, Event::Notice { .. })));
        let v = view(&c.apply(&root, Command::CircuitSave)).unwrap();
        assert!(!v.dirty);
        assert!(root.join("full-adder.circuit/circuits/half.circ").exists());
        assert!(c.unsaved().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn edits_are_steps_that_undo_back_to_saved_and_copy_and_paste() {
        let root = scratch("edits");
        let mut c = Circuits::default();
        let v = view(&c.apply(&root, Command::CircuitNew { name: "lab".into() })).unwrap();
        let (g, main) = (v.generation, "main".to_string());
        let ed = |c: &mut Circuits, ops: Vec<CircuitOp>| view(&c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops })).unwrap();
        let place = |kind: &str, x: i32, y: i32| CircuitOp::Place { kind: kind.into(), x, y, rot: 0, mirror: false, settings: vec![] };

        // Two inputs and an AND, placed as one step: ids from their kinds, inputs labelled.
        let v = ed(&mut c, vec![place("in", 0, 0), place("in", 0, 2), place("and", 3, 0)]);
        assert_eq!(v.made, vec!["In1", "In2", "U1"]);
        assert_eq!(v.parts.iter().find(|p| p.id == "In1").unwrap().settings, vec![("label".to_string(), "In1".to_string())]);
        assert!(v.dirty && v.can_undo && !v.can_redo);
        // Wired, with a leg that bends.
        let v = ed(&mut c, vec![CircuitOp::Wire { points: vec![[0, 0], [3, 0]] }, CircuitOp::Wire { points: vec![[0, 2], [1, 2], [1, 2], [3, 2]] }]);
        assert_eq!(v.wires, vec![[0, 0, 3, 0], [0, 2, 1, 2], [1, 2, 3, 2]]);
        // Moved, turned, renamed and its settings set.
        let v = ed(&mut c, vec![
            CircuitOp::Move { ids: vec!["U1".into()], wires: vec![], dx: 1, dy: 0 },
            CircuitOp::Rotate { ids: vec!["U1".into()] },
            CircuitOp::Set { id: "U1".into(), new_id: Some("Gate".into()), settings: vec![("inputs".into(), "3".into()), ("bits".into(), "".into())] },
        ]);
        let gate = v.parts.iter().find(|p| p.id == "Gate").unwrap();
        assert_eq!((gate.x, gate.rot, gate.settings.clone()), (4, 1, vec![("inputs".to_string(), "3".to_string())]));
        assert_eq!(gate.pins.len(), 4, "three inputs and the output, placed by the core");

        // An edit that cannot be is refused whole.
        let events = c.apply(&root, Command::CircuitEdit { generation: g, circuit: main.clone(), ops: vec![place("and", 9, 9), place("frob", 0, 0)] });
        assert!(events.iter().any(|e| matches!(e, Event::Notice { .. })));
        assert_eq!(view(&events).unwrap().parts.len(), 3, "nothing of a refused edit is kept");
        let stale = c.apply(&root, Command::CircuitEdit { generation: g + 1, circuit: main.clone(), ops: vec![place("and", 9, 9)] });
        assert!(stale.iter().any(|e| matches!(e, Event::Notice { .. })) && view(&stale).is_none());

        // Copy and paste: a fresh id where the old is taken.
        let clip = c.apply(&root, Command::CircuitCopy { generation: g, circuit: main.clone(), ids: vec!["Gate".into(), "In1".into()], wires: vec![[0, 0, 3, 0]] });
        let text = match &clip[0] {
            Event::CircuitClip { text } => text.clone(),
            _ => unreachable!(),
        };
        assert_eq!(text, "part Gate and at 4,0 rot 1 inputs=3\npart In1 in at 0,0 label=In1\nwire 0,0 3,0\n");
        let v = ed(&mut c, vec![CircuitOp::Paste { text, dx: 0, dy: 10 }]);
        assert_eq!(v.made, vec!["U1", "In3"]);
        assert!(v.wires.contains(&[0, 10, 3, 10]));
        let v = ed(&mut c, vec![CircuitOp::Delete { ids: vec!["In3".into(), "U1".into()], wires: vec![[0, 10, 3, 10]] }]);
        assert_eq!(v.parts.len(), 3);

        // Saved, then undone and redone: back at saved is clean again.
        c.apply(&root, Command::CircuitSave);
        let v = view(&c.apply(&root, Command::CircuitUndo)).unwrap();
        assert!(v.dirty && v.can_redo && v.parts.len() == 5);
        let v = view(&c.apply(&root, Command::CircuitRedo)).unwrap();
        assert!(!v.dirty, "redone back to what was saved");
        for _ in 0..10 {
            c.apply(&root, Command::CircuitUndo);
        }
        let v = view(&c.events(&root)).unwrap();
        assert!(v.parts.is_empty() && !v.can_undo);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_circuit_drawn_by_edits_runs_its_table() {
        let root = scratch("runs");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "inv".into() })).unwrap().generation;
        let place = |kind: &str, x: i32, y: i32| CircuitOp::Place { kind: kind.into(), x, y, rot: 0, mirror: false, settings: vec![] };
        // An input, a NOT and an output, wired by dragging from pin to pin.
        c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops: vec![place("in", 0, 0), place("not", 3, 0), place("out", 8, 0)] });
        let v = view(&c.apply(&root, Command::CircuitEdit {
            generation: g,
            circuit: "main".into(),
            ops: vec![CircuitOp::Wire { points: vec![[0, 0], [3, 0]] }, CircuitOp::Wire { points: vec![[5, 0], [8, 0]] }],
        }))
        .unwrap();
        assert_eq!(v.wires.len(), 2);
        let open = c.open.as_ref().unwrap();
        let mut project = open.project.clone();
        project.tests.entry("main".into()).or_default().insert("t".into(), "In1 Out1\n0 1\n1 0\n".into());
        let outcomes = proton_circuit::testlang::run_all(&project.build("main").unwrap());
        assert!(outcomes[0].passed(), "{:?} {:?}", outcomes[0].failures, outcomes[0].error);
        let _ = std::fs::remove_dir_all(root);
    }

    fn run_of(events: &[Event]) -> Option<CircuitRunView> {
        events.iter().rev().find_map(|e| match e {
            Event::CircuitRun { run } => run.as_deref().cloned(),
            _ => None,
        })
    }

    #[test]
    fn a_circuit_runs_its_inputs_clicked_stepped_and_rebuilt_when_edited() {
        let root = scratch("run");
        std::fs::create_dir_all(root.join("inv.circuit/circuits")).unwrap();
        std::fs::write(root.join("inv.circuit/project.json"), "{\n  \"version\": 1,\n  \"top\": \"main\"\n}\n").unwrap();
        // An input through a NOT to an output, and a clock to an LED.
        std::fs::write(
            root.join("inv.circuit/circuits/main.circ"),
            "part A in at 0,0 label=A\npart Clk clock at 0,4 label=Clk\npart L led at 3,4\npart N not at 2,0\npart Y out at 6,0 label=Y\nwire 0,0 2,0\nwire 4,0 6,0\nwire 0,4 3,4\n",
        )
        .unwrap();
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitOpen { path: "inv.circuit".into() })).unwrap().generation;
        let value = |r: &CircuitRunView, id: &str| r.values.iter().find(|(k, _)| k == id).map(|(_, v)| v.hex.clone()).unwrap();
        let r = run_of(&c.apply(&root, Command::CircuitStart { generation: g })).unwrap();
        assert!(r.error.is_none());
        assert_eq!((value(&r, "A").as_str(), value(&r, "Y").as_str()), ("0", "1"));
        assert_eq!(r.wires.len(), 3);
        assert_eq!(r.wires[2].as_ref().unwrap().hex, "1", "the NOT's output wire, third as wires are held sorted, is high");

        let r = run_of(&c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: "1".into(), hold: false })).unwrap();
        assert_eq!(value(&r, "Y"), "0");
        // Held: the change waits, and gate steps take it through.
        let r = run_of(&c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: "0".into(), hold: true })).unwrap();
        assert!(r.waiting > 0 && value(&r, "Y") == "0");
        let mut steps = 0;
        let mut r = r;
        while r.waiting > 0 {
            r = run_of(&c.apply(&root, Command::CircuitStep { generation: g })).unwrap();
            steps += 1;
            assert!(steps < 10);
        }
        assert_eq!((value(&r, "Y").as_str(), steps), ("1", 2), "the input's delay, then the NOT's");
        // A clock edge flips the clock.
        let r = run_of(&c.apply(&root, Command::CircuitClock { generation: g, hold: false })).unwrap();
        assert_eq!((value(&r, "Clk").as_str(), value(&r, "L").as_str()), ("1", "1"));
        // Not an input, not a number: refused.
        assert!(c.apply(&root, Command::CircuitSet { generation: g, id: "Y".into(), value: "1".into(), hold: false }).iter().any(|e| matches!(e, Event::Notice { .. })));
        assert!(c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: "lots".into(), hold: false }).iter().any(|e| matches!(e, Event::Notice { .. })));
        // An edit while running builds it afresh, inputs as they were.
        c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: "1".into(), hold: false });
        let events = c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops: vec![CircuitOp::Move { ids: vec!["Y".into()], wires: vec![], dx: 0, dy: 0 }, CircuitOp::Place { kind: "and".into(), x: 10, y: 10, rot: 0, mirror: false, settings: vec![] }] });
        let r = run_of(&events).unwrap();
        assert_eq!((value(&r, "A").as_str(), value(&r, "Y").as_str(), value(&r, "Clk").as_str()), ("1", "0", "1"));
        let r = c.apply(&root, Command::CircuitStop);
        assert!(matches!(&r[0], Event::CircuitRun { run: None }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_quick_run_flips_the_clocks_until_a_break_rises() {
        let root = scratch("quick");
        std::fs::create_dir_all(root.join("count.circuit/circuits")).unwrap();
        std::fs::write(root.join("count.circuit/project.json"), "{\n  \"version\": 1,\n  \"top\": \"main\"\n}\n").unwrap();
        // Two T flip-flops counting, the second's Q to a Break.
        std::fs::write(
            root.join("count.circuit/circuits/main.circ"),
            "part C clock at 0,1 label=C\npart F1 tff at 2,0\npart F2 tff at 8,0\npart K1 const at 1,0\npart K2 const at 7,0\npart X break at 13,0\n\
             wire 1,0 2,0\nwire 0,1 2,1\nwire 5,0 6,0\nwire 6,0 6,1\nwire 6,1 8,1\nwire 7,0 8,0\nwire 11,0 13,0\n",
        )
        .unwrap();
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitOpen { path: "count.circuit".into() })).unwrap().generation;
        let r = run_of(&c.apply(&root, Command::CircuitStart { generation: g })).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        let r = run_of(&c.apply(&root, Command::CircuitQuick { generation: g })).unwrap();
        assert!(r.quick.as_ref().unwrap().running && r.wires.is_empty());
        assert!(c.apply(&root, Command::CircuitClock { generation: g, hold: false }).iter().any(|e| matches!(e, Event::Notice { .. })), "a quick run has the simulator");
        let deadline = Instant::now() + Duration::from_secs(10);
        let ended = loop {
            if let Some(r) = run_of(&c.poll()) {
                if let Some(q) = r.quick.filter(|q| !q.running) {
                    break q;
                }
            }
            assert!(Instant::now() < deadline, "the quick run never ended");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(ended.ended.as_deref(), Some("a Break rose"));
        assert!(ended.cycles >= 1 && ended.cycles <= 4, "{} cycles", ended.cycles);
        // The circuit runs on from where the quick run left it.
        let r = run_of(&c.apply(&root, Command::CircuitClock { generation: g, hold: false })).unwrap();
        assert!(r.error.is_none() && !r.wires.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_ram_written_while_running_is_kept_as_a_data_file() {
        let root = scratch("ram");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "mem".into() })).unwrap().generation;
        let text = "part Ad in at 0,0 bits=2 label=Ad\npart Di in at 0,1 bits=8 label=Di\npart St in at 0,2 label=St\npart Clk clock at 0,3 label=Clk\n\
                    part K const at 1,4\npart M ram at 2,0 addr=2 bits=8\npart D out at 7,0 bits=8 label=D\n\
                    wire 0,0 2,0\nwire 0,1 2,1\nwire 0,2 2,2\nwire 0,3 2,3\nwire 1,4 2,4\nwire 5,0 7,0\n";
        c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops: vec![CircuitOp::Paste { text: text.into(), dx: 0, dy: 0 }] });
        c.apply(&root, Command::CircuitStart { generation: g });
        for (id, v) in [("Ad", "1"), ("Di", "0x5a"), ("St", "1")] {
            c.apply(&root, Command::CircuitSet { generation: g, id: id.into(), value: v.into(), hold: false });
        }
        let r = run_of(&c.apply(&root, Command::CircuitClock { generation: g, hold: false })).unwrap();
        assert_eq!(r.values.iter().find(|(k, _)| k == "D").unwrap().1.hex, "5a", "written, and read back as ld is high");
        assert!(c.apply(&root, Command::CircuitSaveMemory { generation: g, id: "D".into(), name: "x".into() }).iter().any(|e| matches!(e, Event::Notice { .. })));
        let events = c.apply(&root, Command::CircuitSaveMemory { generation: g, id: "M".into(), name: "boot".into() });
        let v = view(&events).unwrap();
        assert!(v.dirty && v.data == vec!["boot".to_string()]);
        c.apply(&root, Command::CircuitSave);
        assert_eq!(std::fs::read_to_string(root.join("mem.circuit/data/boot.hex")).unwrap(), "v2.0 raw\n0 5a\n");
        // A ROM reads it by name.
        let v = view(&c.apply(&root, Command::CircuitEdit {
            generation: g,
            circuit: "main".into(),
            ops: vec![CircuitOp::Place { kind: "rom".into(), x: 20, y: 0, rot: 0, mirror: false, settings: vec![("addr".into(), "2".into()), ("bits".into(), "8".into()), ("file".into(), "boot".into())] }],
        }))
        .unwrap();
        assert!(v.parts.iter().find(|p| p.kind == "rom").unwrap().problem.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn folders_that_are_not_circuits_or_lead_outside_are_refused() {
        let root = scratch("outside");
        let mut c = Circuits::default();
        for bad in ["../x.circuit", "notes.txt", "/tmp/a.circuit", ""] {
            let events = c.apply(&root, Command::CircuitOpen { path: bad.into() });
            assert!(events.iter().any(|e| matches!(e, Event::Notice { .. })), "{bad}");
        }
        assert!(!c.is_open());
        let _ = std::fs::remove_dir_all(root);
    }
}
