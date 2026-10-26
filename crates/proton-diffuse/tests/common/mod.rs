//! What the tests share: the design's catalogue and run, read from the
//! design itself, and the SDXL flow built by ops.
#![allow(dead_code)]

pub mod server;

use std::collections::BTreeMap;

use proton_diffuse::{Catalogue, Flow, Models, Op};
use serde_json::{json, Value};

pub fn catalogue() -> Catalogue {
    Catalogue::parse(include_str!("../fixtures/catalogue-v1.json")).unwrap()
}

pub fn models() -> Models {
    serde_json::from_str(include_str!("../fixtures/models.json")).unwrap()
}

/// The JSON block under `heading` in `docs/DIFFUSE.md`, from its first `{`.
pub fn design_block(heading: &str) -> Value {
    // A Windows checkout may end its lines with \r\n.
    let design = include_str!("../../../../docs/DIFFUSE.md").replace("\r\n", "\n");
    let from = design.find(heading).unwrap_or_else(|| panic!("the design has no {heading}"));
    let rest = &design[from..];
    let open = rest.find("```json\n").expect("a JSON block under the heading") + "```json\n".len();
    let close = rest[open..].find("\n```").unwrap() + open;
    // A request's block starts with its method and path.
    let block = &rest[open..close];
    let body = &block[block.find('{').unwrap()..];
    serde_json::from_str(body).unwrap()
}

fn values(v: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(v).unwrap()
}

/// The design's SDXL text-to-image flow, node by node as a person would add
/// it, laid out as the mockups draw it.
pub fn sdxl_ops() -> Vec<Op> {
    let add = |kind: &str, at: [f64; 2], v: Value| Op::Add { kind: kind.into(), at, values: values(v) };
    let link = |from: &str, to: &str| Op::Connect { from: from.into(), to: to.into() };
    vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": "sd_xl_base_1.0.safetensors"})),
        add("TextEncode", [212.0, 16.0], json!({"text": "a lighthouse at dusk, oil painting"})),
        add("TextEncode", [212.0, 208.0], json!({"text": "blurry"})),
        add("EmptyLatent", [16.0, 232.0], json!({})),
        add("KSampler", [408.0, 16.0], json!({"seed": 42, "sampler": "dpmpp_2m", "scheduler": "karras"})),
        add("VAEDecode", [596.0, 16.0], json!({})),
        add("SaveImage", [596.0, 148.0], json!({"prefix": "lighthouse"})),
        link("1:clip", "2:clip"),
        link("1:clip", "3:clip"),
        link("1:model", "5:model"),
        link("2:conditioning", "5:positive"),
        link("3:conditioning", "5:negative"),
        link("4:latent", "5:latent"),
        link("5:latent", "6:latent"),
        link("1:vae", "6:vae"),
        link("6:image", "7:images"),
    ]
}

pub fn sdxl() -> Flow {
    let mut flow = Flow::default();
    proton_diffuse::op::apply(&mut flow, &sdxl_ops(), &catalogue()).unwrap();
    flow
}

/// The flow file the SDXL flow writes, as a Windows checkout may not keep it.
pub fn lighthouse_text() -> String {
    include_str!("../fixtures/lighthouse.flow.json").replace("\r\n", "\n")
}
