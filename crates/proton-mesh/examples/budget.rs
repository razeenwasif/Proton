//! What the pipe carries: parse time and wire size, per mesh.
//!
//! Run against a folder of OBJs to get the numbers that set the budget in
//! `proton-core::modeling`. Kept as an example rather than a test because it
//! measures a machine, and a machine is not a thing to assert about.
//!
//! ```text
//! cargo run --release -p proton-mesh --example budget -- <folder>
//! ```

use std::time::Instant;

use proton_mesh::Mesh;

fn main() {
    let folder = std::env::args().nth(1).expect("a folder of .obj files");
    let mut found: Vec<_> = std::fs::read_dir(&folder)
        .expect("that folder")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "obj"))
        .collect();
    found.sort_by_key(|path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));

    println!(
        "{:<10} {:>9} {:>9} {:>9} {:>9} {:>10} {:>10}",
        "file", "verts", "tris", "read ms", "shade ms", "on disk", "as JSON"
    );
    for path in found {
        let began = Instant::now();
        let mesh = match Mesh::read(&path) {
            Ok(mesh) => mesh,
            Err(why) => {
                println!("{}: {why}", path.display());
                continue;
            }
        };
        let read = began.elapsed();

        // What the front end would actually receive. Serialised rather than
        // estimated, because the estimate is the thing being checked.
        // Shaded first, as the core sends it: normals are three more floats a
        // vertex, and splitting at creases makes more vertices.
        let began = Instant::now();
        let shaded = mesh.shade(proton_mesh::CREASE);
        let shade = began.elapsed();
        let wire = serde_json::to_string(&(&shaded.positions, &shaded.normals, &shaded.triangles))
            .map(|text| text.len())
            .unwrap_or(0);

        println!(
            "{:<10} {:>9} {:>9} {:>9.1} {:>9.1} {:>9.1}K {:>9.1}K",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            mesh.vertices(),
            mesh.count(),
            read.as_secs_f64() * 1000.0,
            shade.as_secs_f64() * 1000.0,
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) as f64 / 1024.0,
            wire as f64 / 1024.0,
        );
    }
}
