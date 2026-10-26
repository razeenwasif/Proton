//! What an output PNG says of itself: its size, and the graph that made it.
//!
//! The image server writes the graph a run sent into each PNG it makes, as a
//! `tEXt` chunk keyed `proton-flow`, before the pixels. So the gallery reads
//! a PNG's head — its `IHDR` and the text before the first `IDAT` — and never
//! the pixels: a few kilobytes of a file of megabytes.

use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::flow::{Graph, Port};

/// The text chunk the graph is kept in.
pub const FLOW_KEY: &str = "proton-flow";

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// A PNG's size and its `proton-flow` text, read from its head.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub width: u32,
    pub height: u32,
    pub flow: Option<String>,
}

/// Read a PNG's head; `None` for anything that is not a PNG.
pub fn head(mut reader: impl Read) -> Option<Head> {
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature).ok()?;
    if signature != SIGNATURE {
        return None;
    }
    let (mut width, mut height, mut flow) = (0, 0, None);
    loop {
        let mut top = [0u8; 8];
        if reader.read_exact(&mut top).is_err() {
            break;
        }
        let length = u32::from_be_bytes([top[0], top[1], top[2], top[3]]) as usize;
        let kind = [top[4], top[5], top[6], top[7]];
        if &kind == b"IDAT" || &kind == b"IEND" {
            break;
        }
        // A chunk larger than any text Proton writes is skipped unread.
        if length > 16 * 1024 * 1024 {
            return None;
        }
        let mut data = vec![0u8; length + 4];
        reader.read_exact(&mut data).ok()?;
        data.truncate(length);
        match &kind {
            b"IHDR" if length >= 8 => {
                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            }
            b"tEXt" => {
                if let Some(at) = data.iter().position(|&b| b == 0) {
                    if &data[..at] == FLOW_KEY.as_bytes() {
                        // tEXt is Latin-1; the server writes ASCII JSON.
                        flow = Some(data[at + 1..].iter().map(|&b| b as char).collect());
                    }
                }
            }
            b"iTXt" => {
                // Keyword, NUL, compressed flag, method, language NUL,
                // translated keyword NUL, then UTF-8 text — uncompressed only.
                if let Some(at) = data.iter().position(|&b| b == 0) {
                    if &data[..at] == FLOW_KEY.as_bytes() && data.get(at + 1) == Some(&0) {
                        let rest = &data[at + 3..];
                        let mut nul = rest.iter().enumerate().filter(|(_, &b)| b == 0).map(|(i, _)| i);
                        if let (Some(_), Some(second)) = (nul.next(), nul.next()) {
                            flow = String::from_utf8(rest[second + 1..].to_vec()).ok();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (width > 0).then_some(Head { width, height, flow })
}

/// What made an image, as the gallery says it: from the graph's sampler,
/// its checkpoint and its positive prompt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseMade")]
pub struct Made {
    #[ts(type = "number | null")]
    pub seed: Option<u64>,
    pub steps: Option<u32>,
    pub cfg: Option<f64>,
    pub sampler: Option<String>,
    pub scheduler: Option<String>,
    pub denoise: Option<f64>,
    pub checkpoint: Option<String>,
    pub prompt: Option<String>,
    pub negative: Option<String>,
}

/// What made an image, from the graph its PNG holds. Read by the v1
/// catalogue's names; a graph of other nodes says what it can.
pub fn made(graph: &Graph) -> Made {
    let node = |id: &str| graph.nodes.iter().find(|n| n.id == id);
    let Some(sampler) = graph.nodes.iter().find(|n| n.kind == "KSampler") else { return Made::default() };
    let v = |key: &str| sampler.values.get(key);
    // What feeds an input of a node, followed back to the node it comes from.
    let source = |to: &str| {
        graph.links.iter().find(|l| l.to == to).and_then(|l| Port::parse(&l.from)).and_then(|p| node(&p.node))
    };
    let text = |input: &str| {
        source(&format!("{}:{input}", sampler.id))
            .filter(|n| n.kind == "TextEncode")
            .and_then(|n| n.values.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    // The model the sampler is given, through any LoRA loaders, to its checkpoint.
    let mut at = source(&format!("{}:model", sampler.id));
    let mut checkpoint = None;
    for _ in 0..graph.nodes.len() {
        match at {
            Some(n) if n.kind == "LoadCheckpoint" => {
                checkpoint = n.values.get("checkpoint").and_then(Value::as_str).map(str::to_string);
                break;
            }
            Some(n) => at = source(&format!("{}:model", n.id)),
            None => break,
        }
    }
    Made {
        seed: v("seed").and_then(Value::as_u64),
        steps: v("steps").and_then(Value::as_u64).map(|s| s as u32),
        cfg: v("cfg").and_then(Value::as_f64),
        sampler: v("sampler").and_then(Value::as_str).map(str::to_string),
        scheduler: v("scheduler").and_then(Value::as_str).map(str::to_string),
        denoise: v("denoise").and_then(Value::as_f64),
        checkpoint,
        prompt: text("positive"),
        negative: text("negative"),
    }
}

/// The graph a PNG's `proton-flow` text holds: the run's body (`{graph}`) or
/// the graph itself.
pub fn graph_of(text: &str) -> Option<Graph> {
    let value: Value = serde_json::from_str(text).ok()?;
    let inner = value.get("graph").cloned().unwrap_or(value);
    serde_json::from_value(inner).ok()
}
