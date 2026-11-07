//! K2: the triangulation faces are made of — random polygons with holes,
//! grids that sit on each other's circles, points added inside.

mod common;

use common::Rng;
use proton_cad::mesh::cdt::{self, Cdt};
use proton_cad::Vec2;

/// A star-shaped loop about `c`: `n` points at random radii between `lo`
/// and `hi`, anticlockwise.
fn star(rng: &mut Rng, c: Vec2, n: usize, lo: f64, hi: f64) -> Vec<Vec2> {
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * (k as f64 + rng.range(0.0, 0.8)) / n as f64;
            let r = rng.range(lo, hi);
            Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

fn area(p: &[Vec2]) -> f64 {
    (0..p.len()).map(|i| p[i].cross(p[(i + 1) % p.len()])).sum::<f64>() / 2.0
}

fn tri_area(pts: &[Vec2], t: [usize; 3]) -> f64 {
    (pts[t[1]] - pts[t[0]]).cross(pts[t[2]] - pts[t[0]]) / 2.0
}

/// Checks a triangulation of `pts` against its loops: anticlockwise, the
/// area right, every loop's segment an edge.
fn sound(c: &Cdt, pts: &[Vec2], loops: &[Vec<usize>], want: f64) {
    let tris = c.triangles();
    let mut total = 0.0;
    for &t in &tris {
        let a = tri_area(pts, t);
        assert!(a > 0.0, "a triangle the wrong way: {a}");
        total += a;
    }
    assert!((total - want).abs() <= 1e-9 * want, "area {total} against {want}");
    for l in loops {
        for k in 0..l.len() {
            assert!(c.is_kept(l[k], l[(k + 1) % l.len()]), "a loop's segment is not an edge");
        }
    }
}

#[test]
fn random_polygons_with_holes_triangulate_exactly() {
    let mut rng = Rng(7);
    for case in 0..300 {
        // Twelve points or more, so the outer loop clears the holes' ring.
        let holes = (rng.next_u64() % 5) as usize;
        let n = if holes == 0 { 3 } else { 12 } + (rng.next_u64() % 60) as usize;
        let outer = star(&mut rng, Vec2::new(0.0, 0.0), n, 50.0, 100.0);
        let mut pts = outer.clone();
        let mut loops = vec![(0..n).collect::<Vec<_>>()];
        let mut want = area(&outer);
        // Holes on a ring well inside the outer's least radius, apart.
        for h in 0..holes {
            let a = std::f64::consts::TAU * h as f64 / holes.max(1) as f64;
            let c = Vec2::new(28.0 * a.cos(), 28.0 * a.sin());
            let m = 3 + (rng.next_u64() % 20) as usize;
            let mut ring = star(&mut rng, c, m, 3.0, 9.0);
            ring.reverse();
            want += area(&ring);
            let base = pts.len();
            pts.extend(ring);
            loops.push((base..base + m).collect());
        }
        let c = cdt::polygon(&pts, &loops).unwrap_or_else(|e| panic!("case {case}: {e}"));
        sound(&c, &pts, &loops, want);
        // Without points inside, a polygon of n points and h holes has
        // n + 2h − 2 triangles.
        assert_eq!(c.triangles().len(), pts.len() + 2 * holes - 2, "case {case}");
    }
}

#[test]
fn points_added_inside_keep_it_whole() {
    let mut rng = Rng(8);
    for _ in 0..40 {
        let outer = star(&mut rng, Vec2::new(0.0, 0.0), 40, 50.0, 100.0);
        let mut pts = outer.clone();
        let loops = vec![(0..40).collect::<Vec<_>>()];
        let mut c = cdt::polygon(&pts, &loops).unwrap();
        let mut added = 0;
        for _ in 0..400 {
            let p = Vec2::new(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
            if let Some(i) = c.insert_inside(p, 0.5) {
                assert_eq!(i, pts.len());
                pts.push(p);
                added += 1;
            }
        }
        assert!(added > 50);
        sound(&c, &pts, &loops, area(&outer));
    }
}

#[test]
fn a_grid_on_its_own_circles_triangulates_and_stays_delaunay() {
    // A square of 21 × 21 points: every four of a cell on one circle.
    let mut pts = Vec::new();
    let n = 20;
    let mut boundary = Vec::new();
    for i in 0..=n {
        boundary.push(Vec2::new(i as f64, 0.0));
    }
    for j in 1..=n {
        boundary.push(Vec2::new(n as f64, j as f64));
    }
    for i in (0..n).rev() {
        boundary.push(Vec2::new(i as f64, n as f64));
    }
    for j in (1..n).rev() {
        boundary.push(Vec2::new(0.0, j as f64));
    }
    pts.extend(boundary.iter().copied());
    let loops = vec![(0..boundary.len()).collect::<Vec<_>>()];
    let mut c = cdt::polygon(&pts, &loops).unwrap();
    for i in 1..n {
        for j in 1..n {
            let p = Vec2::new(i as f64, j as f64);
            assert_eq!(c.insert_inside(p, 0.1), Some(pts.len()));
            pts.push(p);
        }
    }
    sound(&c, &pts, &loops, (n * n) as f64);
    assert_eq!(c.triangles().len(), 2 * n * n);
    // No point strictly inside any triangle's circumcircle.
    let tris = c.triangles();
    for t in &tris {
        let [a, b, d] = t.map(|i| pts[i]);
        let (ax, ay, bx, by, dx, dy) = (a.x, a.y, b.x, b.y, d.x, d.y);
        let den = 2.0 * (ax * (by - dy) + bx * (dy - ay) + dx * (ay - by));
        let ux = ((ax * ax + ay * ay) * (by - dy) + (bx * bx + by * by) * (dy - ay) + (dx * dx + dy * dy) * (ay - by)) / den;
        let uy = ((ax * ax + ay * ay) * (dx - bx) + (bx * bx + by * by) * (ax - dx) + (dx * dx + dy * dy) * (bx - ax)) / den;
        let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();
        for p in &pts {
            let d = ((p.x - ux).powi(2) + (p.y - uy).powi(2)).sqrt();
            assert!(d >= r - 1e-9, "a point inside a circumcircle");
        }
    }
}

#[test]
fn a_thin_comb_triangulates() {
    // Teeth a hundredth as wide as they are long, the kind a slotted
    // plate's face has.
    let mut pts = vec![Vec2::new(0.0, 0.0)];
    for k in 0..50 {
        let x = k as f64 * 2.0;
        pts.push(Vec2::new(x + 1.0, 0.0));
        pts.push(Vec2::new(x + 1.0, 100.0));
        pts.push(Vec2::new(x + 1.02, 100.0));
        pts.push(Vec2::new(x + 1.02, 0.0));
    }
    pts.push(Vec2::new(102.0, 0.0));
    pts.push(Vec2::new(102.0, -1.0));
    pts.push(Vec2::new(0.0, -1.0));
    let loops = vec![(0..pts.len()).collect::<Vec<_>>()];
    let c = cdt::polygon(&pts, &loops).unwrap();
    sound(&c, &pts, &loops, area(&pts).abs());
}
