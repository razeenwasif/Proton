//! Starter graphs: a new flow that already does something, laid out as
//! the mockups draw it. Each is a batch of ops, so it is checked against the
//! server's catalogue as any edit is, and refused whole when the server has
//! not the nodes it needs.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::op::Op;

/// The starters there are, by name, with what the editor calls each.
pub const TEMPLATES: [(&str, &str); 1] = [("textToImage", "Text to image")];

/// A starter's ops, by name.
pub fn ops(name: &str) -> Option<Vec<Op>> {
    match name {
        "textToImage" => Some(text_to_image()),
        _ => None,
    }
}

fn values(v: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(v).unwrap_or_default()
}

/// SDXL's text to image: a checkpoint, two prompts, an empty latent, the
/// sampler, the decode and the save.
fn text_to_image() -> Vec<Op> {
    let add = |kind: &str, at: [f64; 2], v: Value| Op::Add { kind: kind.into(), at, values: values(v) };
    let link = |from: &str, to: &str| Op::Connect { from: from.into(), to: to.into() };
    vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": "sd_xl_base_1.0.safetensors"})),
        add("TextEncode", [266.0, 16.0], json!({"text": "a lighthouse at dusk, oil painting"})),
        add("TextEncode", [266.0, 236.0], json!({"text": "blurry, low contrast, text, watermark"})),
        add("EmptyLatent", [16.0, 260.0], json!({})),
        add("KSampler", [516.0, 16.0], json!({"seed": 42, "sampler": "dpmpp_2m", "scheduler": "karras"})),
        add("VAEDecode", [766.0, 16.0], json!({})),
        add("SaveImage", [766.0, 160.0], json!({"prefix": "image"})),
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
