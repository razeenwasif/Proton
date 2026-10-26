//! What stops a flow running, node by node — what the editor marks on the
//! graph and what keeps Run from sending a graph the server would refuse.
//!
//! Two kinds of trouble, checked over two sets of nodes:
//!
//! - **A broken graph**, wherever it is: two nodes with one id, a type the
//!   server does not serve, a link from a port that is not there, between
//!   socket types that differ, a second link into one input, a loop. Edits
//!   never make these (see [`crate::op`]); a file edited by hand, or a
//!   catalogue that changed, can.
//! - **An unfinished node**, only among the nodes a run needs — those that
//!   feed an output, bypassed ones taken out: an input with no link, a field
//!   with no value and no default, a value of the wrong kind or out of its
//!   range, a choice that is not on its list. A half-built node off to one
//!   side does not stop the rest running, as in ComfyUI.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalogue::{Catalogue, Kind, Models};
use crate::flow::{Flow, Port};

/// One thing wrong, at a node and one of its inputs when it is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseProblem")]
pub struct Problem {
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    pub message: String,
}

impl Problem {
    fn flow(message: impl Into<String>) -> Problem {
        Problem { node: None, input: None, message: message.into() }
    }

    fn node(node: &str, message: impl Into<String>) -> Problem {
        Problem { node: Some(node.to_string()), input: None, message: message.into() }
    }

    fn input(node: &str, input: &str, message: impl Into<String>) -> Problem {
        Problem { node: Some(node.to_string()), input: Some(input.to_string()), message: message.into() }
    }
}

/// Everything that stops `flow` running on a server with this catalogue.
/// `models` is the server's model lists when they are known; without them a
/// model's name is not checked against what the server has.
pub fn check(flow: &Flow, catalogue: &Catalogue, models: Option<&Models>) -> Vec<Problem> {
    let mut problems = broken(flow, catalogue);
    let runs = flow.without_bypassed(catalogue);
    let needed = runs.needed(catalogue);
    if needed.is_empty() {
        problems.push(Problem::flow("nothing to run: no output node such as Save Image or Preview Image"));
    }
    for node in runs.nodes.iter().filter(|n| needed.contains(&n.id)) {
        let Some(kind) = catalogue.node(&node.kind) else {
            continue;
        };
        for input in &kind.inputs {
            if input.is_socket() {
                let port = format!("{}:{}", node.id, input.name);
                if !input.optional && runs.link_into(&port).is_none() {
                    problems.push(Problem::input(&node.id, &input.name, format!("{} has no link", input.name)));
                }
                continue;
            }
            let Some(value) = node.values.get(&input.name) else {
                let required =
                    matches!(input.kind(), Kind::Int | Kind::Float | Kind::Choice | Kind::Seed | Kind::Upload);
                if required && input.default.is_none() {
                    let what = match input.kind() {
                        Kind::Choice => format!("choose a {}", input.name),
                        Kind::Upload => format!("no {} chosen", input.name),
                        _ => format!("{} has no value", input.name),
                    };
                    problems.push(Problem::input(&node.id, &input.name, what));
                }
                continue;
            };
            if let Some(why) = input.refuse(value) {
                problems.push(Problem::input(&node.id, &input.name, why));
                continue;
            }
            if input.kind() == Kind::Choice {
                let list = input.from.as_deref().and_then(|from| catalogue.choices(from, models));
                if let (Some(list), Some(chosen)) = (list, value.as_str()) {
                    if !list.iter().any(|c| c == chosen) {
                        problems.push(Problem::input(
                            &node.id,
                            &input.name,
                            format!(
                                "{chosen} is not one of the server's {}",
                                input.from.as_deref().unwrap_or("choices")
                            ),
                        ));
                    }
                }
            }
        }
        for name in node.values.keys() {
            if kind.fields().all(|f| &f.name != name) {
                problems.push(Problem::input(&node.id, name, format!("{} has no field {name}", kind.title)));
            }
        }
    }
    problems
}

