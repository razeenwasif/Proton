//! Edits: the design's flow built by ops, and the graph kept sound.

mod common;

use std::collections::BTreeMap;

use common::{catalogue, design_block, sdxl, sdxl_ops};
use proton_diffuse::op::apply;
use proton_diffuse::{Flow, Op, SeedMode};
use serde_json::{json, Value};

fn run(flow: &mut Flow, ops: &[Op]) -> Result<proton_diffuse::Applied, String> {
    apply(flow, ops, &catalogue())
}

fn connect(from: &str, to: &str) -> Op {
    Op::Connect { from: from.into(), to: to.into() }
}

fn set(id: &str, name: &str, value: Value) -> Op {
    Op::Set { id: id.into(), name: name.into(), value }
}

/// Numbers as doubles, links in order: `1` and `1.0` are one denoise.
fn same(value: &Value) -> Value {
    match value {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(items) => {
            let mut items: Vec<Value> = items.iter().map(same).collect();
            items.sort_by_key(|v| v.to_string());
            Value::Array(items)
        }
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), same(v))).collect()),
        other => other.clone(),
    }
}

#[test]
fn the_flow_built_by_ops_sends_the_designs_run() {
    let flow = sdxl();
    let graph = flow.graph(&catalogue(), None).unwrap();
    let sent = serde_json::to_value(&graph).unwrap();
    assert_eq!(same(&sent), same(&design_block("### A run")["graph"]));
}

#[test]
fn adding_names_the_node_and_fills_its_defaults() {
    let mut flow = Flow::default();
    let added = run(&mut flow, &sdxl_ops()[..5]).unwrap();
    assert_eq!(added.added, ["1", "2", "3", "4", "5"]);
    let latent = flow.node("4").unwrap();
    assert_eq!(
        latent.values,
        serde_json::from_value::<BTreeMap<String, Value>>(json!({"width": 1024, "height": 1024, "batch": 1})).unwrap()
    );
    // What has no default is left for the person, but for a seed.
    let mut fresh = Flow::default();
    run(&mut fresh, &[Op::Add { kind: "KSampler".into(), at: [0.0, 0.0], values: BTreeMap::new() }]).unwrap();
    let sampler = fresh.node("1").unwrap();
    assert_eq!(sampler.values["seed"], json!(0));
    assert!(!sampler.values.contains_key("sampler"));
    assert_eq!(sampler.values["steps"], json!(30));
    // Removing a node frees nothing: the next id is still one past the top.
    run(&mut flow, &[Op::Remove { ids: vec!["5".into()] }]).unwrap();
    let again =
        run(&mut flow, &[Op::Add { kind: "PreviewImage".into(), at: [0.0, 0.0], values: BTreeMap::new() }]).unwrap();
    assert_eq!(again.added, ["5"]);
    assert_eq!(
        run(&mut flow, &[Op::Add { kind: "Teleport".into(), at: [0.0, 0.0], values: BTreeMap::new() }]).unwrap_err(),
        "the server has no Teleport node"
    );
}

#[test]
fn a_link_joins_ports_that_carry_the_same_type() {
    let mut flow = sdxl();
    assert_eq!(run(&mut flow, &[connect("1:clip", "5:model")]).unwrap_err(), "model takes MODEL, and clip makes CLIP");
    assert_eq!(run(&mut flow, &[connect("1:noise", "5:model")]).unwrap_err(), "Load Checkpoint has no output noise");
    assert_eq!(run(&mut flow, &[connect("1:model", "5:weight")]).unwrap_err(), "KSampler has no input weight");
    assert_eq!(run(&mut flow, &[connect("4:latent", "5:seed")]).unwrap_err(), "seed is set on the node, not linked");
    assert_eq!(run(&mut flow, &[connect("9:model", "5:model")]).unwrap_err(), "there is no node 9");
    assert_eq!(
        run(&mut flow, &[connect("1model", "5:model")]).unwrap_err(),
        "1model → 5:model is not a link between two ports"
    );
}

#[test]
fn a_second_link_into_an_input_takes_the_firsts_place() {
    let mut flow = sdxl();
    // The negative prompt's encoder feeds the positive input now.
    let applied = run(&mut flow, &[connect("3:conditioning", "5:positive")]).unwrap();
    assert!(applied.changed);
    let into: Vec<_> = flow.links.iter().filter(|l| l.to == "5:positive").collect();
    assert_eq!(into.len(), 1);
    assert_eq!(into[0].from, "3:conditioning");
    // The same link again changes nothing.
    assert!(!run(&mut flow, &[connect("3:conditioning", "5:positive")]).unwrap().changed);
}

#[test]
fn no_link_closes_a_loop() {
    let mut flow = sdxl();
    // A decode fed back through an encode into the sampler it came from.
    run(&mut flow, &[Op::Add { kind: "VAEEncode".into(), at: [0.0, 400.0], values: BTreeMap::new() }]).unwrap();
    run(&mut flow, &[connect("6:image", "8:image")]).unwrap();
    assert_eq!(
        run(&mut flow, &[connect("8:latent", "5:latent")]).unwrap_err(),
        "linking 8:latent to 5:latent would make a loop"
    );
    // A node into itself is a loop too, where the types would allow it.
    run(&mut flow, &[Op::Add { kind: "InvertMask".into(), at: [0.0, 0.0], values: BTreeMap::new() }]).unwrap();
    assert!(run(&mut flow, &[connect("9:mask", "9:mask")]).unwrap_err().contains("loop"));
}

