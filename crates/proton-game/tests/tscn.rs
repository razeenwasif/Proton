//! What reading somebody else's scene file has to get right.
//!
//! The fixture is shaped the way Godot 4 writes one, including the parts this
//! does not understand — a sub-resource, an external resource, a connection —
//! because those are most of a real scene and all of what a careless writer
//! would destroy.

use proton_game::Scene;

const SCENE: &str = r#"[gd_scene load_steps=3 format=3 uid="uid://bq2x8k1n0v"]

[ext_resource type="Texture2D" uid="uid://c7y3" path="res://art/tiles.png" id="1_x8k"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_a1b"]
size = Vector2(24, 40)

[node name="World" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(196, 286)
speed = 400.0
collision_layer = 3

[node name="Sprite" type="Sprite2D" parent="Player"]
texture = ExtResource("1_x8k")
offset = Vector2(0, -4)

[node name="Shape" type="CollisionShape2D" parent="Player"]
shape = SubResource("RectangleShape2D_a1b")

[node name="Camera" type="Camera2D" parent="."]

[connection signal="body_entered" from="Player" to="." method="_on_player_entered"]
"#;

fn scene() -> Scene {
    Scene::parse(SCENE).unwrap()
}

fn tree(scene: &Scene) -> Vec<(usize, String)> {
    scene
        .nodes()
        .into_iter()
        .map(|one| (one.depth, one.name))
        .collect()
}

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    // The promise that matters most here, because this file belongs to a tool
    // that will open it next. A reformatted `.tscn` is a whole-file diff in
    // somebody's game repository.
    assert_eq!(scene().to_text(), SCENE);
}

#[test]
fn the_hierarchy_comes_out_of_the_parent_paths() {
    assert_eq!(
        tree(&scene()),
        [
            (0, "World".into()),
            (1, "Player".into()),
            (2, "Sprite".into()),
            (2, "Shape".into()),
            (1, "Camera".into()),
        ]
    );
}

#[test]
fn a_node_knows_its_own_path_and_how_many_children_it_has() {
    let nodes = scene().nodes();
    assert_eq!(nodes[0].path, "", "the root's path is empty");
    assert_eq!(nodes[1].path, "Player");
    assert_eq!(nodes[2].path, "Player/Sprite");

    let kids: Vec<usize> = nodes.iter().map(|one| one.children).collect();
    assert_eq!(kids, [2, 2, 0, 0, 0], "{nodes:#?}");
}

#[test]
fn only_nodes_become_nodes() {
    // A scene is mostly resources and connections. Reading `[sub_resource]`
    // as a node would put a rectangle in the hierarchy.
    let names: Vec<String> = scene().nodes().into_iter().map(|one| one.name).collect();
    assert_eq!(names.len(), 5, "{names:?}");
    assert!(!names.iter().any(|n| n.contains("Rectangle")));
}

#[test]
fn properties_are_read_as_written() {
    // A `.tscn` value is a small expression language. Parsing it into a typed
    // model would mean re-emitting it and getting the spacing wrong.
    let nodes = scene().nodes();
    let player = &nodes[1];
    let named: Vec<(&str, &str)> = player
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(
        named,
        [
            ("position", "Vector2(196, 286)"),
            ("speed", "400.0"),
            ("collision_layer", "3"),
        ]
    );
    // And a resource reference keeps its whole expression.
    assert_eq!(
        nodes[3].properties[0].value,
        "SubResource(\"RectangleShape2D_a1b\")"
    );
}

#[test]
fn a_node_with_no_properties_has_none() {
    assert!(scene().nodes()[4].properties.is_empty());
}

/* ── writing ──────────────────────────────────────────────────────── */

#[test]
fn setting_a_property_changes_one_line() {
    let mut one = scene();
    assert!(one.set_property("Player", "speed", "250.0").unwrap());
    assert_eq!(
        one.to_text(),
        SCENE.replace("speed = 400.0", "speed = 250.0")
    );
}

#[test]
fn setting_a_property_on_a_deep_node_finds_the_right_one() {
    // Two nodes are called after their type and both are under Player. A
    // writer that matched on name alone would edit whichever came first.
    let mut one = scene();
    assert!(one
        .set_property("Player/Sprite", "offset", "Vector2(0, -8)")
        .unwrap());
    assert_eq!(
        one.to_text(),
        SCENE.replace("offset = Vector2(0, -4)", "offset = Vector2(0, -8)")
    );
}

#[test]
fn setting_what_is_already_there_is_not_a_change() {
    let mut one = scene();
    assert!(!one.set_property("Player", "speed", "400.0").unwrap());
    assert_eq!(one.to_text(), SCENE);
}

#[test]
fn a_property_the_node_never_had_is_added_under_the_others() {
    // Where Godot would put it. Anywhere else moves lines that did not change.
    let mut one = scene();
    assert!(one.set_property("Player", "gravity_scale", "1.5").unwrap());
    assert_eq!(
        one.to_text(),
        SCENE.replace(
            "collision_layer = 3\n",
            "collision_layer = 3\ngravity_scale = 1.5\n"
        )
    );
}

#[test]
fn a_property_added_to_a_node_that_had_none_lands_under_its_header() {
    let mut one = scene();
    assert!(one.set_property("Camera", "zoom", "Vector2(2, 2)").unwrap());
    assert!(
        one.to_text()
            .contains("type=\"Camera2D\" parent=\".\"]\nzoom = Vector2(2, 2)"),
        "{}",
        one.to_text()
    );
}

#[test]
fn a_node_that_is_not_there_says_so() {
    let mut one = scene();
    assert!(one.set_property("Ghost", "speed", "1").is_err());
    assert_eq!(one.to_text(), SCENE);
}

#[test]
fn a_value_with_a_newline_in_it_is_refused() {
    // A property is one line. Writing two would silently make the second into
    // a property of its own, or into nothing.
    let mut one = scene();
    assert!(one.set_property("Player", "speed", "1\nspeed = 2").is_err());
    assert_eq!(one.to_text(), SCENE);
}

#[test]
fn two_edits_in_a_row_both_land() {
    // The spans move after the first. A writer that kept the old ones would
    // put the second edit in the wrong place.
    let mut one = scene();
    one.set_property("Player", "speed", "250.0").unwrap();
    one.set_property("Player/Sprite", "offset", "Vector2(1, 1)")
        .unwrap();
    assert_eq!(
        one.to_text(),
        SCENE
            .replace("speed = 400.0", "speed = 250.0")
            .replace("offset = Vector2(0, -4)", "offset = Vector2(1, 1)")
    );
}

/* ── the door ─────────────────────────────────────────────────────── */

#[test]
fn a_resource_is_not_a_scene() {
    // `.tres` has the same syntax and is not a scene. Showing its contents in
    // a hierarchy would be a confident wrong answer.
    let resource = "[gd_resource type=\"Theme\" format=3]\n\n[resource]\n";
    let why = Scene::parse(resource).expect_err("not a scene");
    assert!(why.contains("resource"), "{why}");
}

#[test]
fn something_that_is_not_godot_at_all_says_so() {
    assert!(Scene::parse("{\"format\": \"proton.scene\"}").is_err());
    assert!(Scene::parse("").is_err());
}

#[test]
fn a_bracket_inside_a_value_is_not_a_section() {
    // `PackedVector2Array([0, 0])` has a `[` in it. Read as a section header
    // it would cut the node's properties in half.
    let tricky = "[gd_scene format=3]\n\n[node name=\"Poly\" type=\"Polygon2D\"]\n\
        polygon = PackedVector2Array([0, 0, 4, 0])\ncolor = Color(1, 0, 0, 1)\n";
    let one = Scene::parse(tricky).unwrap();
    let nodes = one.nodes();
    let names: Vec<&str> = nodes[0]
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["polygon", "color"]);
}

