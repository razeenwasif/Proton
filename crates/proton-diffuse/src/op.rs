//! Every edit to a flow, as a value.
//!
//! The window sends a gesture as a batch — a drag of three nodes is three
//! `place`s, a node dropped on a wire is an `add` and two `connect`s — and
//! the batch is applied to a copy, whole or not at all, so it is one undo
//! step or nothing. Ops keep the graph sound: a link joins ports that exist
//! and carry the same socket type, an input has at most one link, and no
//! link closes a loop. They do not insist a graph is finished — a node can
//! sit with its inputs unlinked while it is being built; [`crate::check`]
//! says what stops it running.
//!
//! Ops name nodes by id, never by place in the list, so an edit on its way
//! while another moved things still lands where it was meant to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::catalogue::{Catalogue, Kind};
use crate::check::link_problem;
use crate::flow::{Flag, Flow, Link, Node, Port, SeedMode};

/// One edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseOp")]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// A new node of `type` at `at`, its fields set from `values` and the
    /// rest from the catalogue's defaults. Its id is chosen here and said in
    /// [`Applied::added`].
    Add {
        #[serde(rename = "type")]
        kind: String,
        at: [f64; 2],
        #[serde(default)]
        #[ts(type = "Record<string, unknown>")]
        values: BTreeMap<String, Value>,
    },
    /// Take nodes away, and every link to or from them.
    Remove { ids: Vec<String> },
    /// Put a node at `at`.
    Place { id: String, at: [f64; 2] },
    /// Set a field; `null` takes the value away, back to the default.
    Set {
        id: String,
        name: String,
        #[ts(type = "unknown")]
        value: Value,
    },
    /// Link output port `from` to input port `to`, in place of any link
    /// into `to` already.
    Connect { from: String, to: String },
    /// Take away the link into input port `to`.
    Disconnect { to: String },
    /// What a node's seed does after each run.
    Seed { id: String, mode: SeedMode },
    /// Leave a node out of runs, or put it back.
    Bypass { id: String, on: bool },
    /// Copies of nodes, `by` away from them, with the links among them.
    Duplicate { ids: Vec<String>, by: [f64; 2] },
    /// The flow's note; none takes it away.
    Note {
        #[ts(optional)]
        text: Option<String>,
    },
    /// The project folder outputs go to; none is the default.
    Outputs {
        #[ts(optional)]
        path: Option<String>,
    },
}

/// What a batch did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseApplied")]
pub struct Applied {
    /// Whether the flow is different now; a batch that changed nothing is
    /// not an undo step.
    pub changed: bool,
    /// The ids of the nodes added, in the order their ops came.
    pub added: Vec<String>,
}

/// Apply `ops` to `flow`, all or — when any cannot be done — none, and why.
pub fn apply(flow: &mut Flow, ops: &[Op], catalogue: &Catalogue) -> Result<Applied, String> {
    let mut next = flow.clone();
    let mut added = Vec::new();
    for op in ops {
        apply_one(&mut next, op, catalogue, &mut added)?;
    }
    let changed = next != *flow;
    *flow = next;
    Ok(Applied { changed, added })
}

