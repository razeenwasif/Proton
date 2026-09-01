//! Resolving a pointer to the thing it points at.
//!
//! **A pointer is an edge, never an inline expansion.** Every object appears
//! exactly once — in a frame or in a heap block — and a `ptr` carries only a
//! target address. Turning that address back into a rendered object is this
//! module, and it is the one rule the pane must not break: under inline
//! expansion a doubly linked list or a tree with parent pointers recurses
//! forever.
//!
//! The rule has two halves, and the second is the one that is easy to miss.
//! **Exact match first, then the smallest range containing the address** — so
//! `&node->next` lands on that field and not on the block it sits in, and
//! `&arr[2]` lands on the cell and not on the array. Resolving only exact
//! matches leaves half the arrows in a real program dangling; resolving to the
//! largest container points every arrow at the same box.

use std::collections::HashMap;

use crate::format::{address, Block, Frame, Step, Value, Var};

/// Something an address can land on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// A path the front end can address: `heap:0x7c8700`, `fib@3:n`,
    /// `heap:0x7c8700.next`.
    pub id: String,
    /// What to say about it in a tooltip.
    pub label: String,
    pub addr: u64,
    pub size: u64,
    pub kind: Where,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Frame,
    Heap,
    Global,
}

/// Every addressable object in one step, indexed for lookup.
#[derive(Debug, Default)]
pub struct Graph {
    /// Exact address to target, for the common case.
    exact: HashMap<u64, usize>,
    /// Everything with a size, so a mid-object address can be placed.
    ranged: Vec<usize>,
    targets: Vec<Target>,
    /// Addresses this step's `freed` list says are gone. A pointer into one is
    /// a dangling read, which is the thing the whole tool exists to show.
    freed: Vec<(u64, u64)>,
}

/// What a pointer's target turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// A null pointer. Not a failure — most lists end in one.
    Null,
    /// It lands on a live object.
    At {
        target: &'a Target,
        /// How far into it, for `&node->next` style edges.
        offset: u64,
    },
    /// It lands in memory this step already freed.
    Dangling { addr: u64 },
    /// It points somewhere nothing is drawn — a string literal, a stack frame
    /// that has returned, or a garbage value.
    Unknown { addr: u64 },
}

impl Graph {
    /// Index one step.
    pub fn of(step: &Step) -> Graph {
        let mut graph = Graph::default();

        for frame in &step.stack {
            for var in frame.args.iter().chain(&frame.locals) {
                graph.add_var(&frame.id, var, Where::Frame);
            }
        }
        for var in &step.globals {
            graph.add_var("global", var, Where::Global);
        }
        for block in &step.heap {
            graph.add_block(block);
        }

        graph.freed = step
            .freed
            .iter()
            .filter_map(|a| Some((address(&a.addr)?, a.size)))
            .collect();

        // Smallest last, so a linear scan can stop at the first hit and get
        // the tightest one. Ties break towards the later entry, which is the
        // heap block rather than the frame that pointed into it.
        graph
            .ranged
            .sort_by_key(|&i| std::cmp::Reverse(graph.targets[i].size));
        graph
    }

    fn add_var(&mut self, owner: &str, var: &Var, kind: Where) {
        self.walk(
            &format!("{owner}:{}", var.name),
            &var.name,
            &var.value,
            kind,
        );
    }

    fn add_block(&mut self, block: &Block) {
        let id = format!("heap:{}", block.addr);
        let label = match &block.elem_type {
            Some(t) => format!("{t} @ {}", block.addr),
            None => format!("{} bytes @ {}", block.size, block.addr),
        };
        // The block itself, then its contents, so a pointer to the allocation
        // and a pointer to its first field both land somewhere sensible.
        if let Some(addr) = address(&block.addr) {
            self.push(Target {
                id: id.clone(),
                label,
                addr,
                size: block.size,
                kind: Where::Heap,
            });
        }
        self.walk(&id, "", &block.value, Where::Heap);
    }

    /// Index a value and everything inside it.
    fn walk(&mut self, id: &str, name: &str, value: &Value, kind: Where) {
        if let (Some(addr), Some(size)) = (value.at().address(), value.at().size) {
            self.push(Target {
                id: id.to_string(),
                label: if name.is_empty() {
                    value.type_name().to_string()
                } else {
                    format!("{name}: {}", value.type_name())
                },
                addr,
                size,
                kind,
            });
        }

        match value {
            Value::Struct { fields, .. } | Value::Union { fields, .. } => {
                for field in fields {
                    self.walk(
                        &format!("{id}.{}", field.name),
                        &field.name,
                        &field.value,
                        kind,
                    );
                }
            }
            Value::Array { elems, .. } => {
                for (i, elem) in elems.iter().enumerate() {
                    self.walk(&format!("{id}[{i}]"), &format!("[{i}]"), elem, kind);
                }
            }
            _ => {}
        }
    }