#[test]
fn a_batch_is_whole_or_nothing() {
    let mut flow = sdxl();
    let before = flow.clone();
    let ops = [Op::Place { id: "1".into(), at: [40.0, 40.0] }, connect("1:clip", "5:model")];
    assert!(run(&mut flow, &ops).is_err());
    assert_eq!(flow, before);
}

#[test]
fn removing_a_node_takes_its_links_with_it() {
    let mut flow = sdxl();
    run(&mut flow, &[Op::Remove { ids: vec!["1".into()] }]).unwrap();
    assert!(flow.node("1").is_none());
    assert!(flow.links.iter().all(|l| !l.from.starts_with("1:") && !l.to.starts_with("1:")));
    assert_eq!(flow.links.len(), 9 - 4);
    assert_eq!(run(&mut flow, &[Op::Remove { ids: vec!["1".into()] }]).unwrap_err(), "there is no node 1");
}

#[test]
fn a_field_is_set_as_its_kind_allows_and_null_puts_back_the_default() {
    let mut flow = sdxl();
    run(&mut flow, &[set("5", "steps", json!(40)), set("5", "cfg", json!(7.5))]).unwrap();
    assert_eq!(flow.node("5").unwrap().values["steps"], json!(40));
    assert_eq!(run(&mut flow, &[set("5", "steps", json!(0))]).unwrap_err(), "steps is at least 1");
    assert_eq!(run(&mut flow, &[set("5", "model", json!("x"))]).unwrap_err(), "model is linked, not set");
    assert_eq!(run(&mut flow, &[set("5", "eta", json!(1))]).unwrap_err(), "KSampler has no field eta");
    assert_eq!(run(&mut flow, &[set("9", "steps", json!(1))]).unwrap_err(), "there is no node 9");
    run(&mut flow, &[set("5", "steps", Value::Null)]).unwrap();
    assert!(!flow.node("5").unwrap().values.contains_key("steps"));
    // Sent, it takes the catalogue's default.
    let graph = flow.graph(&catalogue(), None).unwrap();
    assert_eq!(graph.nodes.iter().find(|n| n.id == "5").unwrap().values["steps"], json!(30));
}

#[test]
fn duplicating_copies_the_nodes_and_the_links_among_them() {
    let mut flow = sdxl();
    let applied = run(&mut flow, &[Op::Duplicate { ids: vec!["5".into(), "6".into()], by: [0.0, 300.0] }]).unwrap();
    assert_eq!(applied.added, ["8", "9"]);
    let copy = flow.node("8").unwrap();
    assert_eq!(copy.kind, "KSampler");
    assert_eq!(copy.at, [408.0, 316.0]);
    assert_eq!(copy.values, flow.node("5").unwrap().values);
    // The link between the two is copied; links from outside are not.
    assert!(flow.links.iter().any(|l| l.from == "8:latent" && l.to == "9:latent"));
    assert!(flow.links.iter().all(|l| l.to != "8:model" && l.to != "9:vae"));
}

#[test]
fn seed_bypass_note_and_outputs_are_the_flows_own() {
    let mut flow = sdxl();
    run(
        &mut flow,
        &[
            Op::Seed { id: "5".into(), mode: SeedMode::Random },
            Op::Bypass { id: "6".into(), on: true },
            Op::Note { text: Some("Cover art for the harbour level.".into()) },
            Op::Outputs { path: Some("art/covers/".into()) },
        ],
    )
    .unwrap();
    assert_eq!(flow.node("5").unwrap().seed, Some(SeedMode::Random));
    assert!(flow.node("6").unwrap().bypass.0);
    assert_eq!(flow.outputs.as_deref(), Some("art/covers"));
    // Fixed is what nothing said means.
    run(&mut flow, &[Op::Seed { id: "5".into(), mode: SeedMode::Fixed }, Op::Note { text: Some("  ".into()) }])
        .unwrap();
    assert_eq!(flow.node("5").unwrap().seed, None);
    assert_eq!(flow.note, None);
    for outside in ["../elsewhere", "/Users/someone", "art/../../x", "art\\covers"] {
        assert!(run(&mut flow, &[Op::Outputs { path: Some(outside.into()) }]).is_err(), "{outside}");
    }
}

#[test]
fn ops_read_as_the_window_sends_them() {
    let ops: Vec<Op> = serde_json::from_value(json!([
        {"op": "add", "type": "KSampler", "at": [10, 20]},
        {"op": "place", "id": "1", "at": [30.5, 40]},
        {"op": "set", "id": "1", "name": "steps", "value": 12},
        {"op": "connect", "from": "2:latent", "to": "1:latent"},
        {"op": "disconnect", "to": "1:latent"},
        {"op": "seed", "id": "1", "mode": "increment"},
        {"op": "bypass", "id": "1", "on": true},
        {"op": "note"},
        {"op": "outputs", "path": null}
    ]))
    .unwrap();
    assert_eq!(ops.len(), 9);
    assert_eq!(ops[0], Op::Add { kind: "KSampler".into(), at: [10.0, 20.0], values: BTreeMap::new() });
}
