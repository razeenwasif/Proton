//! What reading somebody else's geometry has to get right.
//!
//! The fixture is a unit cube written the way an exporter writes one — with a
//! normal list this does not read, a material reference it does not follow, a
//! quad face, a comment, and a group name — because those are most of a real
//! OBJ and all of what a careless reader trips over.

use proton_mesh::Mesh;

const CUBE: &str = r#"# Blender v4.0 OBJ File: 'cube.blend'
mtllib cube.mtl
o Cube
v -0.5 -0.5 -0.5
v -0.5 -0.5  0.5
v -0.5  0.5 -0.5
v -0.5  0.5  0.5
v  0.5 -0.5 -0.5
v  0.5 -0.5  0.5
v  0.5  0.5 -0.5
v  0.5  0.5  0.5
vn -1.0000 0.0000 0.0000
vn 0.0000 0.0000 -1.0000
vt 0.625000 0.500000
usemtl Material
s off
f 1//1 2//1 4//1 3//1
f 5//2 7//2 8//2 6//2
f 1/1/2 3/1/2 7/1/2 5/1/2
f 2 6 8 4
f 3 4 8 7
f 1 5 6 2
"#;

fn cube() -> Mesh {
    Mesh::parse(CUBE).unwrap()
}

#[test]
fn a_cube_is_eight_vertices_and_twelve_triangles() {
    // Six quads, fanned. A reader that took only the first three corners of
    // each face would give six triangles and half a cube.
    let cube = cube();
    assert_eq!(cube.vertices(), 8);
    assert_eq!(cube.count(), 12);
    assert_eq!(cube.skipped, 0);
}

#[test]
fn indices_count_from_one() {
    // The mistake every reader of this format makes once. `f 1 ...` is the
    // *first* vertex, and reading it as an offset shifts the whole mesh.
    let cube = cube();
    let first = &cube.triangles[..3];
    assert_eq!(first, [0, 1, 3], "{:?}", &cube.triangles[..6]);
    // Which is to say the first corner is the first `v` line.
    assert_eq!(&cube.positions[..3], [-0.5, -0.5, -0.5]);
}

#[test]
fn a_corner_may_carry_a_texture_coordinate_and_a_normal() {
    // `1`, `1//1` and `1/1/2` are the same vertex. A reader that parsed the
    // whole token as a number would fail on two of the three.
    let plain = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let dressed = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/4/7 2/5/8 3/6/9\n").unwrap();
    let normals_only = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1//7 2//8 3//9\n").unwrap();
    assert_eq!(plain.triangles, [0, 1, 2]);
    assert_eq!(dressed.triangles, plain.triangles);
    assert_eq!(normals_only.triangles, plain.triangles);
}

