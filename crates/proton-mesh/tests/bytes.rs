//! What the binary layout has to get right.
//!
//! Round trips, first: a mesh encoded and decoded is the same mesh. Then the
//! layout itself, byte by byte, because the other end of this is JavaScript
//! laying typed arrays over offsets it was told about — and a misplaced
//! section there does not fail, it draws garbage shaped like a mesh.

use proton_mesh::{Mesh, Step, CREASE, HEADER, MAGIC, VERSION};

const CUBE: &str = "v -0.5 -0.5 -0.5\nv -0.5 -0.5 0.5\nv -0.5 0.5 -0.5\nv -0.5 0.5 0.5\n\
                    v 0.5 -0.5 -0.5\nv 0.5 -0.5 0.5\nv 0.5 0.5 -0.5\nv 0.5 0.5 0.5\n\
                    f 1 2 4 3\nf 5 7 8 6\nf 1 3 7 5\nf 2 6 8 4\nf 3 4 8 7\nf 1 5 6 2\n";

fn cube() -> Mesh {
    Mesh::parse(CUBE).unwrap()
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn float(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn a_mesh_comes_back_as_itself() {
    for mesh in [
        cube(),
        cube().shade(CREASE),
        cube().apply(&[Step::Subdivide { levels: 2 }]).shade(CREASE),
    ] {
        assert_eq!(Mesh::from_bytes(&mesh.to_bytes()).unwrap(), mesh);
    }
}

#[test]
fn a_mesh_without_normals_says_so_and_carries_none() {
    // A mesh nobody shaded, which the page must then shade from the surface.
    // Sending a block of zeros instead would be read as normals pointing
    // nowhere, and every pixel of it would light as black.
    let bare = cube();
    let bytes = bare.to_bytes();
    assert_eq!(word(&bytes, 16) & 2, 0, "the normals flag is set");
    assert_eq!(
        bytes.len(),
        HEADER + (bare.positions.len() + bare.triangles.len()) * 4
    );
    assert!(Mesh::from_bytes(&bytes).unwrap().normals.is_empty());
}

#[test]
fn the_header_is_where_the_page_reads_it() {
    // Asserted against fixed offsets, because the page reads fixed offsets.
    let mesh = cube().shade(CREASE);
    let bytes = mesh.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC);
    assert_eq!(word(&bytes, 4), VERSION);
    assert_eq!(word(&bytes, 8), 24, "vertices");
    assert_eq!(word(&bytes, 12), 12, "triangles");
    assert_eq!(word(&bytes, 16), 1 | 2, "closed and with normals");
    assert_eq!(word(&bytes, 20), 0, "nothing skipped");
    for (k, want) in [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5].into_iter().enumerate() {
        assert_eq!(float(&bytes, 24 + k * 4), want, "bound {k}");
    }
}

#[test]
fn the_sections_follow_the_header_in_order() {
    // Positions, then normals, then triangles — each exactly where the counts
    // in the header put it.
    let mesh = cube().shade(CREASE);
    let bytes = mesh.to_bytes();
    let v = mesh.vertices();
    let t = mesh.count();

    let normals_at = HEADER + v * 3 * 4;
    let triangles_at = normals_at + v * 3 * 4;
    assert_eq!(bytes.len(), triangles_at + t * 3 * 4);

    assert_eq!(float(&bytes, HEADER), mesh.positions[0]);
    assert_eq!(
        float(&bytes, normals_at - 4),
        *mesh.positions.last().unwrap()
    );
    assert_eq!(float(&bytes, normals_at), mesh.normals[0]);
    assert_eq!(word(&bytes, triangles_at), mesh.triangles[0]);
    assert_eq!(
        word(&bytes, bytes.len() - 4),
        *mesh.triangles.last().unwrap()
    );
}

#[test]
fn every_section_starts_on_a_four_byte_boundary() {
    // A `Float32Array` over an `ArrayBuffer` throws if its offset is not a
    // multiple of four. The header is twelve words so nothing needs padding.
    assert_eq!(HEADER % 4, 0);
    let mesh = cube().shade(CREASE);
    let v = mesh.vertices();
    for at in [HEADER, HEADER + v * 12, HEADER + v * 24] {
        assert_eq!(at % 4, 0);
    }
}

#[test]
fn it_is_smaller_than_the_json_it_replaces() {
    // Smaller, but not by as much as this test first claimed. It asserted
    // "under half" and got 18,528 bytes against 34,853: 1.9×, because a
    // subdivided cube's coordinates are short decimals like 0.3125 that JSON
    // writes compactly. On a real sphere with long decimals it measured 2.06×
    // — 6,423 KB against 13,257 KB.
    //
    // Size was never the main win. Writing that JSON took 25 ms in Rust and
    // parsing it back into typed arrays took another 25 ms in the page; the
    // bytes took 1.5 ms to write and nothing measurable to read, because the
    // page lays typed arrays straight over the buffer. That is measured in
    // `examples/budget.rs` and in the page, not asserted here: it depends on
    // the machine.
    let mesh = cube().apply(&[Step::Subdivide { levels: 3 }]).shade(CREASE);
    let json = serde_json::to_string(&mesh).unwrap().len();
    let bytes = mesh.to_bytes().len();
    assert!(bytes < json, "{bytes} bytes against {json} of JSON");
}

#[test]
fn bytes_that_are_not_a_mesh_are_refused_in_words() {
    let good = cube().shade(CREASE).to_bytes();

    let said = Mesh::from_bytes(&good[..20]).unwrap_err();
    assert!(said.contains("too short"), "{said}");

    let mut wrong = good.clone();
    wrong[0] = b'X';
    assert!(Mesh::from_bytes(&wrong).unwrap_err().contains("not a mesh"));

    // A page and a core from different builds. Reading on regardless would be
    // garbage shaped exactly like the right answer.
    let mut later = good.clone();
    later[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
    let said = Mesh::from_bytes(&later).unwrap_err();
    assert!(said.contains("version"), "{said}");

    let said = Mesh::from_bytes(&good[..good.len() - 4]).unwrap_err();
    assert!(said.contains("bytes"), "{said}");
}

#[test]
fn a_triangle_naming_a_vertex_that_is_not_there_is_refused() {
    // Handed to a GPU as an index, that is a read past the end of a buffer.
    let mut bytes = cube().to_bytes();
    let last = bytes.len() - 4;
    bytes[last..].copy_from_slice(&999u32.to_le_bytes());
    let said = Mesh::from_bytes(&bytes).unwrap_err();
    assert!(said.contains("999"), "{said}");
}
