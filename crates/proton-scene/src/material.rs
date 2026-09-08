//! Materials, as a graph of nodes in the scene file.
//!
//! ## Why the graph lives in `scene.proton.json` and not beside it
//!
//! A material is part of a scene the way an object is: it is referenced by id,
//! it is edited in the same session, and a change to one is reviewed with the
//! change to the other. Splitting it into sidecar files would mean two things
//! to keep in step and two diffs to read for one edit.
//!
//! ## Nodes and links are two flat lists
//!
//! Same reasoning as the object hierarchy. A link could have been a field on
//! the node it arrives at — and then rewiring an input would edit the node's
//! block, and adding one would move every line after it. As a separate list,
//! a rewire is one line that says exactly what changed:
//!
//! ```text
//! -    { "from": "noise-1:fac", "to": "bsdf-1:roughness" },
//! +    { "from": "ramp-1:color", "to": "bsdf-1:roughness" },
//! ```
//!
//! A port is written `node:port` rather than as two keys, because it is one
//! thing — an endpoint — and splitting it across a `node` and a `port` field
//! would put half of a rewire on each of two lines.
//!
//! ## What a graph is allowed to be
//!
//! Acyclic, and one link into any input. Both are enforced on the way in
//! rather than checked when something tries to evaluate it: a shader graph
//! with a loop has no order to evaluate in, and an input fed twice has no
//! answer, and in both cases the honest moment to say so is when somebody
//! draws the wire.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

/// One material and the graph that makes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub id: String,
    pub name: String,
    pub nodes: Vec<GraphNode>,
    pub links: Vec<Link>,
}

/// One node in a material's graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    /// `principled`, `noise`, `ramp`, `output` — free text, so a graph written
    /// by a later version keeps its nodes instead of losing them.
    pub kind: String,
    /// Where the box sits on the canvas, in graph units.
    ///
    /// Stored, which means tidying a layout shows up in the diff. That is the
    /// honest trade: a graph that forgot its arrangement every time it was
    /// closed would be re-tidied constantly, and *that* churn is worse and
    /// falls on a person rather than on Git.
    pub at: [f64; 2],
    pub params: Vec<Param>,
}

/// One setting on a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    pub value: ParamValue,
}

/// What a setting holds. Numbers and colours are the two a shader graph needs
/// before it needs anything else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ParamValue {
    Number(f64),
    /// Linear RGB, 0..1.
    Color([f64; 3]),
}

/// One wire, from an output port to an input port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// `node:port`.
    pub from: String,
    pub to: String,
}

impl crate::Scene {
    /// Every material in the scene.
    pub fn materials(&self) -> Vec<Material> {
        self.json
            .get("materials")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(read_material).collect())
            .unwrap_or_default()
    }

    /// The material an object uses, if it names one that exists.
    pub fn material_of(&self, object: &str) -> Option<Material> {
        let named = self
            .json
            .get("objects")?
            .as_array()?
            .iter()
            .filter_map(Value::as_object)
            .find(|one| one.get("id").and_then(Value::as_str) == Some(object))?
            .get("material")?
            .as_str()?
            .to_string();
        self.materials().into_iter().find(|one| one.id == named)
    }

    /// Change one setting on one node.
    pub fn set_param(
        &mut self,
        material: &str,
        node: &str,
        param: &str,
        value: ParamValue,
    ) -> Result<bool, String> {
        let now = write_value(&value);
        let slot = self.param_mut(material, node, param)?;
        // Compared as written, so a change too small to reach the file is not
        // a change: the same rule a transform follows.
        if crate::write::to_text(slot) == crate::write::to_text(&now) {
            return Ok(false);
        }
        *slot = now;
        Ok(true)
    }

    /// Move a node's box on the canvas.
    pub fn move_node(&mut self, material: &str, node: &str, at: [f64; 2]) -> Result<bool, String> {
        let one = self.node_mut(material, node)?;
        let now = json!([at[0], at[1]]);
        if one.get("at").map(crate::write::to_text) == Some(crate::write::to_text(&now)) {
            return Ok(false);
        }
        one.insert("at".into(), now);
        Ok(true)
    }

    /// Wire an output to an input.
    ///
    /// Replaces whatever fed that input, because an input takes one wire — a
    /// graph where two things feed one socket has no answer for what the value
    /// is, and silently keeping both would produce one.
    pub fn connect(&mut self, material: &str, from: &str, to: &str) -> Result<bool, String> {
        let (from_node, _) = split(from)?;
        let (to_node, _) = split(to)?;
        if from_node == to_node {
            return Err("a node cannot feed itself".into());
        }
        let found = self
            .materials()
            .into_iter()
            .find(|one| one.id == material)
            .ok_or_else(|| format!("there is no material {material:?}"))?;
        for end in [&from_node, &to_node] {
            if !found.nodes.iter().any(|one| &one.id == end) {
                return Err(format!("there is no node {end:?} in {material:?}"));
            }
        }
        // Would the wire close a loop? `from` is upstream of `to` once this
        // exists, so a loop is exactly the case where `to` already reaches
        // `from`.
        if reaches(&found, &to_node, &from_node) {
            return Err(format!(
                "{to_node:?} already feeds {from_node:?} — that would make a loop"
            ));
        }
        if found.links.iter().any(|l| l.from == from && l.to == to) {
            return Ok(false);
        }

        let list = self.links_mut(material)?;
        // Rewired in place when there is already a wire into that input, and
        // only appended when there is not. Removing and pushing would have
        // been shorter and would move the link to the end of the list — so
        // changing what feeds one socket would show up as a block deleted
        // here and an identical-looking block added there, which is the exact
        // diff this format exists to avoid.
        if let Some(existing) = list
            .iter_mut()
            .filter_map(Value::as_object_mut)
            .find(|link| link.get("to").and_then(Value::as_str) == Some(to))
        {
            existing.insert("from".into(), json!(from));
            return Ok(true);
        }
        list.push(json!({ "from": from, "to": to }));
        Ok(true)
    }

    /// Remove whatever feeds an input.
    pub fn disconnect(&mut self, material: &str, to: &str) -> Result<bool, String> {
        let list = self.links_mut(material)?;
        let before = list.len();
        list.retain(|link| link.get("to").and_then(Value::as_str) != Some(to));
        Ok(list.len() != before)
    }

    fn links_mut(&mut self, material: &str) -> Result<&mut Vec<Value>, String> {
        let found = self.material_mut(material)?;
        // A graph with no wires yet has no list, which is ordinary.
        if !found.get("links").is_some_and(Value::is_array) {
            found.insert("links".into(), json!([]));
        }
        found
            .get_mut("links")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "links is not a list".into())
    }

    fn material_mut(&mut self, material: &str) -> Result<&mut Map<String, Value>, String> {
        self.json
            .get_mut("materials")
            .and_then(Value::as_array_mut)
            .and_then(|list| {
                list.iter_mut()
                    .filter_map(Value::as_object_mut)
                    .find(|one| one.get("id").and_then(Value::as_str) == Some(material))
            })
            .ok_or_else(|| format!("there is no material {material:?}"))
    }

    fn node_mut(&mut self, material: &str, node: &str) -> Result<&mut Map<String, Value>, String> {
        let found = self.material_mut(material)?;
        found
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .and_then(|list| {
                list.iter_mut()
                    .filter_map(Value::as_object_mut)
                    .find(|one| one.get("id").and_then(Value::as_str) == Some(node))
            })
            .ok_or_else(|| format!("there is no node {node:?} in {material:?}"))
    }

    fn param_mut(&mut self, material: &str, node: &str, param: &str) -> Result<&mut Value, String> {
        let one = self.node_mut(material, node)?;
        if !one.get("params").is_some_and(Value::is_object) {
            one.insert("params".into(), json!({}));
        }
        let params = one
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or("params is not an object")?;
        // Absent is ordinary: a node need not write down a setting it has
        // never been given.
        Ok(params.entry(param.to_string()).or_insert(Value::Null))
    }
}

