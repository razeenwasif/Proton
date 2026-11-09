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
//!
//! A run records its inputs' and displays' values after each click, clock
//! edge and gate step, for the window's timing graph and for Data. Test
//! tables are the project's, edited as any other part of it, and run
//! against the circuit built afresh.

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
    /// `circuit` for a `.circuit` folder; `dig` for a Digital file, which is
    /// imported when it is opened.
    pub kind: String,
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
    /// The shown circuit's test tables: each label and its text.
    pub tables: Vec<(String, String)>,
    /// The project's state machines: each name and its text.
    pub machines: Vec<(String, String)>,
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

/// A test table run: how many rows, and those that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitTestResult {
    pub label: String,
    pub rows: u32,
    /// The first rows that failed: the line, and what came instead.
    pub failures: Vec<String>,
    /// Why it could not run at all.
    pub error: Option<String>,
}

/// A truth table and its minimised forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitAnalysisView {
    /// What it is of: a circuit's name, or the expressions.
    pub source: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// Each output's column: `0`, `1` or `x` a row, the first input the
    /// highest bit of the row's number.
    pub columns: Vec<String>,
    /// Each output's least sum of products, and product of sums.
    pub sop: Vec<String>,
    pub pos: Vec<String>,
    /// Each output's sum-of-products terms as (value, care) bit masks, for
    /// a Karnaugh map's groups.
    pub groups: Vec<Vec<[u32; 2]>>,
    /// Whether every cover is known to be the least.
    pub exact: bool,
    pub error: Option<String>,
}

/// A state machine as read: for its diagram, and its state table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitMachineView {
    /// The project's name for it, if it is one of the project's.
    pub name: Option<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub states: Vec<String>,
    /// Each state's outputs as written: `Z=1`.
    pub state_outputs: Vec<String>,
    /// Each transition: from, to, its condition, its outputs.
    pub transitions: Vec<(u32, u32, String, String)>,
    pub mealy: bool,
    /// Bits to number the states.
    pub bits: u32,
    /// The state table: state, inputs as bits, next state, outputs as bits.
    pub table: Vec<(u32, String, u32, String)>,
    pub error: Option<String>,
}

/// What a run has recorded: the signals, and their values at each sample,
/// newest last; none for high-Z.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitTraceView {
    /// Each signal's id and width.
    pub signals: Vec<(String, u8)>,
    /// The last samples, at most `SHOWN_SAMPLES`, each a value a signal.
    pub samples: Vec<Vec<Option<String>>>,
    /// How many have been taken since it was cleared.
    pub total: u32,
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
    pub trace: CircuitTraceView,
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
    /// The signals recorded, id order, and the samples: (value, high-Z).
    signals: Vec<(String, PartId, bool, u8)>,
    samples: std::collections::VecDeque<Vec<Option<u64>>>,
    taken: u32,
}

/// No more samples than this are kept, nor sent for the graph.
const MOST_SAMPLES: usize = 10_000;
const SHOWN_SAMPLES: usize = 400;

impl Run {
    /// The signals' values now, as one more sample.
    fn sample(&mut self) {
        let Some(sim) = &self.sim else { return };
        let row: Vec<Option<u64>> = self
            .signals
            .iter()
            .map(|(_, part, input, _)| {
                let v = if *input { sim.output(*part, 0) } else { sim.shown(*part) };
                (!v.is_z()).then(|| v.get())
            })
            .collect();
        // A sample like the last is kept too: a clock edge that changed
        // nothing still marks time on the graph.
        self.samples.push_back(row);
        if self.samples.len() > MOST_SAMPLES {
            self.samples.pop_front();
        }
        self.taken += 1;
    }
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
    /// What the window has selected, for `@circuit`: the circuit, its parts and wires.
    selected: (String, Vec<String>, Vec<[i32; 4]>),
    /// The last tables run, and of which circuit.
    last_tests: (String, Vec<CircuitTestResult>),
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
    /// What the agent's last call changed on screen, for the window.
    pending: Vec<Event>,
}

/// The tools the agent has for a circuit project.
pub const TOOLS: [&str; 7] = ["circuit_info", "circuit_open", "circuit_read", "circuit_edit", "circuit_test", "circuit_analyse", "circuit_synthesise"];