fn apply_one(flow: &mut Flow, op: &Op, catalogue: &Catalogue, added: &mut Vec<String>) -> Result<(), String> {
    match op {
        Op::Add { kind, at, values } => {
            let Some(node_type) = catalogue.node(kind) else {
                return Err(format!("the server has no {kind} node"));
            };
            let id = flow.fresh_id();
            let mut node = Node {
                id: id.clone(),
                kind: kind.clone(),
                at: place(*at)?,
                values: BTreeMap::new(),
                seed: None,
                bypass: Flag(false),
            };
            for (name, value) in values {
                set_value(&mut node, catalogue, name, value)?;
            }
            for field in node_type.fields() {
                if node.values.contains_key(&field.name) {
                    continue;
                }
                match (&field.default, field.kind()) {
                    (Some(default), _) => {
                        node.values.insert(field.name.clone(), default.clone());
                    }
                    // A seed is a number before it is anything; the window
                    // sends a random one.
                    (None, Kind::Seed) => {
                        node.values.insert(field.name.clone(), Value::from(0));
                    }
                    _ => {}
                }
            }
            flow.nodes.push(node);
            added.push(id);
        }
        Op::Remove { ids } => {
            for id in ids {
                if flow.node(id).is_none() {
                    return Err(format!("there is no node {id}"));
                }
            }
            let gone: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
            flow.nodes.retain(|n| !gone.contains(n.id.as_str()));
            flow.links.retain(|l| {
                let touches = |port: &str| Port::parse(port).is_some_and(|p| gone.contains(p.node.as_str()));
                !touches(&l.from) && !touches(&l.to)
            });
        }
        Op::Place { id, at } => {
            let at = place(*at)?;
            node_mut(flow, id)?.at = at;
        }
        Op::Set { id, name, value } => {
            let node = node_mut(flow, id)?;
            match value.is_null() {
                true => {
                    node.values.remove(name);
                }
                false => set_value(node, catalogue, name, value)?,
            }
        }
        Op::Connect { from, to } => {
            let (Some(source), Some(target)) = (Port::parse(from), Port::parse(to)) else {
                return Err(format!("{from} → {to} is not a link between two ports"));
            };
            if let Some(why) = link_problem(flow, catalogue, &source, &target) {
                return Err(why);
            }
            // Between nodes the server lacks, nothing says what fits.
            for id in [&source.node, &target.node] {
                let kind = flow.node(id).map(|n| n.kind.as_str()).unwrap_or_default();
                if catalogue.node(kind).is_none() {
                    return Err(format!("the server has no {kind} node"));
                }
            }
            if flow.would_cycle(&source.node, &target.node) {
                return Err(format!("linking {from} to {to} would make a loop"));
            }
            flow.links.retain(|l| &l.to != to);
            flow.links.push(Link { from: from.clone(), to: to.clone() });
        }
        Op::Disconnect { to } => flow.links.retain(|l| &l.to != to),
        Op::Seed { id, mode } => {
            node_mut(flow, id)?.seed = match mode {
                SeedMode::Fixed => None,
                other => Some(*other),
            };
        }
        Op::Bypass { id, on } => node_mut(flow, id)?.bypass = Flag(*on),
        Op::Duplicate { ids, by } => {
            let mut copies: BTreeMap<String, String> = BTreeMap::new();
            for id in ids {
                let Some(node) = flow.node(id).cloned() else {
                    return Err(format!("there is no node {id}"));
                };
                if copies.contains_key(id) {
                    continue;
                }
                let fresh = flow.fresh_id();
                let at = place([node.at[0] + by[0], node.at[1] + by[1]])?;
                flow.nodes.push(Node { id: fresh.clone(), at, ..node });
                copies.insert(id.clone(), fresh.clone());
                added.push(fresh);
            }
            let between: Vec<Link> = flow
                .links
                .iter()
                .filter_map(|l| {
                    let (f, t) = (Port::parse(&l.from)?, Port::parse(&l.to)?);
                    let (nf, nt) = (copies.get(&f.node)?, copies.get(&t.node)?);
                    Some(Link { from: format!("{nf}:{}", f.name), to: format!("{nt}:{}", t.name) })
                })
                .collect();
            flow.links.extend(between);
        }
        Op::Note { text } => flow.note = text.clone().filter(|t| !t.trim().is_empty()),
        Op::Outputs { path } => {
            flow.outputs = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                None => None,
                Some(path) => Some(plain_folder(path)?),
            };
        }
    }
    Ok(())
}

fn node_mut<'f>(flow: &'f mut Flow, id: &str) -> Result<&'f mut Node, String> {
    flow.node_mut(id).ok_or_else(|| format!("there is no node {id}"))
}

/// A place on the graph: finite numbers only.
fn place(at: [f64; 2]) -> Result<[f64; 2], String> {
    match at.iter().all(|v| v.is_finite()) {
        true => Ok(at),
        false => Err("a node's place is two numbers".into()),
    }
}

/// Set `name` on `node`, refused unless its type has that field and the
/// value is of the field's kind.
fn set_value(node: &mut Node, catalogue: &Catalogue, name: &str, value: &Value) -> Result<(), String> {
    let Some(kind) = catalogue.node(&node.kind) else {
        return Err(format!("the server has no {} node", node.kind));
    };
    let Some(field) = kind.fields().find(|f| f.name == name) else {
        return Err(match kind.input(name) {
            Some(_) => format!("{name} is linked, not set"),
            None => format!("{} has no field {name}", kind.title),
        });
    };
    if let Some(why) = field.refuse(value) {
        return Err(why);
    }
    node.values.insert(name.to_string(), value.clone());
    Ok(())
}

/// A folder inside the project, as `/`-separated plain parts.
fn plain_folder(path: &str) -> Result<String, String> {
    let trimmed = path.trim_end_matches('/');
    let plain = !trimmed.is_empty()
        && !trimmed.contains('\\')
        && !trimmed.contains('\0')
        && Path::new(trimmed).components().all(|c| matches!(c, Component::Normal(_)));
    match plain {
        true => Ok(trimmed.to_string()),
        false => Err(format!("{path} is not a folder inside the project")),
    }
}
