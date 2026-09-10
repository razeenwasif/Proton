//! Turning a material's graph into something a renderer can run.
//!
//! ## Why this is a compiler and not an evaluator
//!
//! The obvious thing is to evaluate the graph here and send the answer. It
//! does not work: a surface property that varies across a mesh is one value
//! per triangle, and at this project's triangle budget that is eighty thousand
//! colours crossing the seam — re-sent on every nudge of a slider. What
//! crosses instead is the *program*: a dozen instructions, sent once, and run
//! by whatever is drawing.
//!
//! Which is also the honest division of labour. What a graph means is this
//! crate's business, because this crate owns the file it is written in. How
//! fast it runs, and at what resolution, is the renderer's — a canvas doing
//! one triangle at a time and a GPU doing one pixel at a time are running the
//! same program and will not agree exactly, and that is true of every real
//! pipeline.
//!
//! ## Everything is three numbers
//!
//! A scalar is a vector with the same value in all three channels, which is
//! what shading languages do and what lets `mix` take a colour or a number in
//! the same socket without two of every instruction. Reading a scalar back
//! takes the first channel.
//!
//! ## A graph this cannot compile is refused, not approximated
//!
//! A node kind from a later version, a missing output, an input fed by
//! something that is not there: each of those produces a `problem` and no
//! program. The surface then draws in the plain no-material grey, and the
//! panel says why. Guessing at a node nobody here understands would put a
//! picture on screen that the file does not describe.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::material::{Material, ParamValue};

/// One instruction. Each writes a three-channel value to its own register,
/// numbered by position, and may only refer to registers already written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// A number or a colour written down in the file.
    Constant { value: [f32; 3] },
    /// The point being shaded, in the object's own space, in metres.
    ///
    /// Object space and not world space, so a pattern stays put on a thing
    /// that is moved — which is what somebody who textured it expects.
    Position,
    /// Fractal noise of a point: `detail` octaves, each half the amplitude
    /// and twice the frequency of the last.
    Noise { at: usize, scale: f32, detail: f32 },
    /// A three-dimensional checkerboard of a point.
    Checker { at: usize, scale: f32 },
    /// Between `a` and `b` by the first channel of `fac`, clamped.
    Mix { a: usize, b: usize, fac: usize },
}

/// A compiled material: what to run, and which registers hold the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Shader {
    /// The material this came from.
    pub id: String,
    /// In dependency order: every register an instruction reads was written
    /// by an earlier one, so a single pass evaluates the lot.
    pub ops: Vec<Op>,
    /// Which register holds the surface colour.
    pub base: usize,
    /// Which holds how metallic it is, in channel one.
    pub metallic: usize,
    /// Which holds how rough, likewise.
    pub roughness: usize,
    /// Why there is no program, when there is not. Empty when `ops` is good.
    pub problem: String,
}

/// The node kinds this can compile.
///
/// Deliberately short. Everything here is something a renderer can run in a
/// few lines, and a graph reaching past it is told so rather than guessed at.
pub const NODES: &[&str] = &[
    "output",
    "principled",
    "noise",
    "checker",
    "ramp",
    "mix",
    "rgb",
    "value",
];

impl Material {
    /// Compile this material, or say why it cannot be.
    pub fn compile(&self) -> Shader {
        let mut out = Build {
            ops: Vec::new(),
            done: Vec::new(),
            material: self,
        };
        let refused = |why: String| Shader {
            id: self.id.clone(),
            ops: Vec::new(),
            base: 0,
            metallic: 0,
            roughness: 0,
            problem: why,
        };

        // The graph is read backwards from what it produces, so a node wired
        // to nothing costs nothing — which is what makes a work-in-progress
        // graph with a spare noise node lying beside it perfectly renderable.
        let Some(output) = self.nodes.iter().find(|one| one.kind == "output") else {
            return refused("this graph has no output node".into());
        };
        let surface = match out.feeding(&format!("{}:surface", output.id)) {
            Some(node) => node,
            None => return refused("nothing is wired into the output".into()),
        };

        let shading = match self.nodes.iter().find(|one| one.id == surface) {
            Some(one) => one,
            None => return refused(format!("there is no node {surface:?}")),
        };
        if shading.kind != "principled" {
            return refused(format!(
                "the output is fed by a {:?}, which is not a surface",
                shading.kind
            ));
        }

        let base = out.port(&shading.id, "baseColor", [0.8, 0.8, 0.8]);
        let metallic = out.port(&shading.id, "metallic", [0.0; 3]);
        let roughness = out.port(&shading.id, "roughness", [0.5; 3]);
        match (base, metallic, roughness) {
            (Ok(base), Ok(metallic), Ok(roughness)) => Shader {
                id: self.id.clone(),
                ops: out.ops,
                base,
                metallic,
                roughness,
                problem: String::new(),
            },
            (Err(why), _, _) | (_, Err(why), _) | (_, _, Err(why)) => refused(why),
        }
    }
}

