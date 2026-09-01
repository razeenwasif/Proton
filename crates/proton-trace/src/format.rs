//! The trace format — the contract between Code Visualizer's backends and
//! anything that draws them.
//!
//! Upstream states it plainly: *"Swapping in a TUI later means writing one
//! consumer of the same JSON, not rewriting anything."* Proton is that second
//! consumer, and this module is the whole of what it has to agree with. There
//! is no language-specific code here and there must never be — a backend's job
//! is to produce this, and a frontend's job is to not care which one did.
//!
//! Deserialise-only. Proton never writes a trace, it reads one; a struct that
//! could also emit this would be claiming to be a backend.

use serde::Deserialize;

/// One traced program.
#[derive(Debug, Clone, Deserialize)]
pub struct Trace {
    pub language: String,
    pub status: Status,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub stdin: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// Present unless `status` is `Ok`. For a compile error this is the
    /// compiler's raw output — the C backend does not populate `diagnostics`,
    /// whatever the format document implies, so this is the only text there is.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub warnings: Vec<Diagnostic>,
    #[serde(default)]
    pub double_free: bool,
    #[serde(default)]
    pub double_free_line: Option<u32>,
    /// Still allocated when the process exited — measured at exit, not at the
    /// last step, because at `return 0` a C++ `unique_ptr` still owns its block.
    #[serde(default)]
    pub leaks: Vec<Allocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Never ran.
    CompileError,
    /// Crashed. `steps` holds everything up to and including it.
    RuntimeError,
    /// The watchdog fired; `steps` is partial.
    Timeout,
    /// Hit the step cap; `steps` is partial.
    StepLimit,
    Unsupported,
    InternalError,
    /// A status the backend added after this was written. Rendering whatever
    /// steps came with it beats refusing to parse the run.
    #[serde(other)]
    Unknown,
}

impl Status {
    /// Whether the run produced steps worth drawing.
    ///
    /// Everything except a compile error may carry useful steps, so the pane
    /// renders what it got and reports `error` beside it rather than showing
    /// an empty panel with a message.
    pub fn has_steps(self) -> bool {
        self != Status::CompileError
    }

