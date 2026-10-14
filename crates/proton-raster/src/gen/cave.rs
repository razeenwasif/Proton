//! Caves: a cellular automaton. Cells start as rock by chance, then each
//! becomes rock if five or more of the nine about it are — a few rounds of
//! that turn noise into caverns. Past the edge is rock, or, for a tile that
//! repeats, the far edge.

use super::rng::Rng;

/// Rock (true) and open cells, `cols` × `rows`, row by row.
pub fn cells(seed: u32, density: u8, steps: u8, cols: u32, rows: u32, wrap: bool) -> Vec<bool> {
    let (w, h) = (cols as i64, rows as i64);
    let mut rng = Rng::new(seed);
    let mut grid: Vec<bool> = (0..w * h).map(|_| rng.below(100) < density.min(100) as u32).collect();
    for _ in 0..steps.min(16) {
        let rock = |g: &Vec<bool>, x: i64, y: i64| {
            if wrap {
                g[(y.rem_euclid(h) * w + x.rem_euclid(w)) as usize]
            } else if x < 0 || y < 0 || x >= w || y >= h {
                true
            } else {
                g[(y * w + x) as usize]
            }
        };
        grid = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let around = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).filter(|&(dx, dy)| rock(&grid, x + dx, y + dy)).count();
                around >= 5
            })
            .collect();
    }
    grid
}