/// What is wrong with the graph's shape, wherever it is.
fn broken(flow: &Flow, catalogue: &Catalogue) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut ids = BTreeSet::new();
    for node in &flow.nodes {
        if node.id.is_empty() || node.id.contains(':') {
            problems.push(Problem::node(&node.id, format!("{:?} is not a node id: no colons, not empty", node.id)));
        }
        if !ids.insert(node.id.as_str()) {
            problems.push(Problem::node(&node.id, format!("two nodes are called {}", node.id)));
        }
        if catalogue.node(&node.kind).is_none() {
            problems.push(Problem::node(&node.id, format!("the server has no {} node", node.kind)));
        }
    }
    let mut into = BTreeSet::new();
    for link in &flow.links {
        let (Some(from), Some(to)) = (Port::parse(&link.from), Port::parse(&link.to)) else {
            problems.push(Problem::flow(format!("{} → {} is not a link between two ports", link.from, link.to)));
            continue;
        };
        if !into.insert(link.to.as_str()) {
            problems.push(Problem::input(&to.node, &to.name, format!("{} has two links into it", to.name)));
        }
        if let Some(why) = link_problem(flow, catalogue, &from, &to) {
            problems.push(Problem::input(&to.node, &to.name, why));
        }
    }
    if let Some(looped) = in_loops(flow) {
        for id in looped {
            problems.push(Problem::node(&id, "feeds itself through a loop of links"));
        }
    }
    problems
}

/// Why a link from `from` to `to` cannot be, if it cannot: shared by the
/// checks and by the edit that makes a link.
pub(crate) fn link_problem(flow: &Flow, catalogue: &Catalogue, from: &Port, to: &Port) -> Option<String> {
    let Some(source) = flow.node(&from.node) else {
        return Some(format!("there is no node {}", from.node));
    };
    let Some(target) = flow.node(&to.node) else {
        return Some(format!("there is no node {}", to.node));
    };
    // A node of a type the server lacks is reported on its own.
    let (Some(source_kind), Some(target_kind)) = (catalogue.node(&source.kind), catalogue.node(&target.kind)) else {
        return None;
    };
    let Some(output) = source_kind.output(&from.name) else {
        return Some(format!("{} has no output {}", source_kind.title, from.name));
    };
    let Some(input) = target_kind.input(&to.name) else {
        return Some(format!("{} has no input {}", target_kind.title, to.name));
    };
    match &input.socket {
        None => Some(format!("{} is set on the node, not linked", to.name)),
        Some(socket) if socket != &output.socket => {
            Some(format!("{} takes {socket}, and {} makes {}", to.name, from.name, output.socket))
        }
        Some(_) => None,
    }
}

/// The nodes on a loop, if there are any: what is left once every node
/// nothing loops back into, and then every node that feeds nothing left,
/// has been taken away, in rounds — so what a loop feeds is not counted as
/// on it.
fn in_loops(flow: &Flow) -> Option<BTreeSet<String>> {
    let mut feeds: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut fed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for link in &flow.links {
        if let (Some(f), Some(t)) = (Port::parse(&link.from), Port::parse(&link.to)) {
            feeds.entry(t.node.clone()).or_default().insert(f.node.clone());
            fed.entry(f.node).or_default().insert(t.node);
        }
    }
    let mut left: BTreeSet<String> = flow.nodes.iter().map(|n| n.id.clone()).collect();
    for edges in [&feeds, &fed] {
        loop {
            let free: Vec<String> = left
                .iter()
                .filter(|id| edges.get(*id).is_none_or(|next| next.iter().all(|n| !left.contains(n))))
                .cloned()
                .collect();
            if free.is_empty() {
                break;
            }
            for id in free {
                left.remove(&id);
            }
        }
    }
    (!left.is_empty()).then_some(left)
}