    pub fn is_failure(self) -> bool {
        self != Status::Ok
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Diagnostic {
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub column: Option<u32>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub severity: Option<String>,
}

/// A heap allocation, as reported in `freed` and `leaks`.
#[derive(Debug, Clone, Deserialize)]
pub struct Allocation {
    pub addr: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// One executed source line, captured **before** it runs.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub step: u32,
    #[serde(default)]
    pub event: Event,
    pub line: u32,
    #[serde(default)]
    pub func: String,
    /// Innermost first.
    #[serde(default)]
    pub stack: Vec<Frame>,
    #[serde(default)]
    pub globals: Vec<Var>,
    #[serde(default)]
    pub heap: Vec<Block>,
    #[serde(default)]
    pub freed: Vec<Allocation>,
    /// Calls that finished since the previous step, innermost first. This is
    /// what makes the call tree exact — depth alone cannot separate
    /// `f(a) + f(b)` on one line.
    #[serde(default)]
    pub returns: Vec<Return>,
    /// Byte offset into the top-level `stdout`, which is how output can be
    /// shown accumulating without storing a copy per step.
    #[serde(default)]
    pub stdout_len: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    #[default]
    Step,
    /// Where a crash happened. The last step of a `runtime_error` run.
    Signal,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Return {
    #[serde(default)]
    pub func: String,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Frame {
    /// Stable across steps: name plus depth.
    pub id: String,
    #[serde(default)]
    pub func: String,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub args: Vec<Var>,
    #[serde(default)]
    pub locals: Vec<Var>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Var {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub addr: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub seq: u32,
    #[serde(default)]
    pub kind: Option<String>,
    pub value: Value,
    #[serde(default)]
    pub elem_type: Option<String>,
    #[serde(default)]
    pub count: Option<u64>,
    /// `false` means the type is unknown and the payload is raw bytes.
    #[serde(default)]
    pub typed: bool,
    /// Where the payload starts inside the allocation. `make_shared` puts a
    /// control block in front, so the only pointer anyone holds aims into the
    /// middle of the block.
    #[serde(default)]
    pub offset: u64,
}

/// A value, tagged by `kind`.
///
/// `addr` and `size` are on every variant that can supply them, and they are
/// what make the pointer graph work — see [`crate::resolve`].
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Prim {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        repr: String,
    },
    /// An **edge**, never an inline expansion. Carries where it points and
    /// nothing about what is there.
    Ptr {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        target: String,
        /// Only when null.
        #[serde(default)]
        repr: Option<String>,
        /// Only for `char *`.
        #[serde(default)]
        str: Option<String>,
        #[serde(default)]
        is_func: bool,
        /// A C++ reference: an edge with no storage of its own.
        #[serde(default)]
        is_ref: bool,
        #[serde(default)]
        smart: Option<String>,
        #[serde(default)]
        dynamic_type: Option<String>,
    },
    String {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        str: String,
        #[serde(default)]
        capacity: Option<u64>,
    },
    Struct {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        summary: Option<String>,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Union {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Array {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        elems: Vec<Value>,
        #[serde(default)]
        count: Option<u64>,
        #[serde(default)]
        truncated: bool,
    },
    /// Allocated, type unknown.
    Bytes {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
    },
    /// Declared but not yet reached. Deliberately **not** decoded: the bytes
    /// are whatever was on the stack, and a garbage value that happened to
    /// look like a pointer would pollute the graph.
    Uninit {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        r#type: String,
        #[serde(default)]
        decl_line: Option<u32>,
    },
    Error {
        #[serde(flatten)]
        at: At,
        #[serde(default)]
        msg: String,
    },
    /// A kind added after this was written.
    #[serde(other)]
    Unknown,
}

/// Where a value lives, and how much room it takes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct At {
    #[serde(default)]
    pub addr: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    #[serde(default)]
    pub name: String,
    pub value: Value,
    /// A C++ base subobject rather than a declared member.
    #[serde(default)]
    pub is_base: bool,
}

impl Value {
    pub fn at(&self) -> &At {
        match self {
            Value::Prim { at, .. }
            | Value::Ptr { at, .. }
            | Value::String { at, .. }
            | Value::Struct { at, .. }
            | Value::Union { at, .. }
            | Value::Array { at, .. }
            | Value::Bytes { at, .. }
            | Value::Uninit { at, .. }
            | Value::Error { at, .. } => at,
            Value::Unknown => &EMPTY,
        }
    }

    /// The declared type, for display.
    pub fn type_name(&self) -> &str {
        match self {
            Value::Prim { r#type, .. }
            | Value::Ptr { r#type, .. }
            | Value::String { r#type, .. }
            | Value::Struct { r#type, .. }
            | Value::Union { r#type, .. }
            | Value::Array { r#type, .. }
            | Value::Bytes { r#type, .. }
            | Value::Uninit { r#type, .. } => r#type,
            _ => "",
        }
    }

    /// Immediate children, for walking a value tree.
    pub fn children(&self) -> Vec<&Value> {
        match self {
            Value::Struct { fields, .. } | Value::Union { fields, .. } => {
                fields.iter().map(|f| &f.value).collect()
            }
            Value::Array { elems, .. } => elems.iter().collect(),
            _ => Vec::new(),
        }
    }
}

static EMPTY: At = At {
    addr: None,
    size: None,
};

/// Parse a hex address as the backends write them: `0x7c8700`, or `0x0`.
pub fn address(text: &str) -> Option<u64> {
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    u64::from_str_radix(hex, 16).ok()
}

impl At {
    pub fn address(&self) -> Option<u64> {
        self.addr.as_deref().and_then(address)
    }
}
