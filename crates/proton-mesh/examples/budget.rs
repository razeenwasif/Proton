//! What the pipe carries: parse time, shading time, and what one mesh costs to
//! send — as JSON and as bytes.
//!
//! Run against a folder of OBJs to get the numbers that set the budget in
//! `proton-core::modeling`. Kept as an example rather than a test because it
//! measures a machine, and a machine is not a thing to assert about.
//!
//! ```text
//! cargo run --release -p proton-mesh --example budget -- <folder>
//! ```

use std::time::Instant;

use proton_mesh::{Mesh, CREASE};

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
        "{:<8} {:>8} {:>8} {:>8} {:>10} {:>9} {:>10} {:>9}",
        "file", "tris", "read ms", "shade ms", "JSON", "JSON ms", "bytes", "bytes ms"
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

        // Shaded first, as the core sends it.
        let began = Instant::now();
        let shaded = mesh.shade(CREASE);
        let shade = began.elapsed();

        // Both encodings of what the front end receives. Serialised rather
        // than estimated, because the estimate is the thing being checked.
        let began = Instant::now();
        let json = serde_json::to_string(&shaded).unwrap().len();
        let json_ms = began.elapsed();
        let began = Instant::now();
        let bytes = shaded.to_bytes().len();
        let bytes_ms = began.elapsed();

        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        println!(
            "{:<8} {:>8} {:>8.1} {:>8.1} {:>9.0}K {:>9.1} {:>9.0}K {:>9.1}",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            shaded.count(),
            ms(read),
            ms(shade),
            json as f64 / 1024.0,
            ms(json_ms),
            bytes as f64 / 1024.0,
            ms(bytes_ms),
        );
    }
}