    fn push(&mut self, target: Target) {
        let index = self.targets.len();
        // A field and its parent can share an address — a struct and its first
        // member both start at the same byte. First writer wins, and the walk
        // visits the outer one first, which is the one a bare pointer to the
        // allocation means.
        self.exact.entry(target.addr).or_insert(index);
        if target.size > 0 {
            self.ranged.push(index);
        }
        self.targets.push(target);
    }

    /// Follow an address.
    pub fn resolve(&self, target_addr: &str) -> Resolved<'_> {
        let Some(addr) = address(target_addr) else {
            return Resolved::Unknown { addr: 0 };
        };
        if addr == 0 {
            return Resolved::Null;
        }

        // Exact first. This is the overwhelming majority and it is a hash hit.
        if let Some(&index) = self.exact.get(&addr) {
            return Resolved::At {
                target: &self.targets[index],
                offset: 0,
            };
        }

        // Then the smallest range containing it, which is what makes
        // `&node->next` and `&arr[2]` land on the field and the cell rather
        // than on whatever encloses them.
        if let Some(&index) = self
            .ranged
            .iter()
            .rev()
            .find(|&&i| self.contains(&self.targets[i], addr))
        {
            let target = &self.targets[index];
            return Resolved::At {
                target,
                offset: addr - target.addr,
            };
        }

        // Nothing live. Freed memory is worth saying so about — it is the
        // difference between "I cannot draw this" and "this is a bug".
        if self.freed.iter().any(|&(at, size)| {
            addr == at || (size > 0 && addr >= at && addr < at.saturating_add(size))
        }) {
            return Resolved::Dangling { addr };
        }
        Resolved::Unknown { addr }
    }

    fn contains(&self, target: &Target, addr: u64) -> bool {
        addr > target.addr && addr < target.addr.saturating_add(target.size)
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

/// Every pointer in a step, paired with what it resolves to.
pub fn edges(step: &Step, graph: &Graph) -> Vec<Edge> {
    let mut out = Vec::new();
    for frame in &step.stack {
        for var in frame.args.iter().chain(&frame.locals) {
            collect(
                &format!("{}:{}", frame.id, var.name),
                &var.value,
                graph,
                &mut out,
            );
        }
    }
    for var in &step.globals {
        collect(&format!("global:{}", var.name), &var.value, graph, &mut out);
    }
    for block in &step.heap {
        collect(
            &format!("heap:{}", block.addr),
            &block.value,
            graph,
            &mut out,
        );
    }
    out
}

/// One arrow: where it starts, and where it lands.
#[derive(Debug, Clone)]
pub struct Edge {
    /// The pointer slot's own id.
    pub from: String,
    /// The slot's own address, which is where the arrow is drawn from.
    pub from_addr: Option<u64>,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Points,
    Null,
    /// Into freed memory.
    Dangling,
    /// Nothing drawn at the other end.
    Unknown,
}

fn collect(id: &str, value: &Value, graph: &Graph, out: &mut Vec<Edge>) {
    if let Value::Ptr { at, target, .. } = value {
        let (to, kind) = match graph.resolve(target) {
            Resolved::Null => (String::new(), EdgeKind::Null),
            Resolved::At { target, .. } => (target.id.clone(), EdgeKind::Points),
            Resolved::Dangling { addr } => (format!("0x{addr:x}"), EdgeKind::Dangling),
            Resolved::Unknown { addr } => (format!("0x{addr:x}"), EdgeKind::Unknown),
        };
        out.push(Edge {
            from: id.to_string(),
            from_addr: at.address(),
            to,
            kind,
        });
    }

    match value {
        Value::Struct { fields, .. } | Value::Union { fields, .. } => {
            for field in fields {
                collect(&format!("{id}.{}", field.name), &field.value, graph, out);
            }
        }
        Value::Array { elems, .. } => {
            for (i, elem) in elems.iter().enumerate() {
                collect(&format!("{id}[{i}]"), elem, graph, out);
            }
        }
        _ => {}
    }
}

/// A frame's id, for building target ids without reaching into the format.
pub fn frame_id(frame: &Frame) -> &str {
    &frame.id
}
