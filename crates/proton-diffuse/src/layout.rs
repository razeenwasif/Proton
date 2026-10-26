//! A graph laid out as a flow: places for nodes that came without any.
//!
//! A run's graph — the one forge writes into each PNG it makes — carries
//! nodes and links but not where each node sat, so a graph read back out of
//! an image is laid out here before it is opened. Columns by depth, as the
//! starters are drawn: a node goes one column right of the deepest node that
//! feeds it, so every wire runs left to right; within a column, nodes are
//! ordered by where what feeds them sits, so wires cross as little as a
//! simple pass can manage, and stacked by the height their cards will have.

use std::collections::BTreeMap;

use crate::catalogue::Catalogue;
use crate::flow::{Flag, Flow, Graph, Node, Port, FORMAT};

/// A column's width, card and gap: `ui/diffuse-graph.js`'s `NODE_W` and room.
const COLUMN: f64 = 250.0;
/// Space between cards in a column.
const GAP: f64 = 24.0;
const MARGIN: f64 = 16.0;

/// A flow of `graph`, its nodes placed.
pub fn flow_from_graph(graph: &Graph, catalogue: &Catalogue) -> Flow {
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    // What feeds each node.
    let mut feeds: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for link in &graph.links {
        if let (Some(from), Some(to)) = (Port::parse(&link.from), Port::parse(&link.to)) {
            if let (Some(f), Some(t)) = (ids.iter().find(|i| **i == from.node), ids.iter().find(|i| **i == to.node)) {
                feeds.entry(*t).or_default().push(*f);
            }
        }
    }
    // Depth: the longest way back to a node nothing feeds. A loop — a graph
    // no server would have run — is cut at as many steps as there are nodes.
    let mut depth: BTreeMap<&str, usize> = ids.iter().map(|id| (*id, 0)).collect();
    for _ in 0..ids.len() {
        let mut moved = false;
        for id in &ids {
            let deepest = feeds.get(id).map(|f| f.iter().map(|s| depth[s] + 1).max().unwrap_or(0)).unwrap_or(0);
            if deepest > depth[id] && deepest <= ids.len() {
                depth.insert(id, deepest);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // Columns, each ordered by where what feeds it sits, then stacked.
    let columns = depth.values().copied().max().map_or(0, |d| d + 1);
    let mut at: BTreeMap<&str, [f64; 2]> = BTreeMap::new();
    for column in 0..columns {
        let mut here: Vec<&str> = ids.iter().copied().filter(|id| depth[id] == column).collect();
        let centre = |id: &str| -> f64 {
            let ys: Vec<f64> =
                feeds.get(id).map(|f| f.iter().filter_map(|s| at.get(s).map(|p| p[1])).collect()).unwrap_or_default();
            match ys.is_empty() {
                true => f64::MAX,
                false => ys.iter().sum::<f64>() / ys.len() as f64,
            }
        };
        here.sort_by(|a, b| centre(a).total_cmp(&centre(b)).then(number(a).cmp(&number(b))));
        let mut y = MARGIN;
        for id in here {
            at.insert(id, [MARGIN + column as f64 * COLUMN, y]);
            let kind = graph.nodes.iter().find(|n| n.id == id).map(|n| n.kind.as_str()).unwrap_or("");
            y += height(catalogue, kind) + GAP;
        }
    }
    Flow {
        format: FORMAT,
        note: None,
        outputs: None,
        nodes: graph
            .nodes
            .iter()
            .map(|n| Node {
                id: n.id.clone(),
                kind: n.kind.clone(),
                at: at.get(n.id.as_str()).copied().unwrap_or([MARGIN, MARGIN]),
                values: n.values.clone(),
                seed: None,
                bypass: Flag(false),
            })
            .collect(),
        links: graph.links.clone(),
    }
}

/// An id's number, for a stable order among nodes placed alike.
fn number(id: &str) -> (u64, String) {
    (id.parse().unwrap_or(u64::MAX), id.to_string())
}

/// How tall a node's card is: `ui/diffuse.js`'s `card` — a head, a row a
/// socket, a pill a field, a box a prompt.
pub fn height(catalogue: &Catalogue, kind: &str) -> f64 {
    let Some(node) = catalogue.node(kind) else { return 70.0 };
    let sockets = node.inputs.iter().filter(|i| i.is_socket()).count().max(node.outputs.len());
    let fields: f64 = node.fields().map(|f| if f.multiline { 78.0 } else { 26.0 }).sum();
    30.0 + 6.0 + sockets as f64 * 22.0 + if fields > 0.0 { fields + 10.0 } else { 6.0 }
}
