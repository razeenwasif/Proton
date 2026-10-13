//! A paint bucket: the pixels like the one clicked, as a mask.
//!
//! Like is the same colour within a tolerance — the most any channel may
//! differ — and every clear pixel is like every other, whatever colour it
//! keeps under its nothing. Contiguous, it spreads from the pixel clicked to
//! its neighbours (four, or eight with the diagonals); not, it takes every
//! pixel like it on the layer. Which pixels a fill takes does not depend on
//! the order they are visited in, so both builds find the same ones.

use crate::stroke::Mask;
use crate::tile::Grid;

/// The pixels of `grid` like the one at `x`, `y`.
pub fn flood(grid: &Grid, x: i32, y: i32, tolerance: u8, contiguous: bool, diagonal: bool) -> Mask {
    let mut mask = Mask::new(grid.width, grid.height);
    if x < 0 || y < 0 || x as u32 >= grid.width || y as u32 >= grid.height {
        return mask;
    }
    let target = grid.pixel(x as u32, y as u32);
    let like = |p: [u8; 4]| {
        if target[3] == 0 && p[3] == 0 {
            return true;
        }
        (0..4).all(|c| p[c].abs_diff(target[c]) <= tolerance)
    };
    let (w, h) = (grid.width as i32, grid.height as i32);
    if !contiguous {
        for py in 0..h {
            for px in 0..w {
                if like(grid.pixel(px as u32, py as u32)) {
                    mask.stamp(px, py, true);
                }
            }
        }
        return mask;
    }
    let mut seen = vec![false; (grid.width * grid.height) as usize];
    let mut stack = vec![(x, y)];
    seen[(y * w + x) as usize] = true;
    let steps: &[(i32, i32)] = if diagonal { &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] } else { &[(1, 0), (-1, 0), (0, 1), (0, -1)] };
    while let Some((px, py)) = stack.pop() {
        if !like(grid.pixel(px as u32, py as u32)) {
            continue;
        }
        mask.stamp(px, py, true);
        for &(dx, dy) in steps {
            let (nx, ny) = (px + dx, py + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let at = (ny * w + nx) as usize;
            if !seen[at] {
                seen[at] = true;
                stack.push((nx, ny));
            }
        }
    }
    mask
}
