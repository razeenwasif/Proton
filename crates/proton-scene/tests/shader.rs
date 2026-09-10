//! What compiling a material graph has to get right.
//!
//! The graph is the file's; the program is what a renderer runs. What is
//! checked here is the translation between them — which instructions come out,
//! in what order, and what happens to a graph that cannot be translated at
//! all. How the instructions then *look* is the renderer's business and is
//! checked by driving one.

use proton_scene::{Op, Scene};

/// A material shaped the way the panel builds one: a noise through a ramp into
/// the roughness of a principled surface, and the whole thing into an output.
const SCENE: &str = r#"{
  "format": "proton.scene",
  "version": 1,
  "objects": [
    { "id": "a", "name": "A", "kind": "mesh", "material": "alu" }
  ],
  "materials": [
    {
      "id": "alu",
      "name": "Brushed aluminium",
      "nodes": [
        { "id": "noise-1", "kind": "noise", "at": [0, 0],
          "params": { "scale": 8.0, "detail": 3.0 } },
        { "id": "ramp-1", "kind": "ramp", "at": [1, 0],
          "params": { "from": [0.1, 0.1, 0.1], "to": [0.9, 0.9, 0.9] } },
        { "id": "bsdf-1", "kind": "principled", "at": [2, 0],
          "params": { "baseColor": [0.82, 0.84, 0.88], "metallic": 1.0, "roughness": 0.3 } },
        { "id": "out-1", "kind": "output", "at": [3, 0], "params": {} }
      ],
      "links": [
        { "from": "noise-1:fac", "to": "ramp-1:fac" },
        { "from": "ramp-1:color", "to": "bsdf-1:roughness" },
        { "from": "bsdf-1:bsdf", "to": "out-1:surface" }
      ]
    }
  ]
}
"#;

fn shader(text: &str) -> proton_scene::Shader {
    Scene::parse(text)
        .unwrap()
        .materials()
        .into_iter()
        .next()
        .expect("a material")
        .compile()
}

#[test]
fn a_graph_compiles_to_instructions_in_dependency_order() {
    // A single pass has to be enough to run it, so every register an
    // instruction reads must have been written by an earlier one. A compiler
    // that emitted a node before what feeds it would produce a program that
    // reads uninitialised memory and looks almost right.
    let out = shader(SCENE);
    assert_eq!(out.problem, "");
    for (at, op) in out.ops.iter().enumerate() {
        let reads: Vec<usize> = match op {
            Op::Constant { .. } | Op::Position => vec![],
            Op::Noise { at, .. } | Op::Checker { at, .. } => vec![*at],
            Op::Mix { a, b, fac } => vec![*a, *b, *fac],
        };
        for read in reads {
            assert!(read < at, "instruction {at} reads register {read}");
        }
    }
    assert!(out.base < out.ops.len());
    assert!(out.metallic < out.ops.len());
    assert!(out.roughness < out.ops.len());
}

#[test]
fn a_wire_becomes_a_register_and_a_setting_becomes_a_constant() {
    let out = shader(SCENE);
    // Roughness is wired, so it points at the mix the ramp became.
    assert!(
        matches!(out.ops[out.roughness], Op::Mix { .. }),
        "{:?}",
        out.ops[out.roughness]
    );
    // Base colour and metallic are not wired, so they are what the file says.
    assert_eq!(
        out.ops[out.base],
        Op::Constant {
            value: [0.82, 0.84, 0.88]
        }
    );
    assert_eq!(out.ops[out.metallic], Op::Constant { value: [1.0; 3] });
}

#[test]
fn a_noise_with_nothing_wired_into_it_samples_the_surface() {
    // Which is what a texture is for. Something wired in is how one noise
    // warps another, and that is the unusual case rather than the default.
    let out = shader(SCENE);
    let Op::Mix { fac, .. } = out.ops[out.roughness] else {
        unreachable!()
    };
    let Op::Noise { at, scale, detail } = out.ops[fac] else {
        panic!("the ramp's factor is not a noise: {:?}", out.ops[fac])
    };
    assert_eq!(scale, 8.0, "the file's own scale");
    assert_eq!(detail, 3.0);
    assert_eq!(out.ops[at], Op::Position);
}

#[test]
fn the_ramps_ends_come_from_its_own_settings() {
    let out = shader(SCENE);
    let Op::Mix { a, b, .. } = out.ops[out.roughness] else {
        unreachable!()
    };
    assert_eq!(out.ops[a], Op::Constant { value: [0.1; 3] });
    assert_eq!(out.ops[b], Op::Constant { value: [0.9; 3] });
}

#[test]
fn a_node_feeding_two_sockets_is_compiled_once() {
    // Otherwise a graph that reuses one noise four times runs it four times
    // per triangle for the same answer.
    let twice = SCENE.replace(
        r#"{ "from": "ramp-1:color", "to": "bsdf-1:roughness" }"#,
        r#"{ "from": "ramp-1:color", "to": "bsdf-1:roughness" },
        { "from": "ramp-1:color", "to": "bsdf-1:baseColor" }"#,
    );
    let out = shader(&twice);
    assert_eq!(out.problem, "");
    assert_eq!(out.base, out.roughness, "the same register, not a copy");
    assert_eq!(
        out.ops
            .iter()
            .filter(|op| matches!(op, Op::Noise { .. }))
            .count(),
        1
    );
}