#[test]
fn a_negative_index_counts_back_from_the_vertices_so_far() {
    // How a generator that emits one shape at a time writes a file without
    // keeping a running total. `-1` is the vertex on the line above.
    let mesh = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n").unwrap();
    assert_eq!(mesh.triangles, [0, 1, 2]);

    // And it is relative to *that point in the file*, not to the end of it:
    // the second triangle's `-3` is the fourth vertex, not the first.
    let two = Mesh::parse(
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\nv 5 0 0\nv 6 0 0\nv 5 1 0\nf -3 -2 -1\n",
    )
    .unwrap();
    assert_eq!(two.triangles, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_polygon_is_fanned_from_its_first_corner() {
    // A quad `1 2 3 4` is the triangles `1 2 3` and `1 3 4` — sharing the
    // first corner and the diagonal, so the two halves meet exactly.
    let quad = Mesh::parse("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n").unwrap();
    assert_eq!(quad.triangles, [0, 1, 2, 0, 2, 3]);

    // A hexagon is four triangles, not two and a hole.
    let hex = Mesh::parse("v 0 0 0\nv 1 0 0\nv 2 1 0\nv 1 2 0\nv 0 2 0\nv -1 1 0\nf 1 2 3 4 5 6\n")
        .unwrap();
    assert_eq!(hex.count(), 4);
}

#[test]
fn everything_this_does_not_read_is_skipped_rather_than_refused() {
    // An OBJ full of things nothing here uses is still an OBJ. `vn`, `vt`,
    // `o`, `g`, `s`, `usemtl` and `mtllib` are all in the fixture.
    assert_eq!(cube().count(), 12);
}

#[test]
fn a_comment_ends_the_line_it_is_on() {
    let mesh = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0 # the top\nf 1 2 3 # a face\n").unwrap();
    assert_eq!(mesh.triangles, [0, 1, 2]);
    assert_eq!(&mesh.positions[6..9], [0.0, 1.0, 0.0]);
}

#[test]
fn a_face_naming_a_vertex_that_is_not_there_is_counted_not_drawn() {
    // A mesh missing part of its surface should say so rather than merely
    // looking wrong.
    let mesh =
        Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nf 1 2 9\nf 1 2\nf 1 0 2\n").unwrap();
    assert_eq!(mesh.count(), 1);
    assert_eq!(mesh.skipped, 3, "an out-of-range, a short face and a zero");
}

#[test]
fn a_vertex_that_is_not_three_numbers_still_takes_its_place_in_the_numbering() {
    // Dropping it would shift every index after it and take the mesh apart.
    // At the origin it is one wrong vertex instead of a wrong mesh.
    let mesh = Mesh::parse("v 0 0 0\nv oops\nv 1 0 0\nv 0 1 0\nf 1 3 4\n").unwrap();
    assert_eq!(mesh.vertices(), 4);
    assert_eq!(mesh.triangles, [0, 2, 3]);
}

#[test]
fn the_bounds_are_the_box_the_triangles_fill() {
    assert_eq!(cube().bounds, [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5]);
    assert_eq!(cube().size(), 1.0);
}

#[test]
fn a_vertex_no_face_uses_does_not_stretch_the_box() {
    // Exporters leave these behind. A stray point at the origin should not
    // make a mesh frame as if it were twice the size.
    let mesh = Mesh::parse("v 10 10 10\nv 11 10 10\nv 10 11 10\nv 0 0 0\nf 1 2 3\n").unwrap();
    assert_eq!(mesh.bounds, [10.0, 10.0, 10.0, 11.0, 11.0, 10.0]);
}

#[test]
fn a_file_that_is_not_geometry_is_refused_and_says_why() {
    let said = Mesh::parse("hello, world\n").unwrap_err();
    assert!(said.contains("no vertices"), "{said}");

    let said = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap_err();
    assert!(said.contains("no faces"), "{said}");

    // A file whose faces all point past the end of its vertices is a
    // different fault from one with no faces at all, and says so: the count
    // is what tells somebody the file is truncated rather than empty.
    let said = Mesh::parse("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 4 5 6\n").unwrap_err();
    assert!(said.contains("none of this file's 1 faces"), "{said}");
}

#[test]
fn a_file_a_windows_tool_wrote_reads_the_same() {
    assert_eq!(Mesh::parse(&CUBE.replace('\n', "\r\n")).unwrap(), cube());
}

#[test]
fn a_missing_file_says_so_in_words_somebody_can_act_on() {
    // The common failure by a mile: a scene naming a mesh nobody copied.
    let said = Mesh::read(std::path::Path::new("no/such/lamp.obj")).unwrap_err();
    assert!(said.contains("not there"), "{said}");
    assert!(said.contains("lamp.obj"), "{said}");
}

#[test]
fn a_solid_is_closed_and_a_sheet_is_not() {
    // What decides whether a renderer may throw away the faces pointing away
    // from the camera. A cube may; a single triangle may not, or it would
    // vanish when you orbited past it.
    assert!(cube().closed, "a cube is a solid");

    let sheet = Mesh::parse("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n").unwrap();
    assert!(!sheet.closed, "a quad is a sheet with four edges of rim");

    // And a cube with a face missing is a solid with a hole in it, which is
    // exactly the case that must not be culled.
    let holed = Mesh::parse(&CUBE.replace("f 1 5 6 2\n", "")).unwrap();
    assert!(!holed.closed);
    assert_eq!(holed.count(), 10);
}
