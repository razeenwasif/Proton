//! The Visualizer, as the editor sees it.
//!
//! `proton-trace` owns the tracer process and the format. This owns which run
//! is open, which step you are on, and turning that one step into something
//! the pane can draw.
//!
//! **One step at a time crosses the bridge.** A 63-step linked list is 178 KB
//! of JSON; a thousand-step run is megabytes, and none of it is on screen at
//! once. This is the same discipline the editor uses for text — the core sends
//! the window you can see and nothing more — and it is what keeps stepping
//! through a long trace as cheap as stepping through a short one.

use std::path::PathBuf;

use proton_trace::format::{Status, Step, Value};
use proton_trace::{edges, EdgeKind, Graph, Reply, Run, Trace, Tracer};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/* ── what the pane draws ──────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum TraceState {
    /// Nothing has been traced.
    Empty,
    /// The tracer is starting, compiling, or running.
    Running,
    Ready,
    Failed,
}

/// One value, flattened for display.
///
/// The tree is kept — a struct's fields are its children — because the pane
/// draws nested boxes, but every node carries its own address so the arrows
/// can be measured between the exact slots rather than between containers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    /// A stable path: `fib@3:n`, `heap:0x7c8700.next`.
    pub id: String,
    pub name: String,
    pub type_name: String,
    /// What to show: `42`, `NULL`, `"hello"`, or empty for a container.
    pub repr: String,
    /// The value's own address, which is where an arrow leaves from.
    pub addr: Option<String>,
    /// Set when this slot is a pointer, so the pane knows to draw an edge.
    pub points_to: Option<String>,
    pub children: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FrameView {
    pub id: String,
    pub func: String,
    pub line: u32,
    pub depth: u32,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BlockView {
    pub id: String,
    pub addr: String,
    pub size: u64,
    /// The source line that allocated it.
    pub line: Option<u32>,
    pub type_name: String,
    pub slots: Vec<Slot>,
    /// Freed at this step. Still drawn, because the arrow into it is the bug.
    pub freed: bool,
}

/// One arrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct EdgeView {
    pub from: String,
    pub to: String,
    pub kind: EdgeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum EdgeState {
    Points,
    Null,
    /// Into freed memory — the thing the tool exists to show.
    Dangling,
    Unknown,
}

/// Everything needed to draw one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub index: usize,
    pub line: u32,
    pub func: String,
    /// The crash, when this is where it happened.
    pub signal: bool,
    pub frames: Vec<FrameView>,
    pub globals: Vec<Slot>,
    pub heap: Vec<BlockView>,
    pub edges: Vec<EdgeView>,
    /// Everything the program had printed by this step — the format carries a
    /// byte offset rather than a copy per step, which is how output can be
    /// shown accumulating without storing it a thousand times.
    pub stdout: String,
}

/* ── the state the workspace holds ────────────────────────────────── */

pub struct Visualizer {
    root: PathBuf,
    tracer: Option<Tracer>,
    run: Option<Run>,
    trace: Option<Trace>,
    step: usize,
    state: TraceState,
    message: String,
    /// What was traced, for the pane's header.
    subject: String,
}

impl std::fmt::Debug for Visualizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Visualizer")
            .field("state", &self.state)
            .field("step", &self.step)
            .field("steps", &self.trace.as_ref().map_or(0, |t| t.steps.len()))
            .finish()
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Visualizer {
            root: PathBuf::new(),
            tracer: None,
            run: None,
            trace: None,
            step: 0,
            state: TraceState::Empty,
            message: String::new(),
            subject: String::new(),
        }
    }
}

impl Visualizer {
    pub fn new(root: impl Into<PathBuf>) -> Visualizer {
        Visualizer {
            root: root.into(),
            ..Visualizer::default()
        }
    }

