//! What a material graph has to do before a panel draws one.

use proton_scene::{ParamValue, Scene};

const SCENE: &str = r#"{
  "format": "proton.scene",
  "materials": [
    {
      "id": "alu",
      "links": [
        {
          "from": "noise-1:fac",
          "to": "bsdf-1:roughness"
        },
        {
          "from": "bsdf-1:bsdf",
          "to": "out-1:surface"
        }
      ],
      "name": "Brushed aluminium",
      "nodes": [
        {
          "at": [20, 30],
          "id": "noise-1",
          "kind": "noise",
          "params": {
            "detail": 2,
            "scale": 6.4
          }
        },
        {
          "at": [220, 40],
          "id": "bsdf-1",
          "kind": "principled",
          "params": {
            "baseColor": [0.55, 0.56, 0.58],
            "metallic": 0.9,
            "roughness": 0.24
          }
        },
        {
          "at": [440, 60],
          "id": "out-1",
          "kind": "output"
        }
      ]
    }
  ],
  "objects": [
    {
      "id": "body-1",
      "kind": "mesh",
      "material": "alu",
      "name": "Body"
    }
  ],
  "version": 1
}
"#;

fn scene() -> Scene {
    Scene::parse(SCENE).unwrap()
}

#[test]
fn a_material_reads_back_as_a_graph() {
    let one = &scene().materials()[0];
    assert_eq!(one.name, "Brushed aluminium");
    assert_eq!(
        one.nodes
            .iter()
            .map(|n| n.kind.as_str())
            .collect::<Vec<_>>(),
        ["noise", "principled", "output"]
    );
    assert_eq!(one.links.len(), 2);
    assert_eq!(one.nodes[0].at, [20.0, 30.0]);

    // Params come back sorted, so a panel draws a node's settings in the same
    // order twice.
    let bsdf = &one.nodes[1];
    assert_eq!(
        bsdf.params
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["baseColor", "metallic", "roughness"]
    );
    assert_eq!(bsdf.params[0].value, ParamValue::Color([0.55, 0.56, 0.58]));
    assert_eq!(bsdf.params[1].value, ParamValue::Number(0.9));
}

#[test]
fn an_object_finds_the_material_it_names() {
    assert_eq!(scene().material_of("body-1").unwrap().id, "alu");
    // And one that names nothing, or names something absent, gets nothing
    // rather than the first material in the file.
    assert!(scene().material_of("nope").is_none());
}

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    assert_eq!(scene().to_text(), SCENE);
}

#[test]
fn setting_a_param_moves_one_line() {
    let mut one = scene();
    assert!(one
        .set_param("alu", "bsdf-1", "roughness", ParamValue::Number(0.4))
        .unwrap());
    assert_eq!(
        one.to_text(),
        SCENE.replace("\"roughness\": 0.24", "\"roughness\": 0.4")
    );
}

#[test]
fn setting_a_colour_writes_it_on_one_line() {
    // The reason the writer keeps number arrays compact. Three lines for a
    // colour would make every tweak a three-line hunk.
    let mut one = scene();
    assert!(one
        .set_param(
            "alu",
            "bsdf-1",
            "baseColor",
            ParamValue::Color([0.8, 0.2, 0.2])
        )
        .unwrap());
    assert!(
        one.to_text().contains("\"baseColor\": [0.8, 0.2, 0.2]"),
        "{}",
        one.to_text()
    );
}

#[test]
fn setting_what_is_already_there_is_not_a_change() {
    let mut one = scene();
    assert!(!one
        .set_param("alu", "bsdf-1", "metallic", ParamValue::Number(0.9))
        .unwrap());
    // And neither is a difference too small to be written down.
    assert!(!one
        .set_param(
            "alu",
            "bsdf-1",
            "metallic",
            ParamValue::Number(0.9000000001)
        )
        .unwrap());
}

