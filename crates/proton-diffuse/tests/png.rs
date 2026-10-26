//! An output's head: its size and the graph the server wrote into it.

mod common;

use common::{catalogue, sdxl};
use proton_diffuse::png::{graph_of, head, made, FLOW_KEY};

/// A PNG as the image server writes one: its head, a text chunk, then the
/// pixels (which are never read, so here they are nonsense). CRCs are left
/// zero: the head is read, not checked.
fn png(width: u32, height: u32, text: Option<&str>) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend([0, 0, 0, 0]);
    };
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"tEXt", b"Software\0PIL");
    if let Some(text) = text {
        let mut data = FLOW_KEY.as_bytes().to_vec();
        data.push(0);
        data.extend(text.as_bytes());
        chunk(&mut out, b"tEXt", &data);
    }
    chunk(&mut out, b"IDAT", &[1, 2, 3]);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn an_outputs_size_and_graph_are_read_from_its_head() {
    let graph = sdxl().graph(&catalogue(), None).unwrap();
    let text = serde_json::to_string(&graph).unwrap();
    let read = head(&png(1024, 768, Some(&text))[..]).unwrap();
    assert_eq!((read.width, read.height), (1024, 768));
    assert_eq!(graph_of(read.flow.as_deref().unwrap()), Some(graph));
    // A PNG with no graph is still an image; anything else is not.
    assert_eq!(head(&png(64, 64, None)[..]).unwrap().flow, None);
    assert_eq!(head(&b"GIF89a..."[..]), None);
}

#[test]
fn what_made_an_image_is_read_from_its_sampler_prompt_and_checkpoint() {
    let graph = sdxl().graph(&catalogue(), None).unwrap();
    let said = made(&graph);
    assert_eq!(said.seed, Some(42));
    assert_eq!(said.steps, Some(30));
    assert_eq!(said.cfg, Some(6.5));
    assert_eq!(said.sampler.as_deref(), Some("dpmpp_2m"));
    assert_eq!(said.scheduler.as_deref(), Some("karras"));
    assert_eq!(said.checkpoint.as_deref(), Some("sd_xl_base_1.0.safetensors"));
    assert_eq!(said.prompt.as_deref(), Some("a lighthouse at dusk, oil painting"));
    assert_eq!(said.negative.as_deref(), Some("blurry"));
    // The run's whole body is read too, as the design's request carries it.
    let body = serde_json::json!({ "graph": graph }).to_string();
    assert_eq!(graph_of(&body).map(|g| made(&g).seed), Some(Some(42)));
}