/// No more lines of a circuit than this go to the model at once.
const MOST_LINES: usize = 400;

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
            Command::CircuitEdit { .. }
                | Command::CircuitUndo
                | Command::CircuitRedo
                | Command::CircuitShow { .. }
                | Command::CircuitAdd { .. }
                | Command::CircuitSynthesise { .. }
                | Command::CircuitMachineBuild { .. }
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
                        Ok(names) => {
                            run.stepped = names;
                            run.sample();
                        }
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
            Command::CircuitTestSet { generation, circuit, label, text } => {
                let open = match self.checked(generation, &circuit) {
                    Ok(open) => open,
                    Err(events) => return events,
                };
                let label = label.trim().to_string();
                if !circ::is_name(&label) || label.contains(' ') {
                    return vec![notice(format!("“{label}” cannot name a test table: letters, digits, - and _"), NoticeLevel::Warn)];
                }
                if let Some(t) = &text {
                    if let Err(why) = proton_circuit::testlang::check(t) {
                        return vec![notice(format!("{label}: {why}"), NoticeLevel::Warn)];
                    }
                }
                let mut after = open.project.clone();
                let tables = after.tests.entry(circuit.clone()).or_default();
                match text {
                    Some(t) => {
                        let t = if t.ends_with('\n') { t } else { format!("{t}\n") };
                        tables.insert(label, t);
                    }
                    None => {
                        tables.remove(&label);
                    }
                }
                after.tests.retain(|_, t| !t.is_empty());
                if after != open.project {
                    record(open, after);
                }
                vec![self.project_event()]
            }
            Command::CircuitTestRun { generation, circuit, label } => {
                let open = match self.checked(generation, &circuit) {
                    Ok(open) => open,
                    Err(events) => return events,
                };
                let tables: Vec<(String, String)> = open
                    .project
                    .tests
                    .get(&circuit)
                    .map(|t| t.iter().filter(|(l, _)| label.as_ref().is_none_or(|want| want == *l)).map(|(l, t)| (l.clone(), t.clone())).collect())
                    .unwrap_or_default();
                if tables.is_empty() {
                    return vec![notice(format!("{circuit} has no test table{}", label.map(|l| format!(" {l}")).unwrap_or_default()), NoticeLevel::Warn)];
                }
                let results: Vec<CircuitTestResult> = match open.project.build(&circuit) {
                    Ok(built) => tables
                        .iter()
                        .map(|(l, t)| {
                            let o = proton_circuit::testlang::run(&built, l, t);
                            CircuitTestResult { label: o.label, rows: o.rows as u32, failures: o.failures, error: o.error }
                        })
                        .collect(),
                    Err(why) => tables.iter().map(|(l, _)| CircuitTestResult { label: l.clone(), rows: 0, failures: Vec::new(), error: Some(why.clone()) }).collect(),
                };
                open.last_tests = (circuit.clone(), results.clone());
                vec![Event::CircuitTests { generation, circuit, results }]
            }
            Command::CircuitExport { generation, circuit, format } => {
                let open = match self.checked(generation, &circuit) {
                    Ok(open) => open,
                    Err(events) => return events,
                };
                let module = format!("{}_{circuit}", open.name);
                let (text, ext) = match format.as_str() {
                    "verilog" | "vhdl" => {
                        let built = match open.project.build(&circuit) {
                            Ok(b) => b,
                            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                        };
                        if format == "verilog" {
                            (proton_circuit::hdl::verilog(&built, &module), "v")
                        } else {
                            (proton_circuit::hdl::vhdl(&built, &module), "vhd")
                        }
                    }
                    "svg" => match proton_circuit::svg::svg(&open.project, &circuit) {
                        Ok(t) => (t, "svg"),
                        Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                    },
                    other => return vec![notice(format!("“{other}” is not a way to write a circuit: verilog, vhdl or svg"), NoticeLevel::Warn)],
                };
                let file = open.path.join("export").join(format!("{circuit}.{ext}"));
                if let Err(why) = std::fs::create_dir_all(open.path.join("export")).and_then(|_| std::fs::write(&file, &text)) {
                    return vec![notice(format!("{}: {why}", file.display()), NoticeLevel::Error)];
                }
                let shown = format!("{}/export/{circuit}.{ext}", open.shown_path);
                vec![notice(format!("wrote {shown}"), NoticeLevel::Info), Event::CircuitExported { path: shown, format, text }]
            }
            Command::CircuitImport { path } => {
                if self.dirty() {
                    return vec![notice("save or discard the open circuit's changes before opening another".into(), NoticeLevel::Warn)];
                }
                let file = match within(root, &path, "dig") {
                    Ok(f) => f,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                let (Some(dir), Some(name)) = (file.parent(), file.file_name().map(|n| n.to_string_lossy().into_owned())) else {
                    return vec![notice(format!("{path} is nowhere"), NoticeLevel::Warn)];
                };
                let (project, dropped) = match circ::import_dig(dir, &name) {
                    Ok(x) => x,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                let folder = dir.join(format!("{}.{EXTENSION}", slug(name.trim_end_matches(".dig"))));
                if folder.exists() {
                    return vec![notice(format!("{} is already there: open it, or move it away to import again", folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), NoticeLevel::Warn)];
                }
                if let Err(why) = circ::write_project(&folder, &project) {
                    return vec![notice(why, NoticeLevel::Warn)];
                }
                self.show(root, folder, project);
                let mut events = self.events(root);
                let made = self.open.as_ref().map(|o| o.shown_path.clone()).unwrap_or_default();
                events.push(notice(format!("imported {path} as {made}"), NoticeLevel::Info));
                if !dropped.is_empty() {
                    events.push(notice(format!("left out, as Proton has no such parts: {}", dropped.join("; ")), NoticeLevel::Warn));
                }
                events
            }
            Command::CircuitAnalyse { generation, circuit, expression } => {
                let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let source = expression.clone().unwrap_or_else(|| circuit.clone().unwrap_or_else(|| open.shown.clone()));
                let analysis = match table_from(&open.project, circuit.as_deref().unwrap_or(&open.shown), expression.as_deref()) {
                    Ok(t) => analyse(&source, &t),
                    Err(why) => CircuitAnalysisView { source, inputs: vec![], outputs: vec![], columns: vec![], sop: vec![], pos: vec![], groups: vec![], exact: true, error: Some(why) },
                };
                vec![Event::CircuitAnalysis { generation, analysis: Box::new(analysis) }]
            }
            Command::CircuitSynthesise { generation, circuit, expression, style, name } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let name = name.trim().to_string();
                if !circ::is_name(&name) || name.contains(' ') {
                    return vec![notice(format!("“{name}” cannot name a circuit: letters, digits, - and _"), NoticeLevel::Warn)];
                }
                if open.project.circuits.contains_key(&name) {
                    return vec![notice(format!("there is a circuit {name} already"), NoticeLevel::Warn)];
                }
                let style = match style.as_str() {
                    "andOr" => proton_circuit::analysis::Style::AndOr,
                    "nand" => proton_circuit::analysis::Style::Nand,
                    "nor" => proton_circuit::analysis::Style::Nor,
                    other => return vec![notice(format!("“{other}” is not a way to build: andOr, nand or nor"), NoticeLevel::Warn)],
                };
                let shown = open.shown.clone();
                let built = table_from(&open.project, circuit.as_deref().unwrap_or(&shown), expression.as_deref())
                    .and_then(|t| proton_circuit::analysis::synthesise(&t, style).map(|d| (t, d)));
                let (table, drawing) = match built {
                    Ok(x) => x,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                let mut after = open.project.clone();
                after.circuits.insert(name.clone(), drawing);
                after.tests.entry(name.clone()).or_default().insert("truth".into(), proton_circuit::analysis::as_test(&table));
                open.shown = name.clone();
                record(open, after);
                vec![notice(format!("built {name}, with its truth table as the test “truth”"), NoticeLevel::Info), self.project_event()]
            }
            Command::CircuitMachineSet { generation, name, text } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let name = name.trim().to_string();
                if !circ::is_name(&name) || name.contains(' ') {
                    return vec![notice(format!("“{name}” cannot name a machine: letters, digits, - and _"), NoticeLevel::Warn)];
                }
                if let Some(t) = &text {
                    if let Err(why) = proton_circuit::fsm::parse(t) {
                        return vec![notice(format!("{name}: {why}"), NoticeLevel::Warn)];
                    }
                }
                let mut after = open.project.clone();
                match text {
                    Some(t) => {
                        let t = if t.ends_with('\n') { t } else { format!("{t}\n") };
                        after.machines.insert(name, t);
                    }
                    None => {
                        after.machines.remove(&name);
                    }
                }
                if after != open.project {
                    record(open, after);
                }
                vec![self.project_event()]
            }
            Command::CircuitMachineShow { generation, name, text } => {
                let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let text = match (&text, &name) {
                    (Some(t), _) => t.clone(),
                    (None, Some(n)) => match open.project.machines.get(n) {
                        Some(t) => t.clone(),
                        None => return vec![notice(format!("there is no machine {n}"), NoticeLevel::Warn)],
                    },
                    (None, None) => return Vec::new(),
                };
                vec![Event::CircuitMachine { generation, machine: Box::new(machine_view(name, &text)) }]
            }
            Command::CircuitMachineBuild { generation, name, flipflop, circuit } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else {
                    return vec![notice("that project is no longer open".into(), NoticeLevel::Warn)];
                };
                let circuit = circuit.trim().to_string();
                if !circ::is_name(&circuit) || circuit.contains(' ') {
                    return vec![notice(format!("“{circuit}” cannot name a circuit: letters, digits, - and _"), NoticeLevel::Warn)];
                }
                let ff = match flipflop.as_str() {
                    "d" => proton_circuit::fsm::FlipFlop::D,
                    "jk" => proton_circuit::fsm::FlipFlop::Jk,
                    other => return vec![notice(format!("“{other}” is not a flip-flop to build with: d or jk"), NoticeLevel::Warn)],
                };
                let Some(text) = open.project.machines.get(&name) else {
                    return vec![notice(format!("there is no machine {name}: keep it first"), NoticeLevel::Warn)];
                };
                let mut after = open.project.clone();
                if let Err(why) = proton_circuit::fsm::parse(text).and_then(|m| proton_circuit::fsm::build(&m, ff, &circuit, &mut after)) {
                    return vec![notice(why, NoticeLevel::Warn)];
                }
                open.shown = circuit.clone();
                record(open, after);
                vec![notice(format!("built {circuit} and {circuit}_logic from {name}, with its run as the test “machine”"), NoticeLevel::Info), self.project_event()]
            }
            Command::CircuitTraceClear { generation } => {
                let run = match self.running(generation) {
                    Ok(run) => run,
                    Err(events) => return events,
                };
                run.samples.clear();
                run.taken = 0;
                run.sample();
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
            Command::CircuitFromTable { project, circuit, inputs, column, style } => {
                let style = match style.as_str() {
                    "andOr" => proton_circuit::analysis::Style::AndOr,
                    "nand" => proton_circuit::analysis::Style::Nand,
                    "nor" => proton_circuit::analysis::Style::Nor,
                    other => return vec![notice(format!("“{other}” is not a way to build: andOr, nand or nor"), NoticeLevel::Warn)],
                };
                let values: Vec<u8> = column.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
                if inputs.is_empty() || values.len() != 1 << inputs.len().min(31) || values.iter().any(|b| !matches!(b, b'0' | b'1' | b'x')) {
                    return vec![notice(format!("a column of {} rows of 0, 1 or x was expected for {} inputs", 1u64 << inputs.len().min(31), inputs.len()), NoticeLevel::Warn)];
                }
                let table = proton_circuit::analysis::Table { inputs, outputs: vec!["Y".into()], values: vec![values] };
                let drawing = match proton_circuit::analysis::synthesise(&table, style) {
                    Ok(d) => d,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if let Err(events) = self.open_or_make(root, &project, "building") {
                    return events;
                }
                let Some(open) = &mut self.open else { return Vec::new() };
                let name = fresh_circuit(open, &circuit);
                let mut after = open.project.clone();
                after.circuits.insert(name.clone(), drawing);
                after.tests.entry(name.clone()).or_default().insert("truth".into(), proton_circuit::analysis::as_test(&table));
                open.shown = name.clone();
                record(open, after);
                let mut events = self.events(root);
                events.push(notice(format!("built {name}, with its truth table as the test “truth”"), NoticeLevel::Info));
                events
            }
            Command::CircuitSelect { generation, circuit, ids, wires } => {
                if let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) {
                    open.selected = (circuit, ids, wires);
                }
                Vec::new()
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

    /// What the agent's calls changed on screen, taken for the window.
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// A command as the window would send it, its events kept for the
    /// window and its notices — the reasons a thing was refused — as the
    /// call's error.
    fn command(&mut self, root: &Path, command: Command) -> Result<Vec<Event>, String> {
        let events = self.apply(root, command);
        let refused: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::Notice { text, level, .. } if *level != NoticeLevel::Info => Some(text.clone()),
                _ => None,
            })
            .collect();
        self.pending.extend(events.iter().filter(|e| !matches!(e, Event::Notice { .. })).cloned());
        if refused.is_empty() {
            Ok(events)
        } else {
            Err(refused.join("; "))
        }
    }

    fn generation(&self) -> Result<u32, String> {
        self.open.as_ref().map(|o| o.generation).ok_or_else(|| "No circuit project is open in Circuit: circuit_info lists the folder's, circuit_open opens or makes one.".into())
    }

    fn named(&self, input: &serde_json::Value) -> Result<String, String> {
        let open = self.open.as_ref().ok_or("No circuit project is open")?;
        let name = input.get("circuit").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| open.shown.clone());
        if !open.project.circuits.contains_key(&name) {
            return Err(format!("there is no circuit {name}: the project has {}", open.project.circuits.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        Ok(name)
    }

    /// An agent's call: what it did, or why it could not.
    pub fn tool(&mut self, root: &Path, name: &str, input: &serde_json::Value) -> Result<String, String> {
        let text = |k: &str| input.get(k).and_then(|v| v.as_str()).map(str::to_string);
        match name {
            "circuit_info" => Ok(self.info(root)),
            "circuit_open" => {
                if let Some(new) = text("new") {
                    self.command(root, Command::CircuitNew { name: new })?;
                } else {
                    let path = text("path").ok_or("circuit_open: give `path`, or `new` with a name")?;
                    if path.ends_with(".dig") {
                        self.command(root, Command::CircuitImport { path })?;
                    } else {
                        self.command(root, Command::CircuitOpen { path })?;
                    }
                }
                Ok(self.info(root))
            }
            "circuit_read" => {
                let circuit = self.named(input)?;
                Ok(self.read(&circuit))
            }
            "circuit_edit" => {
                let generation = self.generation()?;
                let circuit = text("circuit").unwrap_or_else(|| self.open.as_ref().map(|o| o.shown.clone()).unwrap_or_default());
                let exists = self.open.as_ref().is_some_and(|o| o.project.circuits.contains_key(&circuit));
                if !exists {
                    if input.get("create").and_then(|v| v.as_bool()) == Some(true) {
                        self.command(root, Command::CircuitAdd { name: circuit.clone() })?;
                    } else {
                        return Err(format!("there is no circuit {circuit}: give `create: true` to make it"));
                    }
                }
                let ops: Vec<CircuitOp> = serde_json::from_value(input.get("ops").cloned().unwrap_or_default()).map_err(|e| format!("circuit_edit: the ops do not read: {e}"))?;
                let events = self.command(root, Command::CircuitEdit { generation, circuit: circuit.clone(), ops })?;
                let made: Vec<String> = events
                    .iter()
                    .find_map(|e| match e {
                        Event::CircuitProject { project: Some(p) } => Some(p.made.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let open = self.open.as_ref().ok_or("the project closed")?;
                let pins: HashMap<String, Result<Vec<circ::PinAt>, String>> = open.project.pins(&circuit).into_iter().collect();
                let mut out = format!("Done, as one step the person can undo; held until they save. {circuit} has {} parts and {} wires.", open.project.circuits[&circuit].parts.len(), open.project.circuits[&circuit].wires.len());
                if !made.is_empty() {
                    out.push_str("\nMade:");
                    for id in &made {
                        out.push_str(&format!("\n{id}: {}", pins_text(pins.get(id))));
                    }
                }
                Ok(out)
            }
            "circuit_test" => {
                let generation = self.generation()?;
                let circuit = self.named(input)?;
                if let Some(table) = text("table") {
                    if input.get("keep").and_then(|v| v.as_bool()) == Some(true) {
                        let label = text("label").ok_or("circuit_test: `keep` needs a `label` to keep the table as")?;
                        self.command(root, Command::CircuitTestSet { generation, circuit: circuit.clone(), label, text: Some(table.clone()) })?;
                    }
                    let open = self.open.as_ref().ok_or("the project closed")?;
                    proton_circuit::testlang::check(&table)?;
                    let built = open.project.build(&circuit)?;
                    let o = proton_circuit::testlang::run(&built, text("label").as_deref().unwrap_or("table"), &table);
                    return Ok(results_text(&[CircuitTestResult { label: o.label, rows: o.rows as u32, failures: o.failures, error: o.error }]));
                }
                let events = self.command(root, Command::CircuitTestRun { generation, circuit, label: text("label") })?;
                let results = events.into_iter().find_map(|e| match e {
                    Event::CircuitTests { results, .. } => Some(results),
                    _ => None,
                });
                Ok(results_text(&results.unwrap_or_default()))
            }
            "circuit_analyse" => {
                let generation = self.generation()?;
                let expression = text("expression");
                let circuit = if expression.is_some() { None } else { Some(self.named(input)?) };
                let events = self.command(root, Command::CircuitAnalyse { generation, circuit, expression })?;
                let a = events
                    .into_iter()
                    .find_map(|e| match e {
                        Event::CircuitAnalysis { analysis, .. } => Some(*analysis),
                        _ => None,
                    })
                    .ok_or("nothing was analysed")?;
                if let Some(why) = a.error {
                    return Err(why);
                }
                let mut out = format!("Inputs: {}\nOutputs: {}\n", a.inputs.join(" "), a.outputs.join(" "));
                for (k, o) in a.outputs.iter().enumerate() {
                    out.push_str(&format!("{o} = {}\n{o} = {}\n", a.sop[k], a.pos[k]));
                }
                if !a.exact {
                    out.push_str("(some covers were chosen greedily and may not be the least)\n");
                }
                if a.inputs.len() <= 6 {
                    let n = a.inputs.len();
                    out.push_str(&format!("\n{} | {}\n", a.inputs.join(" "), a.outputs.join(" ")));
                    for r in 0..1usize << n {
                        let ins: Vec<String> = (0..n).map(|i| ((r >> (n - 1 - i)) & 1).to_string()).collect();
                        let outs: Vec<String> = a.columns.iter().map(|c| c[r..r + 1].to_string()).collect();
                        out.push_str(&format!("{} | {}\n", ins.join(" "), outs.join(" ")));
                    }
                }
                Ok(out)
            }
            "circuit_synthesise" => {
                let generation = self.generation()?;
                let name = text("name").ok_or("circuit_synthesise: `name` is required")?;
                let expression = text("expression");
                let circuit = if expression.is_some() { None } else { Some(self.named(input)?) };
                let style = text("style").unwrap_or_else(|| "andOr".into());
                self.command(root, Command::CircuitSynthesise { generation, circuit, expression, style, name: name.clone() })?;
                Ok(format!("Built {name}, shown, with its truth table as the test \"truth\"; held until the person saves.\n\n{}", self.read(&name)))
            }
            other => Err(format!("no such tool: {other}")),
        }
    }

    /// The project open, its circuits and what each holds; or the folder's.
    fn info(&self, root: &Path) -> String {
        let Some(open) = &self.open else {
            let found = find(root);
            if found.is_empty() {
                return "No circuit project is open, and the folder has none: circuit_open with `new` makes one.".into();
            }
            let list: Vec<String> = found.iter().map(|f| format!("- {}{}", f.path, if f.kind == "dig" { " (Digital: imported when opened)" } else { "" })).collect();
            return format!("No circuit project is open. In the folder:\n{}", list.join("\n"));
        };
        let p = &open.project;
        let mut out = format!("Project {}{}\n", open.shown_path, if p != &open.saved { " (unsaved changes)" } else { "" });
        for (name, d) in &p.circuits {
            let pins = |kind: &str| -> String {
                circ::ordered(d)
                    .iter()
                    .filter(|q| q.kind == kind || (kind == "in" && matches!(q.kind.as_str(), "clock" | "button")))
                    .map(|q| {
                        let label = q.attrs.get("label").cloned().unwrap_or_else(|| q.id.clone());
                        match q.attrs.get("bits") {
                            Some(b) if b != "1" => format!("{label}[{b}]"),
                            _ => label,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let mut marks = Vec::new();
            if *name == p.top {
                marks.push("top");
            }
            if *name == open.shown {
                marks.push("on screen");
            }
            let marks = if marks.is_empty() { String::new() } else { format!(" ({})", marks.join(", ")) };
            out.push_str(&format!("- {name}{marks}: {} parts, {} wires; in {}; out {}", d.parts.len(), d.wires.len(), or_none(pins("in")), or_none(pins("out"))));
            if let Some(t) = p.tests.get(name) {
                out.push_str(&format!("; tables {}", t.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
            out.push('\n');
        }
        if !p.machines.is_empty() {
            out.push_str(&format!("State machines: {}\n", p.machines.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        if !p.data.is_empty() {
            out.push_str(&format!("Memory files: {}\n", p.data.keys().map(|k| format!("data/{k}.hex")).collect::<Vec<_>>().join(", ")));
        }
        out.push_str(if open.run.is_some() { "It is running.\n" } else { "It is not running.\n" });
        out
    }

    /// A circuit's text, and where each part's pins are.
    fn read(&self, circuit: &str) -> String {
        let Some(open) = &self.open else { return String::new() };
        let Some(d) = open.project.circuits.get(circuit) else { return format!("there is no circuit {circuit}") };
        let text = circ::write(d);
        let lines: Vec<&str> = text.lines().collect();
        let mut out = format!("# {circuit}.circ\n");
        for l in lines.iter().take(MOST_LINES) {
            out.push_str(l);
            out.push('\n');
        }
        if lines.len() > MOST_LINES {
            out.push_str(&format!("# … {} more lines\n", lines.len() - MOST_LINES));
        }
        out.push_str("\n# pins (x,y), as the simulator joins them\n");
        let pins: HashMap<String, Result<Vec<circ::PinAt>, String>> = open.project.pins(circuit).into_iter().collect();
        for part in circ::ordered(d).iter().take(MOST_LINES) {
            out.push_str(&format!("# {} {}: {}\n", part.id, part.kind, pins_text(pins.get(&part.id))));
        }
        out
    }

    /// The circuit on screen, what is selected, and the last tables run:
    /// what `@circuit` sends.
    pub fn about(&self) -> String {
        let Some(open) = &self.open else { return "No circuit project is open in Circuit.".into() };
        let mut out = format!("Circuit {} of {}\n", open.shown, open.shown_path);
        out.push_str(&self.read(&open.shown));
        let (circuit, ids, wires) = &open.selected;
        if *circuit == open.shown && (!ids.is_empty() || !wires.is_empty()) {
            out.push_str(&format!("\nSelected: {}{}\n", ids.join(", "), if wires.is_empty() { String::new() } else { format!(" and {} wires", wires.len()) }));
        } else {
            out.push_str("\nNothing is selected.\n");
        }
        let (tested, results) = &open.last_tests;
        if *tested == open.shown && !results.is_empty() {
            out.push_str("\nLast tests run:\n");
            out.push_str(&results_text(results));
        }
        out
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

    /// What the run has recorded, as CSV — a row a sample, a column a
    /// signal, numbers in decimal and high-Z empty — and a name for it.
    pub fn trace_csv(&self, generation: u32) -> Result<(String, String), String> {
        let open = self.open.as_ref().filter(|o| o.generation == generation).ok_or("that project is no longer open")?;
        let run = open.run.as_ref().ok_or("the circuit is not running, so nothing is recorded")?;
        if run.signals.is_empty() {
            return Err("the circuit has no inputs or outputs to record".into());
        }
        let mut csv = String::from("sample");
        for (id, ..) in &run.signals {
            csv.push(',');
            csv.push_str(id);
        }
        csv.push('\n');
        let first = run.taken as usize - run.samples.len();
        for (i, row) in run.samples.iter().enumerate() {
            csv.push_str(&(first + i).to_string());
            for v in row {
                csv.push(',');
                if let Some(v) = v {
                    csv.push_str(&v.to_string());
                }
            }
            csv.push('\n');
        }
        let label = format!("{}-{}", open.name, run.circuit).chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' }).collect();
        Ok((csv, label))
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
            run.sample();
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
                trace: CircuitTraceView {
                    signals: run.signals.iter().map(|(id, _, _, bits)| (id.clone(), *bits)).collect(),
                    samples: run
                        .samples
                        .iter()
                        .skip(run.samples.len().saturating_sub(SHOWN_SAMPLES))
                        .map(|row| row.iter().map(|v| v.map(|v| format!("{v:x}"))).collect())
                        .collect(),
                    total: run.taken,
                },
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

    /// The project named opened, or made with one empty circuit, `start`,
    /// for something to be put in it; refused while another has unsaved
    /// changes.
    fn open_or_make(&mut self, root: &Path, project: &str, doing: &str) -> Result<(), Vec<Event>> {
        let folder = format!("{}.{EXTENSION}", slug(project));
        let dir = inside(root, &folder).map_err(|why| vec![notice(why, NoticeLevel::Warn)])?;
        if self.open.as_ref().is_some_and(|o| o.path == dir) {
            return Ok(());
        }
        if self.dirty() {
            return Err(vec![notice(format!("save or discard the open circuit's changes before {doing}"), NoticeLevel::Warn)]);
        }
        if dir.exists() {
            let p = circ::read(&dir).map_err(|why| vec![notice(why, NoticeLevel::Warn)])?;
            self.show(root, dir, p);
        } else {
            let mut p = Project::new();
            p.circuits.clear();
            p.top = "start".into();
            p.circuits.insert("start".into(), Drawing::default());
            circ::write_project(&dir, &p).map_err(|why| vec![notice(why, NoticeLevel::Warn)])?;
            self.show(root, dir, p);
        }
        Ok(())
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
            selected: (String::new(), Vec::new(), Vec::new()),
            last_tests: (String::new(), Vec::new()),
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
                tables: p.tests.get(&open.shown).map(|t| t.iter().map(|(l, t)| (l.clone(), t.clone())).collect()).unwrap_or_default(),
                machines: p.machines.iter().map(|(n, t)| (n.clone(), t.clone())).collect(),
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
        signals: Vec::new(),
        samples: std::collections::VecDeque::new(),
        taken: 0,
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
            // The inputs and what shows values, in id order, recorded.
            let mut signals: Vec<(String, PartId, bool, u8)> = run
                .parts
                .iter()
                .filter(|(_, (_, k))| SETTABLE.contains(&k.as_str()) || SHOWN.contains(&k.as_str()))
                .map(|(id, (part, k))| {
                    let input = SETTABLE.contains(&k.as_str());
                    let bits = if input { sim.output(*part, 0).bits } else { sim.shown(*part).bits };
                    (id.clone(), *part, input, bits)
                })
                .collect();
            signals.sort_by(|a, b| a.0.cmp(&b.0));
            run.signals = signals;
            run.sim = Some(sim);
            run.sample();
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
    if !hold {
        run.sample();
    }
}

/// A machine's text read, for its diagram and table, or why it does not.
fn machine_view(name: Option<String>, text: &str) -> CircuitMachineView {
    let mut view = CircuitMachineView { name, inputs: vec![], outputs: vec![], states: vec![], state_outputs: vec![], transitions: vec![], mealy: false, bits: 0, table: vec![], error: None };
    let m = match proton_circuit::fsm::parse(text) {
        Ok(m) => m,
        Err(why) => {
            view.error = Some(why);
            return view;
        }
    };
    let set = |outs: &[(String, bool)]| outs.iter().map(|(k, v)| format!("{k}={}", *v as u8)).collect::<Vec<_>>().join(" ");
    let k = m.inputs.len();
    let bits = |v: u32, n: usize| (0..n).map(|i| if v >> (n - 1 - i) & 1 == 1 { '1' } else { '0' }).collect::<String>();
    view.table = m.table().into_iter().map(|r| (r.state as u32, bits(r.inputs, k), r.next as u32, r.outputs.iter().map(|b| if *b { '1' } else { '0' }).collect())).collect();
    view.inputs = m.inputs.clone();
    view.outputs = m.outputs.clone();
    view.states = m.states.clone();
    view.state_outputs = m.state_outputs.iter().map(|o| set(o)).collect();
    view.transitions = m.transitions.iter().map(|t| (t.from as u32, t.to as u32, t.when.clone(), set(&t.outputs))).collect();
    view.mealy = m.mealy();
    view.bits = m.bits() as u32;
    view
}

/// A table of expressions, if given, or else of a circuit of the project.
fn table_from(project: &Project, circuit: &str, expression: Option<&str>) -> Result<proton_circuit::analysis::Table, String> {
    match expression.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => proton_circuit::analysis::expressions(e),
        None => proton_circuit::analysis::project_table(project, circuit),
    }
}

/// A table minimised, each output both ways.
fn analyse(source: &str, t: &proton_circuit::analysis::Table) -> CircuitAnalysisView {
    use proton_circuit::analysis::{minimise, pos_text, sop_text};
    let mut view = CircuitAnalysisView {
        source: source.to_string(),
        inputs: t.inputs.clone(),
        outputs: t.outputs.clone(),
        columns: t.values.iter().map(|c| String::from_utf8_lossy(c).into_owned()).collect(),
        sop: Vec::new(),
        pos: Vec::new(),
        groups: Vec::new(),
        exact: true,
        error: None,
    };
    for col in &t.values {
        match minimise(t.inputs.len(), col) {
            Ok(m) => {
                view.sop.push(sop_text(&m, &t.inputs));
                view.pos.push(pos_text(&m, &t.inputs));
                view.groups.push(m.sop.iter().map(|g| [g.value, g.care]).collect());
                view.exact &= m.exact;
            }
            Err(why) => {
                view.error = Some(why);
                view.sop.push(String::new());
                view.pos.push(String::new());
                view.groups.push(Vec::new());
            }
        }
    }
    view
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

fn or_none(s: String) -> String {
    if s.is_empty() {
        "none".into()
    } else {
        s
    }
}

/// A part's pins as the model reads them: `in 3,0 · in 3,2 · out 6,1`.
fn pins_text(pins: Option<&Result<Vec<circ::PinAt>, String>>) -> String {
    match pins {
        Some(Ok(pins)) if pins.is_empty() => "no pins".into(),
        Some(Ok(pins)) => pins.iter().map(|p| format!("{} {},{}", if p.input { "in" } else { "out" }, p.x, p.y)).collect::<Vec<_>>().join(" · "),
        Some(Err(why)) => format!("not placed: {why}"),
        None => "?".into(),
    }
}

/// Tables run, as the model reads them.
fn results_text(results: &[CircuitTestResult]) -> String {
    let mut out = String::new();
    for r in results {
        match (&r.error, r.failures.is_empty()) {
            (Some(why), _) => out.push_str(&format!("{}: could not run: {why}\n", r.label)),
            (None, true) => out.push_str(&format!("{}: all {} rows pass\n", r.label, r.rows)),
            (None, false) => {
                out.push_str(&format!("{}: {} rows, failing:\n", r.label, r.rows));
                for f in &r.failures {
                    out.push_str(&format!("  {f}\n"));
                }
            }
        }
    }
    out
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
    within(root, rel, EXTENSION)
}

/// A path of the project ending `.<ext>`, refused if it leads outside.
fn within(root: &Path, rel: &str, ext: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if named.extension().and_then(|e| e.to_str()) != Some(ext) {
        return Err(if ext == EXTENSION { format!("{rel} is not a circuit: a circuit is a .{EXTENSION} folder") } else { format!("{rel} is not a .{ext} file") });
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

/// A circuit's name from words, as the project has none of yet: lower
/// case, `_` between words, a letter first, a number after if taken.
fn fresh_circuit(open: &Open, words: &str) -> String {
    let base: String = slug(words).replace('-', "_");
    let base = if base.is_empty() || !base.starts_with(|c: char| c.is_ascii_alphabetic()) { format!("p_{base}") } else { base };
    (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}_{n}") }).find(|n| !open.project.circuits.contains_key(n)).unwrap_or(base)
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
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let rel = || path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let ext = path.extension().and_then(|e| e.to_str());
            if !path.is_dir() {
                // A Digital file, to import.
                if ext == Some("dig") {
                    out.push(CircuitFound { path: rel(), name: name.trim_end_matches(".dig").to_string(), kind: "dig".into() });
                }
                continue;
            }
            if ext == Some(EXTENSION) {
                out.push(CircuitFound { path: rel(), name: name.trim_end_matches(&format!(".{EXTENSION}")).to_string(), kind: "circuit".into() });
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
        assert_eq!(found, vec![CircuitFound { path: "full-adder.circuit".into(), name: "full-adder".into(), kind: "circuit".into() }]);

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

    /// A quick run of `root`'s project for a second, stopped: its news.
    fn quick_for_a_second(c: &mut Circuits, root: &Path, path: &str) -> (CircuitQuickView, CircuitRunView) {
        let open = if path.ends_with(".dig") { Command::CircuitImport { path: path.into() } } else { Command::CircuitOpen { path: path.into() } };
        let g = view(&c.apply(root, open)).unwrap().generation;
        let r = run_of(&c.apply(root, Command::CircuitStart { generation: g })).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(run_of(&c.apply(root, Command::CircuitQuick { generation: g })).unwrap().quick.unwrap().running);
        std::thread::sleep(Duration::from_secs(1));
        c.apply(root, Command::CircuitStop);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(r) = run_of(&c.poll()) {
                if let Some(q) = r.quick.clone().filter(|q| !q.running) {
                    assert_eq!(q.ended.as_deref(), Some("stopped"));
                    return (q, r);
                }
            }
            assert!(Instant::now() < deadline, "the quick run never stopped");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// C9's budget: a small CPU — `tiny-cpu.circuit`, proton-circuit's
    /// fixture — quick-run at 100 kHz or more, as the window's Quick runs
    /// it. Kept by a release build; a debug build only says how fast.
    #[test]
    fn a_small_cpu_quick_runs_at_100_khz_or_more() {
        let root = scratch("tiny-cpu");
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../proton-circuit/tests/tiny-cpu.circuit");
        for sub in ["", "circuits", "data", "tests"] {
            std::fs::create_dir_all(root.join("tiny-cpu.circuit").join(sub)).unwrap();
            for e in std::fs::read_dir(from.join(sub)).unwrap().flatten().filter(|e| e.path().is_file()) {
                std::fs::copy(e.path(), root.join("tiny-cpu.circuit").join(sub).join(e.file_name())).unwrap();
            }
        }
        let mut c = Circuits::default();
        let (q, r) = quick_for_a_second(&mut c, &root, "tiny-cpu.circuit");
        println!("tiny-cpu quick run: {} cycles, {:.0} Hz", q.cycles, q.hz);
        // The program has long since counted down and stopped at seven.
        assert_eq!(r.values.iter().find(|(k, _)| k == "OUT").map(|(_, v)| v.hex.as_str()), Some("7"));
        if !cfg!(debug_assertions) {
            assert!(q.hz >= 100_000.0, "{:.0} Hz", q.hz);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// The same for a folder of `.dig` files with a `basic-cpu.dig`, e.g.
    /// a course's CPU; copied to a scratch folder first, since a `.dig`
    /// file is imported beside itself.
    #[test]
    #[ignore = "needs PROTON_CIRCUIT_DIG_DIR, a folder with basic-cpu.dig"]
    fn a_folders_cpus_quick_run() {
        let Ok(dir) = std::env::var("PROTON_CIRCUIT_DIG_DIR") else { return };
        let root = scratch("dig-cpu");
        for e in std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "dig" || x == "hex")) {
            std::fs::copy(e.path(), root.join(e.file_name())).unwrap();
        }
        for file in ["basic-cpu.dig", "extended-cpu.dig"] {
            if root.join(file).exists() {
                let (q, _) = quick_for_a_second(&mut Circuits::default(), &root, file);
                println!("{file} quick run: {} cycles, {:.0} Hz", q.cycles, q.hz);
            }
        }
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
    fn tables_are_held_edits_run_against_the_circuit_and_a_run_is_recorded() {
        let root = scratch("tables");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "inv".into() })).unwrap().generation;
        let inv = "part A in at 0,0 label=A\npart N not at 2,0\npart Y out at 6,0 label=Y\nwire 0,0 2,0\nwire 4,0 6,0\n";
        c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops: vec![CircuitOp::Paste { text: inv.into(), dx: 0, dy: 0 }] });
        let set = |c: &mut Circuits, label: &str, text: Option<&str>| c.apply(&root, Command::CircuitTestSet { generation: g, circuit: "main".into(), label: label.into(), text: text.map(str::to_string) });
        let v = view(&set(&mut c, "truth", Some("A Y\n0 1\n1 0"))).unwrap();
        assert_eq!(v.tables, vec![("truth".to_string(), "A Y\n0 1\n1 0\n".to_string())]);
        assert!(set(&mut c, "bad", Some("A Y\nloop(i\n")).iter().any(|e| matches!(e, Event::Notice { .. })), "a table that does not read is refused");
        assert!(set(&mut c, "two words", Some("A Y\n0 1\n")).iter().any(|e| matches!(e, Event::Notice { .. })));
        set(&mut c, "wrong", Some("A Y\n0 0\n1 1\n"));
        let results = |events: Vec<Event>| match events.into_iter().find(|e| matches!(e, Event::CircuitTests { .. })) {
            Some(Event::CircuitTests { results, .. }) => results,
            _ => panic!("no results"),
        };
        let all = results(c.apply(&root, Command::CircuitTestRun { generation: g, circuit: "main".into(), label: None }));
        assert_eq!(all.iter().map(|r| (r.label.as_str(), r.rows, r.failures.len())).collect::<Vec<_>>(), vec![("truth", 2, 0), ("wrong", 2, 2)]);
        assert!(all[1].failures[0].contains("Y is 1, not 0x0"), "{:?}", all[1].failures);
        let one = results(c.apply(&root, Command::CircuitTestRun { generation: g, circuit: "main".into(), label: Some("truth".into()) }));
        assert_eq!(one.len(), 1);
        // Taken away, and back with undo; saved as files.
        set(&mut c, "wrong", None);
        c.apply(&root, Command::CircuitSave);
        assert!(root.join("inv.circuit/tests/main.truth.tests").exists() && !root.join("inv.circuit/tests/main.wrong.tests").exists());
        let v = view(&c.apply(&root, Command::CircuitUndo)).unwrap();
        assert_eq!(v.tables.len(), 2);

        // A run records each change; the graph's samples, and as CSV.
        c.apply(&root, Command::CircuitStart { generation: g });
        for v in ["1", "0", "1"] {
            c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: v.into(), hold: false });
        }
        let r = run_of(&c.apply(&root, Command::CircuitSet { generation: g, id: "A".into(), value: "0".into(), hold: true })).unwrap();
        assert_eq!(r.trace.signals, vec![("A".to_string(), 1), ("Y".to_string(), 1)]);
        assert_eq!(r.trace.total, 4, "power-on and three settled changes; a held one waits");
        let r = run_of(&c.apply(&root, Command::CircuitStep { generation: g })).unwrap();
        assert_eq!(r.trace.samples.last().unwrap(), &vec![Some("0".to_string()), Some("0".to_string())], "the input changed, the NOT not yet");
        let (csv, label) = c.trace_csv(g).unwrap();
        assert_eq!(label, "inv-main");
        assert_eq!(csv, "sample,A,Y\n0,0,1\n1,1,0\n2,0,1\n3,1,0\n4,0,0\n");
        let r = run_of(&c.apply(&root, Command::CircuitTraceClear { generation: g })).unwrap();
        assert_eq!((r.trace.total, r.trace.samples.len()), (1, 1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_table_is_analysed_and_a_circuit_built_from_it_passes_it() {
        let root = scratch("analysis");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "logic".into() })).unwrap().generation;
        let analysis = |events: Vec<Event>| match events.into_iter().find(|e| matches!(e, Event::CircuitAnalysis { .. })) {
            Some(Event::CircuitAnalysis { analysis, .. }) => *analysis,
            _ => panic!("no analysis"),
        };
        let a = analysis(c.apply(&root, Command::CircuitAnalyse { generation: g, circuit: None, expression: Some("M = A B + A C + B C".into()) }));
        assert_eq!((a.inputs.join(" "), a.columns[0].as_str(), a.sop[0].as_str()), ("A B C".into(), "00010111", "B·C + A·C + A·B"));
        assert_eq!(a.groups[0].len(), 3);
        // The empty main circuit has no outputs to tabulate.
        assert!(analysis(c.apply(&root, Command::CircuitAnalyse { generation: g, circuit: None, expression: None })).error.is_some());
        // Built as NANDs, shown, with its table as a test that passes.
        let v = view(&c.apply(&root, Command::CircuitSynthesise { generation: g, circuit: None, expression: Some("M = A B + A C + B C".into()), style: "nand".into(), name: "majority".into() })).unwrap();
        assert!(v.dirty && v.shown == "majority" && v.parts.iter().filter(|p| p.kind == "nand").count() == 4);
        assert_eq!(v.tables[0].0, "truth");
        let results = match c.apply(&root, Command::CircuitTestRun { generation: g, circuit: "majority".into(), label: None }).remove(0) {
            Event::CircuitTests { results, .. } => results,
            other => panic!("{other:?}"),
        };
        assert!(results[0].failures.is_empty() && results[0].error.is_none() && results[0].rows == 8, "{results:?}");
        // And the circuit analysed gives the same table back.
        let back = analysis(c.apply(&root, Command::CircuitAnalyse { generation: g, circuit: Some("majority".into()), expression: None }));
        assert_eq!(back.columns, vec!["00010111".to_string()]);
        assert!(c.apply(&root, Command::CircuitSynthesise { generation: g, circuit: None, expression: Some("A".into()), style: "wires".into(), name: "x".into() }).iter().any(|e| matches!(e, Event::Notice { .. })));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_state_machine_is_kept_shown_and_built_passing_its_own_run() {
        let root = scratch("machine");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "seq".into() })).unwrap().generation;
        let text = "inputs X\noutputs Z\nstate S0\nstate S1\nstate S2 Z=1\nS0 -> S1 when X\nS1 -> S2 when X\nS1 -> S0 when X'\nS2 -> S0 when !X\n";
        assert!(c.apply(&root, Command::CircuitMachineSet { generation: g, name: "ones".into(), text: Some("A -> B when Y".into()) }).iter().any(|e| matches!(e, Event::Notice { .. })));
        let v = view(&c.apply(&root, Command::CircuitMachineSet { generation: g, name: "ones".into(), text: Some(text.into()) })).unwrap();
        assert_eq!(v.machines, vec![("ones".to_string(), text.to_string())]);
        let shown = match c.apply(&root, Command::CircuitMachineShow { generation: g, name: Some("ones".into()), text: None }).remove(0) {
            Event::CircuitMachine { machine, .. } => *machine,
            other => panic!("{other:?}"),
        };
        assert_eq!((shown.states.len(), shown.transitions.len(), shown.bits, shown.mealy, shown.table.len()), (3, 4, 2, false, 6));
        assert_eq!(shown.state_outputs[2], "Z=1");
        assert_eq!(shown.table[3], (1, "1".to_string(), 2, "0".to_string()), "S1 on 1 goes to S2, saying 0");
        let draft = match c.apply(&root, Command::CircuitMachineShow { generation: g, name: None, text: Some("A -> ".into()) }).remove(0) {
            Event::CircuitMachine { machine, .. } => *machine,
            other => panic!("{other:?}"),
        };
        assert!(draft.error.is_some());
        let v = view(&c.apply(&root, Command::CircuitMachineBuild { generation: g, name: "ones".into(), flipflop: "jk".into(), circuit: "detector".into() })).unwrap();
        assert!(v.shown == "detector" && v.circuits.contains(&"detector_logic".to_string()));
        assert_eq!(v.parts.iter().filter(|p| p.kind == "jkff").count(), 2);
        let results = match c.apply(&root, Command::CircuitTestRun { generation: g, circuit: "detector".into(), label: None }).remove(0) {
            Event::CircuitTests { results, .. } => results,
            other => panic!("{other:?}"),
        };
        assert!(results[0].label == "machine" && results[0].failures.is_empty() && results[0].error.is_none(), "{results:?}");
        c.apply(&root, Command::CircuitSave);
        assert!(root.join("seq.circuit/machines/ones.fsm").exists() && root.join("seq.circuit/circuits/detector_logic.circ").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_circuit_is_written_out_and_a_dig_file_imported() {
        let root = scratch("export");
        let mut c = Circuits::default();
        let g = view(&c.apply(&root, Command::CircuitNew { name: "lab".into() })).unwrap().generation;
        let inv = "part A in at 0,0 label=A\npart N not at 2,0\npart Y out at 6,0 label=Y\nwire 0,0 2,0\nwire 4,0 6,0\n";
        c.apply(&root, Command::CircuitEdit { generation: g, circuit: "main".into(), ops: vec![CircuitOp::Paste { text: inv.into(), dx: 0, dy: 0 }] });
        for (format, ext, says) in [("verilog", "v", "module lab_main ("), ("vhdl", "vhd", "entity lab_main is"), ("svg", "svg", "<svg ")] {
            let events = c.apply(&root, Command::CircuitExport { generation: g, circuit: "main".into(), format: format.into() });
            let (path, text) = events
                .iter()
                .find_map(|e| match e {
                    Event::CircuitExported { path, text, .. } => Some((path.clone(), text.clone())),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{format}: {events:?}"));
            assert_eq!(path, format!("lab.circuit/export/main.{ext}"));
            assert!(text.contains(says), "{format}: {text}");
            assert_eq!(std::fs::read_to_string(root.join(&path)).unwrap(), text);
        }
        assert!(c.apply(&root, Command::CircuitExport { generation: g, circuit: "main".into(), format: "pdf".into() }).iter().any(|e| matches!(e, Event::Notice { .. })));
        c.apply(&root, Command::CircuitSave);
        // A Digital file in the folder: listed, and imported as a project beside it.
        let dig = "<?xml version=\"1.0\"?><circuit><version>2</version><attributes/><visualElements>\
            <visualElement><elementName>In</elementName><elementAttributes><entry><string>Label</string><string>A</string></entry></elementAttributes><pos x=\"0\" y=\"0\"/></visualElement>\
            <visualElement><elementName>Out</elementName><elementAttributes><entry><string>Label</string><string>Y</string></entry></elementAttributes><pos x=\"100\" y=\"0\"/></visualElement>\
            </visualElements><wires><wire><p1 x=\"0\" y=\"0\"/><p2 x=\"100\" y=\"0\"/></wire></wires></circuit>";
        std::fs::create_dir_all(root.join("course")).unwrap();
        std::fs::write(root.join("course/wire.dig"), dig).unwrap();
        let found = match &c.events(&root)[0] {
            Event::CircuitHome { projects } => projects.clone(),
            _ => unreachable!(),
        };
        assert!(found.iter().any(|f| f.path == "course/wire.dig" && f.kind == "dig"), "{found:?}");
        let v = view(&c.apply(&root, Command::CircuitImport { path: "course/wire.dig".into() })).unwrap();
        assert_eq!((v.path.as_str(), v.top.as_str(), v.parts.len()), ("course/wire.circuit", "wire", 2));
        assert!(root.join("course/wire.circuit/circuits/wire.circ").exists());
        c.apply(&root, Command::CircuitClose { discard: false });
        assert!(c.apply(&root, Command::CircuitImport { path: "course/wire.dig".into() }).iter().any(|e| matches!(e, Event::Notice { .. })), "it is there already");
        assert!(c.apply(&root, Command::CircuitImport { path: "../x.dig".into() }).iter().any(|e| matches!(e, Event::Notice { .. })));
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