#[test]
fn a_param_the_node_never_had_is_added() {
    // A node need not write down a setting it has never been given, so the
    // first time one is set it will not be there.
    let mut one = scene();
    assert!(one
        .set_param("alu", "out-1", "displacement", ParamValue::Number(0.0))
        .unwrap());
    assert!(
        one.to_text().contains("\"displacement\": 0"),
        "{}",
        one.to_text()
    );
}

#[test]
fn moving_a_node_writes_where_it_moved_to() {
    let mut one = scene();
    assert!(one.move_node("alu", "noise-1", [40.0, 90.0]).unwrap());
    assert_eq!(
        one.to_text(),
        SCENE.replace("\"at\": [20, 30]", "\"at\": [40, 90]")
    );
}

/* ── wiring ───────────────────────────────────────────────────────── */

#[test]
fn a_rewire_is_one_line() {
    // The reason links are their own flat list rather than a field on the node
    // they arrive at — and the reason `connect` edits an existing wire in
    // place instead of removing it and appending a new one.
    //
    // A *rewire*: something else takes over an input that already had a wire.
    let mut one = scene();
    assert!(one
        .connect("alu", "out-1:surface", "bsdf-1:roughness")
        .is_err());
    assert!(one
        .connect("alu", "bsdf-1:bsdf", "bsdf-1:roughness")
        .is_err());
    // `noise-1` has another output to offer, and no loop.
    assert!(one
        .connect("alu", "noise-1:color", "bsdf-1:roughness")
        .unwrap());

    assert_eq!(
        one.to_text(),
        SCENE.replace("\"from\": \"noise-1:fac\"", "\"from\": \"noise-1:color\""),
        "a rewire moved more than the one line that changed"
    );
}

#[test]
fn a_new_wire_leaves_the_others_where_they_were() {
    // Appending is right for a wire into an input that had none — what must
    // not happen is the existing wires shuffling to make room.
    let mut one = scene();
    let before = one.materials()[0].links.clone();
    assert!(one
        .connect("alu", "noise-1:fac", "bsdf-1:metallic")
        .unwrap());

    let after = one.materials()[0].links.clone();
    assert_eq!(&after[..before.len()], &before[..], "the old wires moved");
    assert_eq!(after.len(), before.len() + 1);
}

#[test]
fn an_input_takes_one_wire() {
    // Two things feeding one socket has no answer for what the value is.
    // Keeping both would quietly produce one.
    let mut one = scene();
    one.connect("alu", "noise-1:fac", "bsdf-1:metallic")
        .unwrap();
    one.connect("alu", "bsdf-1:bsdf", "bsdf-1:metallic")
        .unwrap_err();

    let links = &one.materials()[0].links;
    let into: Vec<&str> = links
        .iter()
        .filter(|l| l.to == "bsdf-1:metallic")
        .map(|l| l.from.as_str())
        .collect();
    assert_eq!(into, ["noise-1:fac"]);
}

#[test]
fn a_wire_that_would_close_a_loop_is_refused() {
    // A graph with a loop has no order to evaluate in. The honest moment to
    // say so is when somebody draws the wire.
    let mut one = scene();
    // `bsdf-1` already reaches `out-1`, so feeding bsdf from out closes it.
    let why = one
        .connect("alu", "out-1:surface", "bsdf-1:roughness")
        .expect_err("a loop");
    assert!(why.contains("loop"), "{why}");

    assert!(one
        .connect("alu", "bsdf-1:bsdf", "bsdf-1:metallic")
        .is_err());
    assert_eq!(one.to_text(), SCENE, "it wrote anyway");
}

#[test]
fn a_wire_to_a_node_that_is_not_there_is_refused() {
    let mut one = scene();
    assert!(one
        .connect("alu", "ghost-1:fac", "bsdf-1:metallic")
        .is_err());
    assert!(one.connect("alu", "noise-1:fac", "ghost-1:x").is_err());
    // And something that is not a port at all.
    assert!(one.connect("alu", "noise-1", "bsdf-1:metallic").is_err());
}

