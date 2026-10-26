//! What the server says it can run: its node catalogue (`GET /nodes`) and
//! the models it can load (`GET /models`).
//!
//! Both are read as the server wrote them and written back the same, so the
//! copies Proton keeps in a project's `.proton/diffuse/` — what lets a graph
//! open and be edited while forge is off — are the server's own words. A
//! kind of input or a list of models this Proton has not heard of is kept as
//! it came, not refused: a newer Nexus should not break an older Proton's
//! editor, only the checks that need to understand it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use ts_rs::TS;

/// The node catalogue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseCatalogue")]
pub struct Catalogue {
    pub version: u32,
    /// The socket types links carry, such as `MODEL` and `LATENT`.
    pub types: Vec<String>,
    pub nodes: Vec<NodeType>,
    /// Named lists a choice can be filled from, such as `samplers`.
    #[serde(default)]
    pub lists: BTreeMap<String, Vec<String>>,
}

/// One kind of node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseNodeType")]
pub struct NodeType {
    /// Its name in a graph, such as `KSampler`.
    #[serde(rename = "type")]
    pub name: String,
    /// What the editor calls it, such as `KSampler` or `Load Checkpoint`.
    pub title: String,
    /// Where the editor lists it.
    pub group: String,
    #[serde(default)]
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub outputs: Vec<Output>,
}

/// One input: a socket a link comes into, when it has a `type`; otherwise a
/// field set on the node, of its `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseInput")]
pub struct Input {
    pub name: String,
    /// The socket type a link into it must carry.
    #[serde(rename = "type", default)]
    pub socket: Option<String>,
    /// For a field: `int`, `float`, `text`, `choice`, `seed` or `upload`.
    #[serde(default)]
    pub kind: Option<String>,
    /// A socket the node runs without.
    #[serde(default)]
    pub optional: bool,
    /// A text field written over several lines, such as a prompt.
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub min: Option<Number>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub max: Option<Number>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub step: Option<Number>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub default: Option<Value>,
    /// For a choice: the list it is chosen from — one of the catalogue's
    /// `lists`, or one of `/models`' lists.
    #[serde(default)]
    pub from: Option<String>,
}

/// One output socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseOutput")]
pub struct Output {
    pub name: String,
    #[serde(rename = "type")]
    pub socket: String,
}

/// What a field holds, as far as this Proton understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Text,
    Choice,
    Seed,
    Upload,
    /// A kind from a newer server: kept as it is, never checked.
    Other,
}

/// The largest seed: the window holds numbers as doubles, so a seed is a
/// whole number it can hold exactly.
pub const SEED_MAX: u64 = (1 << 53) - 1;

/// The models the server can load, list by list (`checkpoints`, `loras`,
/// `vaes`, `upscalers`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseModels")]
pub struct Models(pub BTreeMap<String, Vec<Model>>);

/// One model file on the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseModel")]
pub struct Model {
    pub name: String,
    /// `sd15`, `sdxl`, `sd3` or `flux`; none for an upscaler.
    #[serde(default)]
    pub family: Option<String>,
    #[ts(type = "number")]
    pub bytes: u64,
}

impl Catalogue {
    /// The catalogue from the server's JSON.
    pub fn parse(text: &str) -> Result<Catalogue, String> {
        serde_json::from_str(text).map_err(|e| format!("the node catalogue: {e}"))
    }

    pub fn node(&self, name: &str) -> Option<&NodeType> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// What a choice from `from` can be: the catalogue's own list, or the
    /// server's models. `None` when neither is known — the model lists not
    /// fetched yet — which is not the same as an empty list.
    pub fn choices(&self, from: &str, models: Option<&Models>) -> Option<Vec<String>> {
        if let Some(list) = self.lists.get(from) {
            return Some(list.clone());
        }
        models?.0.get(from).map(|list| list.iter().map(|m| m.name.clone()).collect())
    }
}

impl NodeType {
    pub fn input(&self, name: &str) -> Option<&Input> {
        self.inputs.iter().find(|i| i.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&Output> {
        self.outputs.iter().find(|o| o.name == name)
    }

    /// The inputs set on the node rather than linked.
    pub fn fields(&self) -> impl Iterator<Item = &Input> {
        self.inputs.iter().filter(|i| i.socket.is_none())
    }

    /// A node that makes nothing a link can carry is an output — Save Image,
    /// Preview Image — and is what a run is run for.
    pub fn is_output(&self) -> bool {
        self.outputs.is_empty()
    }
}

impl Input {
    pub fn is_socket(&self) -> bool {
        self.socket.is_some()
    }

    pub fn kind(&self) -> Kind {
        match self.kind.as_deref() {
            Some("int") => Kind::Int,
            Some("float") => Kind::Float,
            Some("text") => Kind::Text,
            Some("choice") => Kind::Choice,
            Some("seed") => Kind::Seed,
            Some("upload") => Kind::Upload,
            _ => Kind::Other,
        }
    }

    /// Why `value` cannot be this field's, if it cannot: the wrong kind of
    /// JSON, or a number outside the field's range. A choice not in its list
    /// is left to [`crate::check`], since the list may not be known yet.
    pub fn refuse(&self, value: &Value) -> Option<String> {
        let name = &self.name;
        let number = |whole: bool| -> Option<String> {
            let Some(n) = value.as_f64() else {
                return Some(format!("{name} is a number"));
            };
            if whole && !(value.is_i64() || value.is_u64()) {
                return Some(format!("{name} is a whole number"));
            }
            if let Some(min) = self.min.as_ref().and_then(Number::as_f64) {
                if n < min {
                    return Some(format!("{name} is at least {min}"));
                }
            }
            if let Some(max) = self.max.as_ref().and_then(Number::as_f64) {
                if n > max {
                    return Some(format!("{name} is at most {max}"));
                }
            }
            // A whole number on a step — a latent's size, a multiple of 8 —
            // is on it, counted from its least, as the server checks it.
            if whole {
                if let (Some(step), Some(v)) =
                    (self.step.as_ref().and_then(Number::as_i64).filter(|s| *s > 1), value.as_i64())
                {
                    let from = self.min.as_ref().and_then(Number::as_i64).unwrap_or(0);
                    if (v - from).rem_euclid(step) != 0 {
                        let below = v - (v - from).rem_euclid(step);
                        return Some(format!(
                            "{name} must be a multiple of {step}: {below} or {}, not {v}",
                            below + step
                        ));
                    }
                }
            }
            None
        };
        match self.kind() {
            Kind::Int => number(true),
            Kind::Float => number(false),
            Kind::Text | Kind::Choice | Kind::Upload => match value.is_string() {
                true => None,
                false => Some(format!("{name} is text")),
            },
            Kind::Seed => match value.as_u64() {
                Some(seed) if seed <= SEED_MAX => None,
                _ => Some(format!("{name} is a whole number from 0 to {SEED_MAX}")),
            },
            Kind::Other => None,
        }
    }
}
