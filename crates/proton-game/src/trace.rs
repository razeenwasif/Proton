//! Collision from art: the outline of where an image is opaque, as a few
//! straight lines.
//!
//! Hand-drawn ground has no grid, and drawing its collision point by point
//! along a painted ledge is the tedious part of building a room. So the
//! image's alpha is thresholded, walked with marching squares into closed
//! outlines — holes and all — and each outline is simplified with
//! Douglas–Peucker until it is the handful of points a designer would have
//! placed. What comes out is ordinary collision: closed solid shapes that can
//! be edited, re-kinded or deleted like any drawn by hand.

/// An image's opacity, one byte a pixel, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

impl Mask {
    /// Whether the pixel at `x`, `y` counts as solid; outside the image is not.
    fn solid(&self, x: isize, y: isize, threshold: u8) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return false;
        }
        self.alpha[y as usize * self.width + x as usize] >= threshold
    }

    /// A PNG's opacity: its alpha channel, or opaque where it has none.
    #[cfg(feature = "editor")]
    pub fn of_png(bytes: &[u8]) -> Result<Mask, String> {
        let mut decoder = png::Decoder::new(bytes);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
        let (width, height) = (info.width as usize, info.height as usize);
        let channels = info.color_type.samples();
        let alpha = match info.color_type {
            png::ColorType::Rgba | png::ColorType::GrayscaleAlpha => {
                (0..width * height).map(|i| buffer[i * channels + channels - 1]).collect()
            }
            _ => vec![255; width * height],
        };
        Ok(Mask { width, height, alpha })
    }
}

/// How an outline is found and how much of its detail is kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracing {
    /// The alpha at which a pixel counts as solid.
    pub threshold: u8,
    /// How far a simplified outline may stray from the traced one, in pixels.
    pub tolerance: f64,
    /// Outlines enclosing less than this many pixels are dropped: specks.
    pub smallest: f64,
}

impl Default for Tracing {
    fn default() -> Tracing {
        Tracing { threshold: 128, tolerance: 2.0, smallest: 64.0 }
    }
}

/// The closed outlines of where `mask` is solid, in image pixels, simplified.
/// An outer edge runs one way round and a hole the other.
pub fn outlines(mask: &Mask, how: &Tracing) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for ring in rings(mask, how.threshold) {
        if area(&ring).abs() < how.smallest {
            continue;
        }
        let simple = simplify_closed(&ring, how.tolerance);
        if simple.len() >= 3 {
            out.push(simple);
        }
    }
    out
}

/// The signed area a ring encloses (shoelace).
pub fn area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n).map(|i| {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        a[0] * b[1] - b[0] * a[1]
    })
    .sum::<f64>()
        / 2.0
}

/// Every boundary between solid and clear, as closed rings through the
/// midpoints of pixel edges — marching squares over the pixel corners, with
/// the image's surround counted clear so every ring closes.
fn rings(mask: &Mask, threshold: u8) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    // Each boundary is a directed step between two edge midpoints, solid on
    // its left, keyed by where it starts — doubled coordinates keep them whole.
    let mut next: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    let (w, h) = (mask.width as isize, mask.height as isize);
    for y in -1..h {
        for x in -1..w {
            let tl = mask.solid(x, y, threshold);
            let tr = mask.solid(x + 1, y, threshold);
            let br = mask.solid(x + 1, y + 1, threshold);
            let bl = mask.solid(x, y + 1, threshold);
            // Midpoints of the cell's sides, doubled: the cell spans the
            // centres of four pixels.
            let (cx, cy) = (2 * x as i64 + 1, 2 * y as i64 + 1);
            let top = (cx + 1, cy);
            let right = (cx + 2, cy + 1);
            let bottom = (cx + 1, cy + 2);
            let left = (cx, cy + 1);
            let case = (tl as u8) << 3 | (tr as u8) << 2 | (br as u8) << 1 | bl as u8;
            let steps: &[((i64, i64), (i64, i64))] = match case {
                0 | 15 => &[],
                1 => &[(left, bottom)],
                2 => &[(bottom, right)],
                3 => &[(left, right)],
                4 => &[(right, top)],
                // A saddle is two corners, not one diagonal: solid touching
                // only at a corner is not joined.
                5 => &[(left, bottom), (right, top)],
                6 => &[(bottom, top)],
                7 => &[(left, top)],
                8 => &[(top, left)],
                9 => &[(top, bottom)],
                10 => &[(top, left), (bottom, right)],
                11 => &[(top, right)],
                12 => &[(right, left)],
                13 => &[(right, bottom)],
                14 => &[(bottom, left)],
                _ => &[],
            };
            for (from, to) in steps {
                next.entry(*from).or_default().push(*to);
            }
        }
    }
    let mut out = Vec::new();
    let mut starts: Vec<(i64, i64)> = next.keys().copied().collect();
    starts.sort();
    for start in starts {
        while let Some(first) = next.get_mut(&start).and_then(Vec::pop) {
            let mut ring = vec![start];
            let mut at = first;
            while at != start {
                ring.push(at);
                match next.get_mut(&at).and_then(Vec::pop) {
                    Some(step) => at = step,
                    None => break,
                }
            }
            if ring.len() >= 3 {
                // Halved: a doubled 2x + 1 is pixel x's centre, x + 0.5.
                out.push(ring.iter().map(|&(x, y)| [x as f64 / 2.0, y as f64 / 2.0]).collect());
            }
        }
    }
    out
}

/// Douglas–Peucker on a closed ring: split at the two points farthest apart,
/// simplify each half as an open line, join.
fn simplify_closed(ring: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if ring.len() <= 3 {
        return ring.to_vec();
    }
    let first = 0;
    let far = (1..ring.len())
        .max_by(|&a, &b| distance(ring[first], ring[a]).total_cmp(&distance(ring[first], ring[b])))
        .unwrap_or(1);
    let mut one: Vec<[f64; 2]> = ring[first..=far].to_vec();
    let mut two: Vec<[f64; 2]> = ring[far..].to_vec();
    two.push(ring[first]);
    one = simplify_open(&one, tolerance);
    two = simplify_open(&two, tolerance);
    one.pop();
    two.pop();
    one.extend(two);
    one
}

fn simplify_open(line: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if line.len() <= 2 {
        return line.to_vec();
    }
    let (a, b) = (line[0], line[line.len() - 1]);
    let (at, most) = (1..line.len() - 1)
        .map(|i| (i, off_line(line[i], a, b)))
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap_or((0, 0.0));
    if most <= tolerance {
        return vec![a, b];
    }
    let mut left = simplify_open(&line[..=at], tolerance);
    let right = simplify_open(&line[at..], tolerance);
    left.pop();
    left.extend(right);
    left
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// How far `p` is from the line through `a` and `b`.
fn off_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let length = distance(a, b);
    if length == 0.0 {
        return distance(p, a);
    }
    ((b[0] - a[0]) * (a[1] - p[1]) - (a[0] - p[0]) * (b[1] - a[1])).abs() / length
}