/// The compile in progress: the instructions so far, and which nodes are
/// already in them.
struct Build<'a> {
    ops: Vec<Op>,
    /// Node id and the register its output landed in, so a node feeding two
    /// sockets is compiled once.
    done: Vec<(String, usize)>,
    material: &'a Material,
}

impl Build<'_> {
    /// Which node feeds an input, if any.
    fn feeding(&self, port: &str) -> Option<String> {
        self.material
            .links
            .iter()
            .find(|link| link.to == port)
            .and_then(|link| link.from.split(':').next().map(str::to_string))
    }

    /// One input of a node: whatever is wired into it, or its own setting.
    fn port(&mut self, node: &str, name: &str, fallback: [f32; 3]) -> Result<usize, String> {
        if let Some(source) = self.feeding(&format!("{node}:{name}")) {
            return self.node(&source);
        }
        let value = self
            .material
            .nodes
            .iter()
            .find(|one| one.id == node)
            .and_then(|one| one.params.iter().find(|p| p.name == name))
            .map(|p| triple(&p.value))
            .unwrap_or(fallback);
        Ok(self.push(Op::Constant { value }))
    }

    /// One number setting of a node, for an instruction that wants a scalar
    /// rather than a wire — a noise's scale is not something you animate with
    /// another noise.
    fn number(&self, node: &str, name: &str, fallback: f32) -> f32 {
        self.material
            .nodes
            .iter()
            .find(|one| one.id == node)
            .and_then(|one| one.params.iter().find(|p| p.name == name))
            .map(|p| triple(&p.value)[0])
            .unwrap_or(fallback)
    }

    /// Compile a node's output, reusing it if it is already compiled.
    fn node(&mut self, id: &str) -> Result<usize, String> {
        if let Some((_, at)) = self.done.iter().find(|(name, _)| name == id) {
            return Ok(*at);
        }
        let Some(one) = self.material.nodes.iter().find(|one| one.id == id) else {
            return Err(format!("there is no node {id:?}"));
        };

        let at = match one.kind.as_str() {
            "rgb" | "value" => {
                let value = one
                    .params
                    .first()
                    .map(|p| triple(&p.value))
                    .unwrap_or([0.5; 3]);
                self.push(Op::Constant { value })
            }
            "noise" => {
                // A noise samples the surface unless something else says where
                // — which is how one noise warps another.
                let at = match self.feeding(&format!("{id}:at")) {
                    Some(source) => self.node(&source)?,
                    None => self.push(Op::Position),
                };
                self.push(Op::Noise {
                    at,
                    scale: self.number(id, "scale", 4.0),
                    detail: self.number(id, "detail", 2.0),
                })
            }
            "checker" => {
                let at = match self.feeding(&format!("{id}:at")) {
                    Some(source) => self.node(&source)?,
                    None => self.push(Op::Position),
                };
                self.push(Op::Checker {
                    at,
                    scale: self.number(id, "scale", 4.0),
                })
            }
            // A ramp is a mix by another name: two colours and something
            // between them. One instruction does both rather than two that
            // differ only in what their sockets are called.
            "ramp" | "mix" => {
                let (first, second) = match one.kind.as_str() {
                    "ramp" => ("from", "to"),
                    _ => ("a", "b"),
                };
                let a = self.port(id, first, [0.0; 3])?;
                let b = self.port(id, second, [1.0; 3])?;
                let fac = self.port(id, "fac", [0.5; 3])?;
                self.push(Op::Mix { a, b, fac })
            }
            other => {
                return Err(format!(
                    "this build cannot render a {other:?} node — the graph is \
                     kept exactly as it is"
                ))
            }
        };
        self.done.push((id.to_string(), at));
        Ok(at)
    }

    fn push(&mut self, op: Op) -> usize {
        // Reused when the same instruction is already there. A graph where six
        // sockets fall back to the same default colour should not be six
        // identical constants.
        if let Some(at) = self.ops.iter().position(|had| *had == op) {
            return at;
        }
        self.ops.push(op);
        self.ops.len() - 1
    }
}

/// A setting as three channels: a number in all three, a colour as it is.
fn triple(value: &ParamValue) -> [f32; 3] {
    match value {
        ParamValue::Number(n) => [*n as f32; 3],
        ParamValue::Color(c) => [c[0] as f32, c[1] as f32, c[2] as f32],
    }
}