#[test]
fn disconnecting_removes_the_wire_and_says_whether_there_was_one() {
    let mut one = scene();
    assert!(one.disconnect("alu", "bsdf-1:roughness").unwrap());
    assert!(!one.disconnect("alu", "bsdf-1:roughness").unwrap());
    assert_eq!(one.materials()[0].links.len(), 1);
}

#[test]
fn a_graph_with_no_links_yet_can_be_wired() {
    // A material written by hand, or by a plugin, need not have a links list.
    let bare = r#"{"format":"proton.scene","version":1,"objects":[],
      "materials":[{"id":"m","name":"M","nodes":[
        {"id":"a","kind":"noise"},{"id":"b","kind":"output"}]}]}"#;
    let mut one = Scene::parse(bare).unwrap();
    assert!(one.connect("m", "a:fac", "b:surface").unwrap());
    assert_eq!(one.materials()[0].links.len(), 1);
}

#[test]
fn a_material_that_is_not_there_says_so() {
    let mut one = scene();
    assert!(one.connect("nope", "a:x", "b:y").is_err());
    assert!(one
        .set_param("nope", "a", "x", ParamValue::Number(1.0))
        .is_err());
    assert!(one.move_node("alu", "nope", [0.0, 0.0]).is_err());
}

#[test]
fn a_scene_with_no_materials_is_not_an_error() {
    // Most scenes will not have one for a while.
    let bare = r#"{"format":"proton.scene","version":1,"objects":[]}"#;
    assert!(Scene::parse(bare).unwrap().materials().is_empty());
}

#[test]
fn moving_a_wire_is_one_line_and_leaves_one_wire() {
    // Picking a wire up and dropping it on another input is one gesture and
    // one link whose `to` changed. Done as a disconnect and a connect it would
    // be two writes and a worse diff — and the first version of the gesture
    // sent only the connect, which left the old wire attached and turned one
    // drag into two wires.
    let mut one = scene();
    let before = one.materials()[0].links.len();
    assert!(one
        .rewire("alu", "bsdf-1:roughness", "bsdf-1:metallic")
        .unwrap());

    assert_eq!(
        one.materials()[0].links.len(),
        before,
        "a wire was duplicated"
    );
    assert_eq!(
        one.to_text(),
        SCENE.replace(
            "\"to\": \"bsdf-1:roughness\"",
            "\"to\": \"bsdf-1:metallic\""
        )
    );
}

#[test]
fn moving_a_wire_onto_an_occupied_input_displaces_what_was_there() {
    let mut one = scene();
    // `bsdf-1:bsdf` feeds `out-1:surface`; move `bsdf-1:roughness`'s wire onto
    // that same input and the old one goes.
    assert!(one
        .rewire("alu", "bsdf-1:roughness", "out-1:surface")
        .unwrap());
    let links = &one.materials()[0].links;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].from, "noise-1:fac");
    assert_eq!(links[0].to, "out-1:surface");
}

#[test]
fn moving_a_wire_that_would_loop_is_refused() {
    let mut one = scene();
    // `noise-1:fac` feeds `bsdf-1:roughness`. Moving it to `noise-1:scale`
    // would make noise feed itself.
    assert!(one
        .rewire("alu", "bsdf-1:roughness", "noise-1:scale")
        .is_err());
    assert_eq!(one.to_text(), SCENE);
}

#[test]
fn moving_a_wire_from_an_input_that_has_none_says_so() {
    let mut one = scene();
    let why = one
        .rewire("alu", "bsdf-1:metallic", "bsdf-1:baseColor")
        .expect_err("nothing to move");
    assert!(why.contains("nothing feeds"), "{why}");
}

#[test]
fn putting_a_wire_back_where_it_was_is_not_a_change() {
    let mut one = scene();
    assert!(!one
        .rewire("alu", "bsdf-1:roughness", "bsdf-1:roughness")
        .unwrap());
}
