//! Starter graphs: a new flow that already does something, laid out as
//! the mockups draw it. Each is a batch of ops, so it is checked against the
//! server's catalogue as any edit is, and refused whole when the server has
//! not the nodes it needs.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::op::Op;

/// The starters there are, by name, with what the editor calls each and
/// what it is for.
pub const TEMPLATES: [(&str, &str, &str); 5] = [
    ("textToImage", "Text to image", "a prompt, SDXL, one picture"),
    ("imageToImage", "Image to image", "a picture of yours, redrawn from a prompt"),
    ("inpaint", "Inpaint", "a picture with part of it redrawn: its mask"),
    ("hiresFix", "Hires fix", "drawn, enlarged in the latent, drawn again sharper"),
    ("flux", "FLUX", "FLUX.1 dev: one prompt, no negative"),
];

/// A starter's ops, by name.
pub fn ops(name: &str) -> Option<Vec<Op>> {
    match name {
        "textToImage" => Some(text_to_image()),
        "imageToImage" => Some(image_to_image(false)),
        "inpaint" => Some(image_to_image(true)),
        "hiresFix" => Some(hires_fix()),
        "flux" => Some(flux()),
        _ => None,
    }
}

fn values(v: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(v).unwrap_or_default()
}

fn add(kind: &str, at: [f64; 2], v: Value) -> Op {
    Op::Add { kind: kind.into(), at, values: values(v) }
}

fn link(from: &str, to: &str) -> Op {
    Op::Connect { from: from.into(), to: to.into() }
}

/// The checkpoint SDXL's starters load.
const SDXL: &str = "sd_xl_base_1.0.safetensors";

/// SDXL's text to image: a checkpoint, two prompts, an empty latent, the
/// sampler, the decode and the save.
fn text_to_image() -> Vec<Op> {
    vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": SDXL})),
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

/// A picture of the person's, encoded and redrawn from a prompt — its
/// denoise how much is redrawn. Inpainting sets the picture's mask on the
/// latent first, so only the masked part is.
fn image_to_image(masked: bool) -> Vec<Op> {
    let (prompt, denoise, prefix) = match masked {
        false => ("the same scene at golden hour, soft warm light", 0.6, "img2img"),
        true => ("a small red fishing boat moored by the rocks", 0.85, "inpaint"),
    };
    let sampler = if masked { "7" } else { "6" };
    let mut ops = vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": SDXL})),
        add("TextEncode", [266.0, 16.0], json!({"text": prompt})),
        add("TextEncode", [266.0, 220.0], json!({"text": "blurry, text, watermark"})),
        add("LoadImage", [16.0, 220.0], json!({})),
        add("VAEEncode", [266.0, 370.0], json!({})),
    ];
    if masked {
        ops.push(add("SetLatentMask", [266.0, 480.0], json!({})));
    }
    let decode = if masked { "8" } else { "7" };
    let save = if masked { "9" } else { "8" };
    ops.extend([
        add(
            "KSampler",
            [516.0, 16.0],
            json!({"seed": 42, "sampler": "dpmpp_2m", "scheduler": "karras", "denoise": denoise}),
        ),
        add("VAEDecode", [766.0, 16.0], json!({})),
        add("SaveImage", [766.0, 160.0], json!({"prefix": prefix})),
        link("1:clip", "2:clip"),
        link("1:clip", "3:clip"),
        link("1:model", &format!("{sampler}:model")),
        link("2:conditioning", &format!("{sampler}:positive")),
        link("3:conditioning", &format!("{sampler}:negative")),
        link("1:vae", "5:vae"),
        link("4:image", "5:image"),
    ]);
    match masked {
        true => ops.extend([link("5:latent", "6:latent"), link("4:mask", "6:mask"), link("6:latent", "7:latent")]),
        false => ops.push(link("5:latent", "6:latent")),
    }
    ops.extend([
        link(&format!("{sampler}:latent"), &format!("{decode}:latent")),
        link("1:vae", &format!("{decode}:vae")),
        link(&format!("{decode}:image"), &format!("{save}:images")),
    ]);
    ops
}

/// Drawn at SDXL's size, the latent enlarged half as much again, then drawn
/// again over it with less noise: detail a single pass at the larger size
/// would not keep together.
fn hires_fix() -> Vec<Op> {
    vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": SDXL})),
        add("TextEncode", [266.0, 16.0], json!({"text": "a lighthouse at dusk, oil painting, fine detail"})),
        add("TextEncode", [266.0, 236.0], json!({"text": "blurry, low contrast, text, watermark"})),
        add("EmptyLatent", [16.0, 260.0], json!({})),
        add("KSampler", [516.0, 16.0], json!({"seed": 42, "sampler": "dpmpp_2m", "scheduler": "karras"})),
        add("UpscaleLatent", [766.0, 16.0], json!({"scale": 1.5})),
        add(
            "KSampler",
            [1016.0, 16.0],
            json!({"seed": 42, "steps": 20, "sampler": "dpmpp_2m", "scheduler": "karras", "denoise": 0.5}),
        ),
        add("VAEDecode", [1266.0, 16.0], json!({})),
        add("SaveImage", [1266.0, 160.0], json!({"prefix": "hires"})),
        link("1:clip", "2:clip"),
        link("1:clip", "3:clip"),
        link("1:model", "5:model"),
        link("2:conditioning", "5:positive"),
        link("3:conditioning", "5:negative"),
        link("4:latent", "5:latent"),
        link("5:latent", "6:latent"),
        link("1:model", "7:model"),
        link("2:conditioning", "7:positive"),
        link("3:conditioning", "7:negative"),
        link("6:latent", "7:latent"),
        link("7:latent", "8:latent"),
        link("1:vae", "8:vae"),
        link("8:image", "9:images"),
    ]
}

/// FLUX.1 dev: its guidance in CFG's place, Euler on the simple schedule,
/// and no negative prompt, which FLUX does not take.
fn flux() -> Vec<Op> {
    vec![
        add("LoadCheckpoint", [16.0, 16.0], json!({"checkpoint": "flux1-dev-fp8.safetensors"})),
        add(
            "TextEncode",
            [266.0, 16.0],
            json!({"text": "a lighthouse at dusk on a rocky shore, oil painting, warm light"}),
        ),
        add("EmptyLatent", [16.0, 260.0], json!({})),
        add(
            "KSampler",
            [516.0, 16.0],
            json!({"seed": 42, "steps": 20, "cfg": 3.5, "sampler": "euler", "scheduler": "simple"}),
        ),
        add("VAEDecode", [766.0, 16.0], json!({})),
        add("SaveImage", [766.0, 160.0], json!({"prefix": "flux"})),
        link("1:clip", "2:clip"),
        link("1:model", "4:model"),
        link("2:conditioning", "4:positive"),
        link("3:latent", "4:latent"),
        link("4:latent", "5:latent"),
        link("1:vae", "5:vae"),
        link("5:image", "6:images"),
    ]
}
