//! A flow: the graph a person builds, as its `.flow.json` holds it.
//!
//! Flat lists of nodes and links, as Model mode's shader graphs keep them
//! and as a run sends them, so the file, the editor and the server all speak
//! one shape. A port is `node:name`; a link goes from an output port to an
//! input port. What the file adds to a run's graph is what only the editor
//! needs: where each node sits, what its seed does after a run, which nodes
//! are bypassed, and a note.
//!
//! ```json
//! {
//!   "format": 1,
//!   "nodes": [
//!     {"id": "1", "type": "LoadCheckpoint", "at": [16, 16], "values": {"checkpoint": "sd_xl_base_1.0.safetensors"}},
//!     {"id": "5", "type": "KSampler", "at": [408, 16], "values": {"seed": 421337, "steps": 30}, "seed": "fixed"}
//!   ],
//!   "links": [{"from": "1:model", "to": "5:model"}]
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::catalogue::Catalogue;
use crate::check::{check, Problem};

/// The format this Proton writes, and the latest it reads.
pub const FORMAT: u32 = 1;

/// A `.flow.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseFlow")]
pub struct Flow {
    pub format: u32,
    /// What the flow is for, in the person's words.
    #[serde(default)]
    pub note: Option<String>,
    /// The project folder its outputs go to; `outputs/<flow name>/` when not
    /// said.
    #[serde(default)]
    pub outputs: Option<String>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
}

/// One node in a flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseNode")]
pub struct Node {
    pub id: String,
    /// Its type in the catalogue, such as `KSampler`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Where it sits on the graph: its top left corner.
    pub at: [f64; 2],
    /// Its fields' values, by input name. A field with none takes the
    /// catalogue's default when the graph is sent.
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub values: BTreeMap<String, Value>,
    /// What its seed does after each run; fixed when not said.
    #[serde(default)]
    pub seed: Option<SeedMode>,
    /// Left out of a run, its inputs passed straight through. Written only
    /// when on: the file's writer leaves out what is null, and a `false` on
    /// every node would be a line of nothing on every node.
    #[serde(default)]
    #[ts(type = "boolean | null")]
    pub bypass: Flag,
}

/// What a node's seed does after a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseSeedMode")]
pub enum SeedMode {
    Fixed,
    Increment,
    Random,
}

/// A link from an output port to an input port, each `node:name`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseLink")]
pub struct Link {
    pub from: String,
    pub to: String,
}

/// A port, read from `node:name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub node: String,
    pub name: String,
}

/// What a run sends: the nodes it needs with every value filled, and the
/// links between them — the design's `POST /runs` body's `graph`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseGraph")]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseGraphNode")]
pub struct GraphNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[ts(type = "Record<string, unknown>")]
    pub values: BTreeMap<String, Value>,
}

/// A switch that is off unless said: written as `true`, or not at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "Option<bool>")]
pub struct Flag(pub bool);

impl From<Option<bool>> for Flag {
    fn from(on: Option<bool>) -> Flag {
        Flag(on.unwrap_or(false))
    }
}

impl Serialize for Flag {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            true => out.serialize_bool(true),
            false => out.serialize_none(),
        }
    }
}

impl Port {
    /// `node:name`; neither part empty. A node's id never holds a colon, so
    /// the first one splits it.
    pub fn parse(text: &str) -> Option<Port> {
        let (node, name) = text.split_once(':')?;
        match node.is_empty() || name.is_empty() {
            true => None,
            false => Some(Port { node: node.to_string(), name: name.to_string() }),
        }
    }

    pub fn text(&self) -> String {
        format!("{}:{}", self.node, self.name)
    }
}

impl Default for Flow {
    fn default() -> Flow {
        Flow { format: FORMAT, note: None, outputs: None, nodes: Vec::new(), links: Vec::new() }
    }
}

