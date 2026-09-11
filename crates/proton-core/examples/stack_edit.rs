//! What one edit to a modifier stack costs, end to end through the workspace.
//!
//! Copies an OBJ into a scratch scene, gives its object a one-copy array — a
//! stack that runs without growing the mesh — and then nudges the offset,
//! timing each `Workspace::apply` from command to geometry event. That is the
//! wait a person dragging a number field feels, less the trip to the page.
//! Kept as an example rather than a test because it measures a machine.
//!
//! ```text
//! cargo run --release -p proton-core --example stack_edit -- <mesh.obj>
//! ```

use std::time::Instant;

use proton_core::bus::Event;
use proton_core::{Command, ModeId, Workspace};

const SCENE: &str = r#"{
  "format": "proton.scene",
  "version": 1,
  "objects": [
    { "id": "it", "name": "It", "kind": "mesh", "mesh": "meshes/it.obj",
      "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] }
  ]
}
"#;

fn main() {
    let obj = std::env::args().nth(1).expect("an .obj file");
    let root = std::env::temp_dir().join(format!("proton-stack-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::copy(&obj, root.join("meshes").join("it.obj")).expect("that file");
    std::fs::write(root.join("scene.proton.json"), SCENE).unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode {
        mode: ModeId::Modeling,
    });
    let ms = |began: Instant| began.elapsed().as_secs_f64() * 1000.0;
    let triangles = |events: &[Event]| {
        events.iter().find_map(|event| match event {
            Event::Geometry(what) => Some(what.triangles),
            _ => None,
        })
    };

    let began = Instant::now();
    let events = space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "array".into(),
    });
    println!("add an array:      {:>7.1} ms", ms(began));
    let events_had = triangles(&events);
    space.apply(Command::SetModifierField {
        index: 0,
        row: 0,
        field: "count".into(),
        value: "1".into(),
    });

    let mut each = Vec::new();
    for step in 1..=8 {
        let began = Instant::now();
        let events = space.apply(Command::SetModifierField {
            index: 0,
            row: 0,
            field: "offsetX".into(),
            value: format!("{}", step as f32 * 0.25),
        });
        each.push(ms(began));
        assert!(triangles(&events).is_some(), "no geometry came back");
    }
    each.sort_by(f64::total_cmp);
    println!(
        "nudge the offset:  {:>7.1} ms median, {:.1}–{:.1} over {} edits ({} triangles)",
        each[each.len() / 2],
        each[0],
        each[each.len() - 1],
        each.len(),
        triangles(&space.apply(Command::LoadMesh {
            path: "meshes/it.obj@it".into()
        }))
        .or(events_had)
        .unwrap_or(0),
    );
    let _ = std::fs::remove_dir_all(&root);
}
