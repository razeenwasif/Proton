//! The v1 catalogue: the fixture is the design's, and it reads as the
//! server will send it.

mod common;

use common::{catalogue, design_block, models};
use proton_diffuse::{Catalogue, Kind};
use serde_json::json;

#[test]
fn the_fixture_is_the_catalogue_in_the_design() {
    // Nexus's tests hold the same JSON: the design is where both copy it
    // from, so a change to it that one side has not made fails here.
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/catalogue-v1.json")).unwrap();
    assert_eq!(fixture, design_block("### The v1 catalogue"));
}

#[test]
fn it_reads_as_sixteen_nodes_and_writes_back_as_it_read() {
    let catalogue = catalogue();
    assert_eq!(catalogue.version, 1);
    assert_eq!(catalogue.nodes.len(), 16);
    // The copy kept in `.proton/diffuse/` reads back as the same catalogue.
    let kept = serde_json::to_string(&catalogue).unwrap();
    assert_eq!(Catalogue::parse(&kept).unwrap(), catalogue);
}

#[test]
fn every_socket_is_a_type_it_names_and_every_choice_has_a_list() {
    let catalogue = catalogue();
    let model_lists = ["checkpoints", "loras", "vaes", "upscalers"];
    for node in &catalogue.nodes {
        for socket in node.inputs.iter().filter_map(|i| i.socket.as_ref()).chain(node.outputs.iter().map(|o| &o.socket))
        {
            assert!(catalogue.types.contains(socket), "{}: {socket}", node.name);
        }
        for input in node.fields() {
            assert_ne!(input.kind(), Kind::Other, "{}.{}", node.name, input.name);
            if input.kind() == Kind::Choice {
                let from = input.from.as_deref().unwrap();
                assert!(
                    catalogue.lists.contains_key(from) || model_lists.contains(&from),
                    "{}.{}",
                    node.name,
                    input.name
                );
            }
        }
    }
    assert!(catalogue.node("SaveImage").unwrap().is_output());
    assert!(!catalogue.node("KSampler").unwrap().is_output());
}

#[test]
fn a_choice_comes_from_the_catalogues_lists_or_the_servers_models() {
    let catalogue = catalogue();
    let models = models();
    assert!(catalogue.choices("samplers", None).unwrap().contains(&"dpmpp_2m".to_string()));
    assert_eq!(
        catalogue.choices("checkpoints", Some(&models)).unwrap(),
        vec!["sd_xl_base_1.0.safetensors", "flux1-dev-fp8.safetensors"]
    );
    // Not fetched yet is not the same as none.
    assert_eq!(catalogue.choices("checkpoints", None), None);
    assert_eq!(catalogue.choices("vaes", Some(&models)), Some(vec![]));
}

#[test]
fn a_newer_servers_kind_of_field_is_kept_and_never_refused() {
    let mut tree: serde_json::Value = serde_json::from_str(include_str!("fixtures/catalogue-v1.json")).unwrap();
    tree["nodes"][0]["inputs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "tint", "kind": "colour", "default": "#ff0000"}));
    let catalogue: Catalogue = serde_json::from_value(tree).unwrap();
    let tint = catalogue.nodes[0].input("tint").unwrap();
    assert_eq!(tint.kind(), Kind::Other);
    assert_eq!(tint.refuse(&json!(12)), None);
    let written = serde_json::to_value(&catalogue).unwrap();
    assert_eq!(written["nodes"][0]["inputs"][1]["kind"], "colour");
}

#[test]
fn a_field_refuses_the_wrong_kind_of_value_and_one_out_of_range() {
    let catalogue = catalogue();
    let sampler = catalogue.node("KSampler").unwrap();
    let field = |name: &str| sampler.input(name).unwrap();
    assert_eq!(field("steps").refuse(&json!(30)), None);
    assert_eq!(field("steps").refuse(&json!(0)).unwrap(), "steps is at least 1");
    assert_eq!(field("steps").refuse(&json!(151)).unwrap(), "steps is at most 150");
    assert_eq!(field("steps").refuse(&json!(2.5)).unwrap(), "steps is a whole number");
    // A latent's size is a multiple of 8, as Nexus refuses it.
    let latent = catalogue.node("EmptyLatent").unwrap();
    assert_eq!(
        latent.input("height").unwrap().refuse(&json!(1964)).unwrap(),
        "height must be a multiple of 8: 1960 or 1968, not 1964"
    );
    assert_eq!(latent.input("width").unwrap().refuse(&json!(1216)), None);
    assert_eq!(field("cfg").refuse(&json!(6.5)), None);
    assert_eq!(field("cfg").refuse(&json!("high")).unwrap(), "cfg is a number");
    assert_eq!(field("sampler").refuse(&json!(3)).unwrap(), "sampler is text");
    assert_eq!(field("seed").refuse(&json!(9_007_199_254_740_991u64)), None);
    assert!(field("seed").refuse(&json!(9_007_199_254_740_992u64)).is_some());
    assert!(field("seed").refuse(&json!(-1)).is_some());
}