    pub fn state(&self) -> TraceState {
        self.state
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn step_index(&self) -> usize {
        self.step
    }

    pub fn steps(&self) -> usize {
        self.trace.as_ref().map_or(0, |t| t.steps.len())
    }

    pub fn is_busy(&self) -> bool {
        self.run.is_some()
    }

    /// Trace a file. The tracer is started on first use rather than at boot —
    /// it compiles code, and most sessions never ask it to.
    pub fn trace(&mut self, path: &std::path::Path, source: &str) -> Result<(), String> {
        let Some(language) = Tracer::language_for(path) else {
            return Err(format!(
                "no tracer for {} — C, C++, Python and TypeScript are supported",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        };

        if self.tracer.is_none() {
            let checkout = proton_trace::server::vendored(&self.root);
            self.tracer = Some(Tracer::start(&checkout)?);
        }
        let tracer = self.tracer.as_ref().expect("just started");
        tracer.readiness(language)?;

        self.run = Some(tracer.trace(language, source));
        self.state = TraceState::Running;
        self.subject = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.message = format!("tracing {}…", self.subject);
        Ok(())
    }

    /// Drain the run. `true` when something changed.
    pub fn poll(&mut self) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let replies = run.poll();
        let finished = run.is_done();
        let mut changed = false;

        for reply in replies {
            match reply {
                Reply::Started => {}
                Reply::Finished(trace) => {
                    self.state = if trace.status.has_steps() && !trace.steps.is_empty() {
                        TraceState::Ready
                    } else {
                        TraceState::Failed
                    };
                    // A partial run is still worth showing. `timeout` and
                    // `runtime_error` carry every step up to the point they
                    // stopped, and those are the interesting ones.
                    self.message = match (&trace.error, trace.status) {
                        (Some(error), _) => first_line(error),
                        (None, Status::Ok) => format!("{} steps", trace.steps.len()),
                        (None, status) => format!("{status:?}"),
                    };
                    self.step = 0;
                    self.trace = Some(*trace);
                    changed = true;
                }
                Reply::Failed { text } => {
                    self.state = TraceState::Failed;
                    self.message = text;
                    changed = true;
                }
                Reply::Cancelled => {
                    self.state = TraceState::Empty;
                    self.message = "cancelled".into();
                    changed = true;
                }
            }
        }
        if finished {
            self.run = None;
        }
        changed
    }

    /// Move to a step, clamped. `true` when it moved.
    pub fn seek(&mut self, to: usize) -> bool {
        let last = self.steps().saturating_sub(1);
        let to = to.min(last);
        let moved = to != self.step;
        self.step = to;
        moved
    }

    pub fn nudge(&mut self, by: i64) -> bool {
        let to = (self.step as i64 + by).max(0) as usize;
        self.seek(to)
    }

    /// The source line the current step is about to execute, so the editor can
    /// follow along.
    pub fn current_line(&self) -> Option<u32> {
        self.trace.as_ref()?.steps.get(self.step).map(|s| s.line)
    }

    /// The current step, rendered.
    pub fn view(&self) -> Option<StepView> {
        let trace = self.trace.as_ref()?;
        let step = trace.steps.get(self.step)?;
        Some(render(trace, step, self.step))
    }
}

/* ── rendering one step ───────────────────────────────────────────── */

fn render(trace: &Trace, step: &Step, index: usize) -> StepView {
    let graph = Graph::of(step);
    let freed: Vec<&str> = step.freed.iter().map(|a| a.addr.as_str()).collect();

    let frames = step
        .stack
        .iter()
        .map(|frame| FrameView {
            id: frame.id.clone(),
            func: frame.func.clone(),
            line: frame.line,
            depth: frame.depth,
            slots: frame
                .args
                .iter()
                .chain(&frame.locals)
                .map(|var| slot(&format!("{}:{}", frame.id, var.name), &var.name, &var.value))
                .collect(),
        })
        .collect();

    let heap = step
        .heap
        .iter()
        .map(|block| {
            let id = format!("heap:{}", block.addr);
            BlockView {
                slots: match &block.value {
                    Value::Struct { fields, .. } => fields
                        .iter()
                        .map(|f| slot(&format!("{id}.{}", f.name), &f.name, &f.value))
                        .collect(),
                    other => vec![slot(&id, "", other)],
                },
                type_name: block
                    .elem_type
                    .clone()
                    .unwrap_or_else(|| block.value.type_name().to_string()),
                freed: freed.contains(&block.addr.as_str()),
                id,
                addr: block.addr.clone(),
                size: block.size,
                line: block.line,
            }
        })
        .collect();

    let edges = edges(step, &graph)
        .into_iter()
        .map(|edge| EdgeView {
            from: edge.from,
            to: edge.to,
            kind: match edge.kind {
                EdgeKind::Points => EdgeState::Points,
                EdgeKind::Null => EdgeState::Null,
                EdgeKind::Dangling => EdgeState::Dangling,
                EdgeKind::Unknown => EdgeState::Unknown,
            },
        })
        .collect();

    StepView {
        index,
        line: step.line,
        func: step.func.clone(),
        signal: step.event == proton_trace::format::Event::Signal,
        frames,
        globals: step
            .globals
            .iter()
            .map(|var| slot(&format!("global:{}", var.name), &var.name, &var.value))
            .collect(),
        heap,
        edges,
        // The byte offset into the whole output, which is how a step shows
        // what had been printed by then without storing a copy.
        stdout: trace
            .stdout
            .as_bytes()
            .get(..step.stdout_len.min(trace.stdout.len()))
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default(),
    }
}

fn slot(id: &str, name: &str, value: &Value) -> Slot {
    let (repr, points_to) = match value {
        Value::Prim { repr, .. } => (repr.clone(), None),
        Value::Ptr { target, repr, .. } => (
            repr.clone().unwrap_or_else(|| target.clone()),
            Some(target.clone()),
        ),
        Value::String { str, .. } => (format!("{str:?}"), None),
        Value::Uninit { .. } => ("?".into(), None),
        Value::Error { msg, .. } => (msg.clone(), None),
        Value::Bytes { .. } => (String::new(), None),
        _ => (String::new(), None),
    };

    let children = match value {
        Value::Struct { fields, .. } | Value::Union { fields, .. } => fields
            .iter()
            .map(|f| slot(&format!("{id}.{}", f.name), &f.name, &f.value))
            .collect(),
        Value::Array { elems, .. } => elems
            .iter()
            .enumerate()
            .map(|(i, e)| slot(&format!("{id}[{i}]"), &format!("[{i}]"), e))
            .collect(),
        _ => Vec::new(),
    };

    Slot {
        id: id.to_string(),
        name: name.to_string(),
        type_name: value.type_name().to_string(),
        repr,
        addr: value.at().addr.clone(),
        points_to,
        children,
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}
