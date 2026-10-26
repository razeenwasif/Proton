//! What stops a flow running, and what does not.

mod common;

use std::collections::BTreeMap;

use common::{catalogue, models, sdxl};
use proton_diffuse::op::apply;
use proton_diffuse::{check, Flow, Op, Problem};
use serde_json::{json, Value};

fn run(flow: &mut Flow, ops: &[Op]) {
    apply(flow, ops, &catalogue()).unwrap();
}

fn add(kind: &str) -> Op {
    Op::Add { kind: kind.into(), at: [0.0, 0.0], values: BTreeMap::new() }
}

fn messages(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|p| {
            format!(
                "{}{}: {}",
                p.node.as_deref().unwrap_or("-"),
                p.input.as_deref().map(|i| format!(".{i}")).unwrap_or_default(),
                p.message
            )
        })
        .collect()
}

#[test]
fn the_designs_flow_is_ready_to_run_with_or_without_the_model_lists() {
    let flow = sdxl();
    assert_eq!(check(&flow, &catalogue(), None), vec![]);
    assert_eq!(check(&flow, &catalogue(), Some(&models())), vec![]);
}

#[test]
fn an_unfinished_node_stops_a_run_only_when_the_run_needs_it() {
    let mut flow = sdxl();
    // A LoRA loader off to one side, linked to nothing: not needed.
    run(&mut flow, &[add("LoadLoRA")]);
    assert_eq!(check(&flow, &catalogue(), None), vec![]);
    // Taking the sampler's model away is another matter.
    run(&mut flow, &[Op::Disconnect { to: "5:model".into() }]);
    assert_eq!(messages(&check(&flow, &catalogue(), None)), ["5.model: model has no link"]);
    // An optional input may go unlinked.
    run(
        &mut flow,
        &[Op::Connect { from: "1:model".into(), to: "5:model".into() }, Op::Disconnect { to: "5:negative".into() }],
    );
    assert_eq!(check(&flow, &catalogue(), None), vec![]);
}

#[test]
fn a_field_with_no_value_and_no_default_must_be_filled() {
    let mut flow = sdxl();
    run(&mut flow, &[Op::Set { id: "5".into(), name: "sampler".into(), value: Value::Null }]);
    run(&mut flow, &[Op::Set { id: "1".into(), name: "checkpoint".into(), value: Value::Null }]);
    assert_eq!(
        messages(&check(&flow, &catalogue(), None)),
        ["1.checkpoint: choose a checkpoint", "5.sampler: choose a sampler"]
    );
}

#[test]
fn a_model_the_server_lacks_is_caught_once_its_models_are_known() {
    let mut flow = sdxl();
    run(&mut flow, &[Op::Set { id: "1".into(), name: "checkpoint".into(), value: json!("dreamshaper_8.safetensors") }]);
    assert_eq!(check(&flow, &catalogue(), None), vec![]);
    assert_eq!(
        messages(&check(&flow, &catalogue(), Some(&models()))),
        ["1.checkpoint: dreamshaper_8.safetensors is not one of the server's checkpoints"]
    );
    // A sampler is checked always: its list is the catalogue's own.
    let mut flow = sdxl();
    flow.node_mut("5").unwrap().values.insert("sampler".into(), json!("plms"));
    assert_eq!(messages(&check(&flow, &catalogue(), None)), ["5.sampler: plms is not one of the server's samplers"]);
}

#[test]
fn a_flow_with_no_output_has_nothing_to_run() {
    let mut flow = sdxl();
    run(&mut flow, &[Op::Remove { ids: vec!["7".into()] }]);
    assert_eq!(
        messages(&check(&flow, &catalogue(), None)),
        ["-: nothing to run: no output node such as Save Image or Preview Image"]
    );
    assert!(flow.graph(&catalogue(), None).is_err());
}

#[test]
fn a_bypassed_node_passes_what_fed_it_straight_through() {
    let mut flow = sdxl();
    // A LoRA between the checkpoint and the sampler and encoders, bypassed.
    run(
        &mut flow,
        &[
            add("LoadLoRA"),
            Op::Set { id: "8".into(), name: "lora".into(), value: json!("impasto.safetensors") },
            Op::Connect { from: "1:model".into(), to: "8:model".into() },
            Op::Connect { from: "1:clip".into(), to: "8:clip".into() },
            Op::Connect { from: "8:model".into(), to: "5:model".into() },
            Op::Connect { from: "8:clip".into(), to: "2:clip".into() },
            Op::Bypass { id: "8".into(), on: true },
        ],
    );
    assert_eq!(check(&flow, &catalogue(), None), vec![]);
    let graph = flow.graph(&catalogue(), None).unwrap();
    assert!(graph.nodes.iter().all(|n| n.id != "8"));
    assert!(graph.links.iter().any(|l| l.from == "1:model" && l.to == "5:model"));
    assert!(graph.links.iter().any(|l| l.from == "1:clip" && l.to == "2:clip"));
    // On, it runs, and the graph goes through it.
    run(&mut flow, &[Op::Bypass { id: "8".into(), on: false }]);
    let graph = flow.graph(&catalogue(), None).unwrap();
    assert!(graph.links.iter().any(|l| l.from == "8:model" && l.to == "5:model"));
}

#[test]
fn bypassing_a_node_with_nothing_to_pass_leaves_its_inputs_unfed() {
    let mut flow = sdxl();
    // The decode makes an image from a latent: there is no image to pass on.
    run(&mut flow, &[Op::Bypass { id: "6".into(), on: true }]);
    assert_eq!(messages(&check(&flow, &catalogue(), None)), ["7.images: images has no link"]);
}

#[test]
fn a_broken_file_says_what_is_broken() {
    let mut flow = sdxl();
    flow.nodes.push(flow.node("4").unwrap().clone());
    flow.nodes.push(proton_diffuse::Node {
        kind: "Teleport".into(),
        id: "9".into(),
        ..flow.node("4").unwrap().clone()
    });
    flow.links.push(proton_diffuse::Link { from: "4:latent".into(), to: "6:latent".into() });
    flow.links.push(proton_diffuse::Link { from: "1:clip".into(), to: "5:model".into() });
    let found = messages(&check(&flow, &catalogue(), None));
    for expected in [
        "4: two nodes are called 4",
        "9: the server has no Teleport node",
        "6.latent: latent has two links into it",
        "5.model: model has two links into it",
        "5.model: model takes MODEL, and clip makes CLIP",
    ] {
        assert!(found.iter().any(|f| f == expected), "{expected} in {found:?}");
    }
}

#[test]
fn a_loop_made_by_hand_is_found() {
    let mut flow = sdxl();
    run(&mut flow, &[add("VAEEncode"), Op::Connect { from: "6:image".into(), to: "8:image".into() }]);
    flow.links.retain(|l| l.to != "5:latent");
    flow.links.push(proton_diffuse::Link { from: "8:latent".into(), to: "5:latent".into() });
    let found = messages(&check(&flow, &catalogue(), None));
    for id in ["5", "6", "8"] {
        assert!(found.contains(&format!("{id}: feeds itself through a loop of links")), "{found:?}");
    }
    assert!(!found.iter().any(|f| f.starts_with("1: feeds")));
}