#[test]
fn no_two_instructions_are_the_same_instruction() {
    // A graph where several sockets fall back to the same default should not
    // emit that constant several times, and two noises with identical settings
    // are one noise. Asserted over the whole program rather than by counting,
    // because what matters is that nothing is duplicated — not how many there
    // happen to be.
    let bare = SCENE.replace(
        r#""baseColor": [0.82, 0.84, 0.88], "metallic": 1.0, "roughness": 0.3"#,
        "",
    );
    let out = shader(&bare);
    assert_eq!(out.problem, "");
    for (at, op) in out.ops.iter().enumerate() {
        assert!(
            !out.ops[..at].contains(op),
            "instruction {at} is already in the program: {op:?}"
        );
    }
}

#[test]
fn a_node_wired_to_nothing_costs_nothing() {
    // The graph is read backwards from what it produces. A spare noise left
    // beside a work-in-progress graph should not appear in the program, and
    // should certainly not stop it compiling.
    let spare = SCENE.replace(
        r#"{ "id": "out-1", "kind": "output", "at": [3, 0], "params": {} }"#,
        r#"{ "id": "out-1", "kind": "output", "at": [3, 0], "params": {} },
        { "id": "spare-1", "kind": "checker", "at": [0, 2], "params": { "scale": 3.0 } }"#,
    );
    let out = shader(&spare);
    assert_eq!(out.problem, "");
    assert!(!out.ops.iter().any(|op| matches!(op, Op::Checker { .. })));
}

#[test]
fn a_kind_this_build_cannot_render_is_refused_by_name() {
    // Guessing at a node nobody here understands would put a picture on screen
    // that the file does not describe.
    let odd = SCENE.replace(r#""kind": "noise""#, r#""kind": "musgrave""#);
    let out = shader(&odd);
    assert!(out.ops.is_empty(), "it compiled something anyway");
    assert!(out.problem.contains("musgrave"), "{}", out.problem);
}

#[test]
fn a_graph_with_no_output_says_so() {
    let headless = SCENE.replace(r#""kind": "output""#, r#""kind": "spare""#);
    let out = shader(&headless);
    assert!(out.problem.contains("no output"), "{}", out.problem);
}

#[test]
fn an_output_wired_to_nothing_says_so() {
    let loose = SCENE.replace(
        r#",
        { "from": "bsdf-1:bsdf", "to": "out-1:surface" }"#,
        "",
    );
    let out = shader(&loose);
    assert!(out.problem.contains("nothing is wired"), "{}", out.problem);
}

#[test]
fn an_output_fed_by_something_that_is_not_a_surface_says_so() {
    // A noise is a pattern, not a material. Rendering it as one would be a
    // guess about what somebody meant by wiring it there.
    let wrong = SCENE.replace(r#""from": "bsdf-1:bsdf""#, r#""from": "noise-1:fac""#);
    let out = shader(&wrong);
    assert!(out.problem.contains("not a surface"), "{}", out.problem);
    assert!(out.problem.contains("noise"), "{}", out.problem);
}

#[test]
fn a_material_with_no_graph_at_all_compiles_to_nothing_and_says_why() {
    let empty = r#"{"format":"proton.scene","version":1,"objects":[],
        "materials":[{"id":"m","name":"M","nodes":[],"links":[]}]}"#;
    let out = shader(empty);
    assert!(out.ops.is_empty());
    assert!(!out.problem.is_empty());
}

#[test]
fn a_mix_takes_its_ends_from_wires_when_they_are_wired() {
    let mixed = r#"{
      "format": "proton.scene", "version": 1, "objects": [],
      "materials": [{
        "id": "m", "name": "M",
        "nodes": [
          { "id": "red", "kind": "rgb", "at": [0,0], "params": { "color": [1, 0, 0] } },
          { "id": "blue", "kind": "rgb", "at": [0,1], "params": { "color": [0, 0, 1] } },
          { "id": "check", "kind": "checker", "at": [0,2], "params": { "scale": 6 } },
          { "id": "mix", "kind": "mix", "at": [1,0], "params": {} },
          { "id": "bsdf", "kind": "principled", "at": [2,0], "params": {} },
          { "id": "out", "kind": "output", "at": [3,0], "params": {} }
        ],
        "links": [
          { "from": "red:color", "to": "mix:a" },
          { "from": "blue:color", "to": "mix:b" },
          { "from": "check:fac", "to": "mix:fac" },
          { "from": "mix:color", "to": "bsdf:baseColor" },
          { "from": "bsdf:bsdf", "to": "out:surface" }
        ]
      }]
    }"#;
    let out = shader(mixed);
    assert_eq!(out.problem, "");
    let Op::Mix { a, b, fac } = out.ops[out.base] else {
        panic!("{:?}", out.ops[out.base])
    };
    assert_eq!(
        out.ops[a],
        Op::Constant {
            value: [1.0, 0.0, 0.0]
        }
    );
    assert_eq!(
        out.ops[b],
        Op::Constant {
            value: [0.0, 0.0, 1.0]
        }
    );
    assert!(matches!(out.ops[fac], Op::Checker { scale: 6.0, .. }));
}