#[test]
fn a_scene_with_no_nodes_is_not_an_error() {
    // Godot writes one when a scene is created and nothing added.
    let empty = "[gd_scene load_steps=1 format=3]\n";
    assert!(Scene::parse(empty).unwrap().nodes().is_empty());
}

/// Two nodes called `Sprite`, under different parents. Godot allows this and
/// it is the norm: every character has one.
const TWINS: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="A"]

[node name="B" type="Node2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="B"]
"#;

#[test]
fn two_nodes_with_the_same_name_are_told_apart() {
    let one = Scene::parse(TWINS).unwrap();
    let paths: Vec<String> = one.nodes().into_iter().map(|n| n.path).collect();
    assert_eq!(paths, ["", "A", "A/Sprite", "B", "B/Sprite"]);
}

#[test]
fn adding_a_property_finds_the_right_one_of_two_nodes_with_a_name() {
    // Finding the section by name alone puts the new property on whichever
    // came first in the file — here, on `A/Sprite`, silently.
    let mut one = Scene::parse(TWINS).unwrap();
    assert!(one.set_property("B/Sprite", "flip_h", "true").unwrap());

    let out = one.to_text();
    let at = out.find("flip_h").expect("it was written");
    assert!(
        out[..at].contains(r#"parent="B""#),
        "the property landed on the wrong Sprite:\n{out}"
    );
}