impl Flow {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// The link into input port `to`, if there is one.
    pub fn link_into(&self, to: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to == to)
    }

    /// An id no node has: one more than the largest number among them, so
    /// ids read as the order nodes were added.
    pub fn fresh_id(&self) -> String {
        let top = self.nodes.iter().filter_map(|n| n.id.parse::<u64>().ok()).max().unwrap_or(0);
        let mut next = top + 1;
        while self.node(&next.to_string()).is_some() {
            next += 1;
        }
        next.to_string()
    }

    /// Whether a link from node `from` into node `to` would close a loop:
    /// whether `from` already depends on `to`.
    pub fn would_cycle(&self, from: &str, to: &str) -> bool {
        if from == to {
            return true;
        }
        // Walk upstream from `from`; reaching `to` means `to` feeds it.
        let mut seen = BTreeSet::new();
        let mut todo = vec![from.to_string()];
        while let Some(at) = todo.pop() {
            if at == to {
                return true;
            }
            if !seen.insert(at.clone()) {
                continue;
            }
            for link in &self.links {
                if let (Some(f), Some(t)) = (Port::parse(&link.from), Port::parse(&link.to)) {
                    if t.node == at {
                        todo.push(f.node);
                    }
                }
            }
        }
        false
    }

    /// The flow as it runs: bypassed nodes taken out, each link that came
    /// from one moved back to what fed it — the bypassed node's first linked
    /// input of the same socket type — or dropped when nothing did.
    pub fn without_bypassed(&self, catalogue: &Catalogue) -> Flow {
        let bypassed: BTreeSet<&str> = self.nodes.iter().filter(|n| n.bypass.0).map(|n| n.id.as_str()).collect();
        if bypassed.is_empty() {
            return self.clone();
        }
        let mut out = self.clone();
        out.nodes.retain(|n| !n.bypass.0);
        out.links = self
            .links
            .iter()
            .filter(|l| Port::parse(&l.to).is_some_and(|t| !bypassed.contains(t.node.as_str())))
            .filter_map(|l| {
                let from = self.source(&l.from, catalogue, &bypassed)?;
                Some(Link { from, to: l.to.clone() })
            })
            .collect();
        out
    }

    /// What really feeds `from`, following bypassed nodes back.
    fn source(&self, from: &str, catalogue: &Catalogue, bypassed: &BTreeSet<&str>) -> Option<String> {
        let mut port = Port::parse(from)?;
        // A bypass chain is at most every node long; a longer walk is a loop.
        for _ in 0..=self.nodes.len() {
            if !bypassed.contains(port.node.as_str()) {
                return Some(port.text());
            }
            let node = self.node(&port.node)?;
            let kind = catalogue.node(&node.kind)?;
            let socket = &kind.output(&port.name)?.socket;
            let input = kind.inputs.iter().find(|i| {
                i.socket.as_ref() == Some(socket) && self.link_into(&format!("{}:{}", node.id, i.name)).is_some()
            })?;
            let link = self.link_into(&format!("{}:{}", node.id, input.name))?;
            port = Port::parse(&link.from)?;
        }
        None
    }

    /// The nodes a run needs: every output node that is not bypassed, and
    /// all that feeds them.
    pub fn needed(&self, catalogue: &Catalogue) -> BTreeSet<String> {
        let mut todo: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| !n.bypass.0 && catalogue.node(&n.kind).is_some_and(|k| k.is_output()))
            .map(|n| n.id.clone())
            .collect();
        let mut needed = BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !needed.insert(id.clone()) {
                continue;
            }
            for link in &self.links {
                if let (Some(f), Some(t)) = (Port::parse(&link.from), Port::parse(&link.to)) {
                    if t.node == id {
                        todo.push(f.node);
                    }
                }
            }
        }
        needed
    }

    /// The graph a run sends, or what stops it running.
    pub fn graph(&self, catalogue: &Catalogue, models: Option<&crate::Models>) -> Result<Graph, Vec<Problem>> {
        let problems = check(self, catalogue, models);
        if !problems.is_empty() {
            return Err(problems);
        }
        let runs = self.without_bypassed(catalogue);
        let needed = runs.needed(catalogue);
        let nodes = runs
            .nodes
            .iter()
            .filter(|n| needed.contains(&n.id))
            .map(|n| {
                let mut values = n.values.clone();
                // check has seen every needed node's type.
                if let Some(kind) = catalogue.node(&n.kind) {
                    for field in kind.fields() {
                        if let (false, Some(default)) = (values.contains_key(&field.name), &field.default) {
                            values.insert(field.name.clone(), default.clone());
                        }
                    }
                }
                GraphNode { id: n.id.clone(), kind: n.kind.clone(), values }
            })
            .collect();
        let links = runs
            .links
            .iter()
            .filter(|l| Port::parse(&l.to).is_some_and(|t| needed.contains(&t.node)))
            .cloned()
            .collect();
        Ok(Graph { nodes, links })
    }
}
