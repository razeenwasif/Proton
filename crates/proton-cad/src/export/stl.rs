//! STL: a mesh as bare triangles, each with its normal. There are no
//! units in the format; millimetres are what every slicer assumes.

use crate::mesh::Mesh;
use crate::vec::Vec3;

fn normal(m: &Mesh, t: &[u32; 3]) -> Vec3 {
    let [a, b, c] = t.map(|i| m.positions[i as usize]);
    (b - a).cross(c - a).unit_or(Vec3::Z)
}

/// Binary STL: an 80-byte header, the count, and fifty bytes a triangle.
pub fn binary(m: &Mesh, name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + 50 * m.triangles.len());
    let mut head = format!("Proton CAD: {name}").into_bytes();
    // A binary file's header must not start with "solid", or readers
    // take it for text.
    head.truncate(80);
    head.resize(80, b' ');
    out.extend_from_slice(&head);
    out.extend_from_slice(&(m.triangles.len() as u32).to_le_bytes());
    let put = |v: Vec3, out: &mut Vec<u8>| {
        for x in [v.x, v.y, v.z] {
            out.extend_from_slice(&(x as f32).to_le_bytes());
        }
    };
    for t in &m.triangles {
        put(normal(m, t), &mut out);
        for &i in t {
            put(m.positions[i as usize], &mut out);
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Text STL.
pub fn text(m: &Mesh, name: &str) -> String {
    let name: String = name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    let mut out = format!("solid {name}\n");
    let v = |p: Vec3| format!("{} {} {}", super::mm(p.x), super::mm(p.y), super::mm(p.z));
    for t in &m.triangles {
        out.push_str(&format!("  facet normal {}\n    outer loop\n", v(normal(m, t))));
        for &i in t {
            out.push_str(&format!("      vertex {}\n", v(m.positions[i as usize])));
        }
        out.push_str("    endloop\n  endfacet\n");
    }
    out.push_str(&format!("endsolid {name}\n"));
    out
}

/// A binary STL's triangles, read back: what a test checks a file by.
pub fn read_binary(bytes: &[u8]) -> Result<Vec<[Vec3; 3]>, String> {
    if bytes.len() < 84 {
        return Err("too short for an STL".into());
    }
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    if bytes.len() != 84 + 50 * n {
        return Err(format!("{n} triangles do not fill {} bytes", bytes.len()));
    }
    let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as f64;
    Ok((0..n)
        .map(|k| {
            let at = 84 + 50 * k + 12;
            [0, 1, 2].map(|j| Vec3::new(f(at + 12 * j), f(at + 12 * j + 4), f(at + 12 * j + 8)))
        })
        .collect())
}