/// Whether `from` reaches `to` by following wires downstream.
fn reaches(material: &Material, from: &str, to: &str) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    let mut stack = vec![from];
    while let Some(at) = stack.pop() {
        if at == to {
            return true;
        }
        if seen.contains(&at) {
            continue;
        }
        seen.push(at);
        for link in &material.links {
            let Ok((source, _)) = split(&link.from) else {
                continue;
            };
            if source == at {
                if let Ok((target, _)) = split(&link.to) {
                    // Pushed as a slice of the link, which outlives the loop.
                    if let Some(node) = material.nodes.iter().find(|n| n.id == target) {
                        stack.push(&node.id);
                    }
                }
            }
        }
    }
    false
}

/// `node:port` into its halves.
fn split(port: &str) -> Result<(String, String), String> {
    match port.split_once(':') {
        Some((node, name)) if !node.is_empty() && !name.is_empty() => {
            Ok((node.to_string(), name.to_string()))
        }
        _ => Err(format!("{port:?} is not a node:port")),
    }
}

fn read_material(value: &Value) -> Option<Material> {
    let map = value.as_object()?;
    let id = map.get("id")?.as_str()?.to_string();
    Some(Material {
        name: map
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string(),
        nodes: map
            .get("nodes")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(read_node).collect())
            .unwrap_or_default(),
        links: map
            .get("links")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|link| {
                        Some(Link {
                            from: link.get("from")?.as_str()?.to_string(),
                            to: link.get("to")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        id,
    })
}

fn read_node(value: &Value) -> Option<GraphNode> {
    let map = value.as_object()?;
    let at = map
        .get("at")
        .and_then(Value::as_array)
        .filter(|list| list.len() == 2)
        .and_then(|list| Some([list[0].as_f64()?, list[1].as_f64()?]))
        .unwrap_or([0.0, 0.0]);
    Some(GraphNode {
        id: map.get("id")?.as_str()?.to_string(),
        kind: map
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        // Sorted by `serde_json`'s map, so the panel draws a node's settings in
        // the same order twice.
        params: map
            .get("params")
            .and_then(Value::as_object)
            .map(|params| {
                params
                    .iter()
                    .filter_map(|(name, value)| {
                        Some(Param {
                            name: name.clone(),
                            value: read_value(value)?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        at,
    })
}

fn read_value(value: &Value) -> Option<ParamValue> {
    if let Some(n) = value.as_f64() {
        return Some(ParamValue::Number(n));
    }
    let list = value.as_array()?;
    if list.len() != 3 {
        return None;
    }
    Some(ParamValue::Color([
        list[0].as_f64()?,
        list[1].as_f64()?,
        list[2].as_f64()?,
    ]))
}

fn write_value(value: &ParamValue) -> Value {
    match value {
        ParamValue::Number(n) => json!(n),
        ParamValue::Color(c) => json!([c[0], c[1], c[2]]),
    }
}
