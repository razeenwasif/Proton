//! A graph out of a PNG, laid out and opened: the same graph again, its
//! wires running left to right, no card on another.

mod common;

use common::{catalogue, sdxl};
use proton_diffuse::layout::{flow_from_graph, height};
use proton_diffuse::op::apply;
use proton_diffuse::Flow;

#[test]
fn a_graph_laid_out_is_the_same_graph_and_its_wires_run_left_to_right() {
    for name in ["textToImage", "hiresFix", "inpaint", "flux"] {
        let mut start = Flow::default();
        apply(&mut start, &proton_diffuse::templates::ops(name).unwrap(), &catalogue()).unwrap();
        // As a run sends it, and as forge writes it into a PNG: no places.
        let mut sent = start.graph(&catalogue(), None).ok();
        if sent.is_none() {
            // Image inputs wait for a picture: the shape is what matters here.
            start.node_mut("4").unwrap().values.insert("image".into(), serde_json::json!("up_1"));
            sent = start.graph(&catalogue(), None).ok();
        }
        let sent = sent.unwrap();
        let flow = flow_from_graph(&sent, &catalogue());
        // The same graph: every value, every link.
        assert_eq!(flow.graph(&catalogue(), None).unwrap(), sent, "{name}");
        // Every wire goes right; no card sits on another.
        for link in &flow.links {
            let at = |port: &str| flow.node(port.split(':').next().unwrap()).unwrap().at;
            assert!(at(&link.from)[0] < at(&link.to)[0], "{name}: {} → {} goes back", link.from, link.to);
        }
        for (i, a) in flow.nodes.iter().enumerate() {
            for b in &flow.nodes[i + 1..] {
                let same_column = a.at[0] == b.at[0];
                let (top, bottom) = if a.at[1] < b.at[1] { (a, b) } else { (b, a) };
                assert!(
                    !same_column || top.at[1] + height(&catalogue(), &top.kind) <= bottom.at[1],
                    "{name}: {} overlaps {}",
                    top.id,
                    bottom.id
                );
            }
        }
    }
}

#[test]
fn the_designs_flow_round_trips_through_a_png() {
    // Out of the flow as a run sends it, into a PNG's text as forge writes
    // it, back out and laid out: the same run.
    let sent = sdxl().graph(&catalogue(), None).unwrap();
    let text = serde_json::to_string(&sent).unwrap();
    let read = proton_diffuse::png::graph_of(&text).unwrap();
    let flow = flow_from_graph(&read, &catalogue());
    assert_eq!(flow.graph(&catalogue(), None).unwrap(), sent);
    // The sampler is right of the encoders, which are right of the loader.
    let x = |id: &str| flow.node(id).unwrap().at[0];
    assert!(x("1") < x("2") && x("2") < x("5") && x("5") < x("6") && x("6") < x("7"));
}
