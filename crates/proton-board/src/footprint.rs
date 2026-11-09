//! Footprints: the copper a part is soldered to, its courtyard, its
//! outline in silk and on the assembly drawing.
//!
//! Surface-mount land patterns are worked out by IPC-7351B's equations
//! from the part's dimensions and their tolerances, at its nominal density
//! (level B):
//!
//! ```text
//! Z = Lmin + 2·Jt + √(CL² + F² + P²)        outside, across the lands
//! G = Smax − 2·Jh − √(CS² + F² + P²)        inside, between them
//! X = Wmin + 2·Js + √(CW² + F² + P²)        each land's width
//! ```
//!
//! `C` a tolerance, `S` the gap between the terminals (`L − 2T`, its
//! tolerance the root of the squares of L's and both T's), `F` the board's
//! fabrication tolerance (0.10 mm) and `P` the placement's (0.05 mm), as
//! KiCad's generator takes them. The fillet goals `J` are the standard's
//! level B: chips 0603 and larger toe 0.35, heel 0, side 0, and gull-wing
//! leads toe 0.35, heel 0.35, side 0.03, as published second hand (the
//! standard itself is sold; read 2026-11-09). Chips smaller than 0603 (toe
//! 0.20, heel 0, side 0) and no-lead parts (QFN; toe 0.40, heel 0, side
//! −0.04) are from memory and not checked: their footprints carry a
//! caution the rules check repeats. Z and X are rounded up to 0.05 mm, G
//! down; the courtyard is 0.25 mm clear of everything.
//!
//! Through-hole parts are a grid of holes: headers by their pitch, and the
//! PC's connectors by their families' usual layouts, which a board made for
//! real is checked against the connector's own drawing.

use crate::symbol::Symbol;

/// The board's fabrication tolerance, and the placement's, mm.
pub const F: f64 = 0.10;
pub const P: f64 = 0.05;
/// How far the courtyard stands clear.
pub const COURTYARD: f64 = 0.25;

/// A dimension and its tolerance: least and most.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dim {
    pub min: f64,
    pub max: f64,
}

impl Dim {
    pub const fn pm(nominal: f64, tol: f64) -> Dim {
        Dim { min: nominal - tol, max: nominal + tol }
    }
    pub fn tol(self) -> f64 {
        self.max - self.min
    }
}

/// The solder fillets wanted: toe, heel, side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goals {
    pub toe: f64,
    pub heel: f64,
    pub side: f64,
}

pub const CHIP: Goals = Goals { toe: 0.35, heel: 0.0, side: 0.0 };
pub const CHIP_SMALL: Goals = Goals { toe: 0.20, heel: 0.0, side: 0.0 };
pub const GULL_WING: Goals = Goals { toe: 0.35, heel: 0.35, side: 0.03 };
pub const NO_LEAD: Goals = Goals { toe: 0.40, heel: 0.0, side: -0.04 };

/// Rounded up, or down, to 0.05 mm.
fn up(x: f64) -> f64 {
    (x / 0.05 - 1e-9).ceil() * 0.05
}
fn down(x: f64) -> f64 {
    (x / 0.05 + 1e-9).floor() * 0.05
}

/// IPC-7351B's lands for two opposite rows of terminals: `l` across the
/// terminals' outsides, `t` each terminal's length, `w` its width. Gives
/// Z, G and X, rounded.
pub fn lands(l: Dim, t: Dim, w: Dim, j: Goals) -> (f64, f64, f64) {
    let s = Dim { min: l.min - 2.0 * t.max, max: l.max - 2.0 * t.min };
    // S's tolerance as the root of the squares of its parts', and its most
    // taken that much above its least, as IPC-7351B does.
    let s_tol = (l.tol().powi(2) + 2.0 * t.tol().powi(2)).sqrt();
    let s_max = s.max - (s.tol() - s_tol) / 2.0;
    let rms = |c: f64| (c * c + F * F + P * P).sqrt();
    let z = l.min + 2.0 * j.toe + rms(l.tol());
    let g = s_max - 2.0 * j.heel - rms(s_tol);
    let x = w.min + 2.0 * j.side + rms(w.tol());
    (up(z), down(g.max(0.0)), up(x))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    /// Rounded corners, a quarter of the shorter side.
    RoundRect,
    Oval,
}

/// A pad: its number, where it is (mm, y up, the part's middle at the
/// origin), its size, and a hole if it goes through.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub number: String,
    pub at: [f64; 2],
    pub size: [f64; 2],
    pub shape: Shape,
    pub drill: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
    pub name: String,
    pub pads: Vec<Pad>,
    /// The part's body, for the assembly drawing and the silk.
    pub body: [f64; 2],
    /// Its courtyard: what nothing else may come into.
    pub courtyard: [f64; 4],
    pub smd: bool,
    pub source: String,
    pub caution: Option<String>,
}

fn finish(name: String, pads: Vec<Pad>, body: [f64; 2], smd: bool, source: &str, caution: Option<&str>) -> Footprint {
    let (mut lo, mut hi) = ([-body[0] / 2.0, -body[1] / 2.0], [body[0] / 2.0, body[1] / 2.0]);
    for p in &pads {
        lo = [lo[0].min(p.at[0] - p.size[0] / 2.0), lo[1].min(p.at[1] - p.size[1] / 2.0)];
        hi = [hi[0].max(p.at[0] + p.size[0] / 2.0), hi[1].max(p.at[1] + p.size[1] / 2.0)];
    }
    let r = |x: f64, up_: bool| if up_ { (x / 0.01).ceil() * 0.01 } else { (x / 0.01).floor() * 0.01 };
    let courtyard = [r(lo[0] - COURTYARD, false), r(lo[1] - COURTYARD, false), r(hi[0] + COURTYARD, true), r(hi[1] + COURTYARD, true)];
    Footprint { name, pads, body, courtyard, smd, source: source.into(), caution: caution.map(str::to_string) }
}

/// A two-terminal chip by its EIA size: typical thick-film resistor
/// dimensions, length, width and terminal, with their tolerances.
fn chip_dims(code: &str) -> Option<(Dim, Dim, Dim)> {
    Some(match code {
        "0201" => (Dim::pm(0.60, 0.03), Dim::pm(0.30, 0.03), Dim::pm(0.15, 0.05)),
        "0402" => (Dim::pm(1.00, 0.05), Dim::pm(0.50, 0.05), Dim::pm(0.25, 0.10)),
        "0603" => (Dim::pm(1.60, 0.10), Dim::pm(0.80, 0.10), Dim::pm(0.30, 0.20)),
        "0805" => (Dim::pm(2.00, 0.10), Dim::pm(1.25, 0.10), Dim::pm(0.40, 0.20)),
        "1206" => (Dim::pm(3.10, 0.10), Dim::pm(1.60, 0.10), Dim::pm(0.45, 0.20)),
        "1210" => (Dim::pm(3.10, 0.10), Dim::pm(2.60, 0.15), Dim::pm(0.50, 0.25)),
        "2010" => (Dim::pm(5.00, 0.10), Dim::pm(2.50, 0.15), Dim::pm(0.60, 0.25)),
        "2512" => (Dim::pm(6.35, 0.10), Dim::pm(3.20, 0.15), Dim::pm(0.60, 0.25)),
        _ => return None,
    })
}

fn chip(code: &str) -> Option<Footprint> {
    let (l, w, t) = chip_dims(code)?;
    let small = l.max < 1.5;
    let (z, g, x) = lands(l, t, w, if small { CHIP_SMALL } else { CHIP });
    let (len, mid) = ((z - g) / 2.0, (z + g) / 4.0);
    let pads = (0..2).map(|i| Pad { number: (i + 1).to_string(), at: [if i == 0 { -mid } else { mid }, 0.0], size: [len, x], shape: Shape::RoundRect, drill: None }).collect();
    let caution = small.then_some("a chip smaller than 0603 takes fillet goals remembered, not read from IPC-7351B: check its lands against the part's datasheet");
    Some(finish(code.to_string(), pads, [(l.min + l.max) / 2.0, (w.min + w.max) / 2.0], true, "IPC-7351B level B, from typical chip dimensions", caution))
}

/// A gull-wing package's two rows: `n` leads a side at `pitch`, the lead
/// span `span` (toe to toe), each lead `t` long and `b` wide; numbered
/// down the left, up the right.
#[allow(clippy::too_many_arguments)]
fn gull_rows(name: String, n: usize, pitch: f64, span: Dim, t: Dim, b: Dim, body: [f64; 2], caution: Option<&str>) -> Footprint {
    let (z, g, x) = lands(span, t, b, GULL_WING);
    let (len, mid) = ((z - g) / 2.0, (z + g) / 4.0);
    let mut pads = Vec::new();
    let top = (n as f64 - 1.0) * pitch / 2.0;
    for i in 0..n {
        pads.push(Pad { number: (i + 1).to_string(), at: [-mid, top - i as f64 * pitch], size: [len, x], shape: Shape::RoundRect, drill: None });
    }
    for i in 0..n {
        pads.push(Pad { number: (n + i + 1).to_string(), at: [mid, -top + i as f64 * pitch], size: [len, x], shape: Shape::RoundRect, drill: None });
    }
    finish(name, pads, body, true, "IPC-7351B level B gull-wing, from JEDEC's package outline", caution)
}

/// Four sides of leads or lands, `n` a side, numbered anticlockwise from
/// the top left: QFP and QFN.
#[allow(clippy::too_many_arguments)]
fn quad(name: String, n: usize, pitch: f64, span: Dim, t: Dim, b: Dim, body: f64, j: Goals, pad: Option<f64>, caution: Option<&str>) -> Footprint {
    let (z, g, x) = lands(span, t, b, j);
    let (len, mid) = ((z - g) / 2.0, (z + g) / 4.0);
    let mut pads = Vec::new();
    let top = (n as f64 - 1.0) * pitch / 2.0;
    let mut k = 1;
    // Left side, top to bottom; bottom, left to right; right, bottom to
    // top; top, right to left.
    for side in 0..4 {
        for i in 0..n {
            let along = -top + i as f64 * pitch;
            let (at, size) = match side {
                0 => ([-mid, -along], [len, x]),
                1 => ([along, -mid], [x, len]),
                2 => ([mid, along], [len, x]),
                _ => ([-along, mid], [x, len]),
            };
            pads.push(Pad { number: k.to_string(), at, size, shape: Shape::RoundRect, drill: None });
            k += 1;
        }
    }
    if let Some(e) = pad {
        pads.push(Pad { number: k.to_string(), at: [0.0, 0.0], size: [e, e], shape: Shape::Rect, drill: None });
    }
    finish(name, pads, [body, body], true, "IPC-7351B level B, from JEDEC's package outline", caution)
}

/// A grid of through holes: `rows` of `n` at `pitch`, rows `row` apart;
/// numbered across the rows (1, 2 in the first column), the first square;
/// `absent` left out — a key.
#[allow(clippy::too_many_arguments)]
fn holes(name: String, rows: usize, n: usize, pitch: f64, row: f64, drill: f64, pad: f64, absent: &[usize], body: [f64; 2], source: &str, caution: Option<&str>) -> Footprint {
    let mut pads = Vec::new();
    let left = -(n as f64 - 1.0) * pitch / 2.0;
    let top = (rows as f64 - 1.0) * row / 2.0;
    for i in 0..n {
        for r in 0..rows {
            let number = i * rows + r + 1;
            if absent.contains(&number) {
                continue;
            }
            pads.push(Pad { number: number.to_string(), at: [left + i as f64 * pitch, top - r as f64 * row], size: [pad, pad], shape: if number == 1 { Shape::Rect } else { Shape::Round }, drill: Some(drill) });
        }
    }
    finish(name, pads, body, false, source, caution)
}

const CONNECTOR: &str = "the connector family's usual layout, not yet checked against the maker's drawing: check the holes against your connector's datasheet before a board is made";

/// A footprint by its name: `0603`; `SOT-23`, `SOT-23-5`, `SOT-23-6`;
/// `SOIC-8`…`SOIC-16`; `TSSOP-8`…`TSSOP-28`; `LQFP-32`, `-44`, `-48`,
/// `-64`, `-100`; `QFN-16`, `-20`, `-24`, `-32`, `-48`; `header-1x4` or
/// `header-2x5`, at 2.54 unless `-2.0`; `mini-fit-jr-2x12`; `kk-254-4`.
pub fn footprint(name: &str) -> Result<Footprint, String> {
    let lower = name.to_ascii_lowercase();
    if let Some(f) = chip(&lower) {
        return Ok(f);
    }
    let num = |s: &str| s.parse::<usize>().map_err(|_| format!("`{name}` is not a footprint"));
    match lower.as_str() {
        "sot-23" | "sot-23-3" => {
            // JEDEC TO-236: span 2.10–2.64, leads 0.3–0.6 long, 0.3–0.5 wide.
            let mut f = gull_rows("SOT-23".into(), 1, 0.95, Dim { min: 2.10, max: 2.64 }, Dim { min: 0.30, max: 0.60 }, Dim { min: 0.30, max: 0.50 }, [1.30, 2.92], None);
            // Three leads: 1 and 2 at the left 1.90 apart, 3 at the right.
            let (left, right) = (f.pads[0].clone(), f.pads[1].clone());
            f.pads = vec![
                Pad { number: "1".into(), at: [left.at[0], 0.95], ..left.clone() },
                Pad { number: "2".into(), at: [left.at[0], -0.95], ..left },
                Pad { number: "3".into(), at: [right.at[0], 0.0], ..right },
            ];
            return Ok(finish("SOT-23".into(), f.pads, f.body, true, "IPC-7351B level B gull-wing, from JEDEC TO-236", None));
        }
        "sot-23-5" | "sot-23-6" => {
            let six = lower.ends_with('6');
            let f = gull_rows(name.to_ascii_uppercase(), 3, 0.95, Dim { min: 2.60, max: 3.00 }, Dim { min: 0.30, max: 0.60 }, Dim { min: 0.30, max: 0.50 }, [1.60, 2.90], None);
            // Five: the right's middle left out, numbered 1–3 down the
            // left, 4–5 up the right.
            let mut pads = f.pads;
            if !six {
                pads.remove(4);
                pads[3].number = "4".into();
                pads[4].number = "5".into();
            }
            return Ok(finish(name.to_ascii_uppercase(), pads, f.body, true, "IPC-7351B level B gull-wing, from JEDEC MO-178", None));
        }
        _ => {}
    }
    if let Some(n) = lower.strip_prefix("soic-") {
        let n = num(n)?;
        if !(8..=16).contains(&n) || n % 2 == 1 {
            return Err(format!("`{name}`: an SOIC here has 8 to 16 leads, an even number"));
        }
        // JEDEC MS-012: span 5.80–6.20, leads 0.40–1.27 long, 0.31–0.51 wide.
        let len = (n / 2) as f64 * 1.27 - 0.2;
        return Ok(gull_rows(format!("SOIC-{n}"), n / 2, 1.27, Dim { min: 5.80, max: 6.20 }, Dim { min: 0.40, max: 1.27 }, Dim { min: 0.31, max: 0.51 }, [3.90, len], None));
    }
    if let Some(n) = lower.strip_prefix("tssop-") {
        let n = num(n)?;
        if !(8..=28).contains(&n) || n % 2 == 1 {
            return Err(format!("`{name}`: a TSSOP here has 8 to 28 leads, an even number"));
        }
        // JEDEC MO-153: span 6.20–6.60, leads 0.45–0.75 long, 0.19–0.30 wide.
        let len = (n / 2) as f64 * 0.65 + 0.4;
        return Ok(gull_rows(format!("TSSOP-{n}"), n / 2, 0.65, Dim { min: 6.20, max: 6.60 }, Dim { min: 0.45, max: 0.75 }, Dim { min: 0.19, max: 0.30 }, [4.40, len], None));
    }
    if let Some(n) = lower.strip_prefix("lqfp-") {
        let n = num(n)?;
        // JEDEC MS-026: body, pitch, span and lead width by count.
        let (body, pitch, span, b) = match n {
            32 => (7.0, 0.8, 9.0, Dim { min: 0.30, max: 0.45 }),
            44 => (10.0, 0.8, 12.0, Dim { min: 0.30, max: 0.45 }),
            48 => (7.0, 0.5, 9.0, Dim { min: 0.17, max: 0.27 }),
            64 => (10.0, 0.5, 12.0, Dim { min: 0.17, max: 0.27 }),
            100 => (14.0, 0.5, 16.0, Dim { min: 0.17, max: 0.27 }),
            _ => return Err(format!("`{name}`: an LQFP here is 32, 44, 48, 64 or 100")),
        };
        return Ok(quad(format!("LQFP-{n}"), n / 4, pitch, Dim::pm(span, 0.2), Dim { min: 0.45, max: 0.75 }, b, body, GULL_WING, None, None));
    }
    if let Some(n) = lower.strip_prefix("qfn-") {
        let n = num(n)?;
        // JEDEC MO-220: body, pitch, exposed pad by count.
        let (body, pitch, pad) = match n {
            16 => (3.0, 0.5, 1.7),
            20 => (4.0, 0.5, 2.6),
            24 => (4.0, 0.5, 2.6),
            32 => (5.0, 0.5, 3.45),
            48 => (7.0, 0.5, 5.15),
            _ => return Err(format!("`{name}`: a QFN here is 16, 20, 24, 32 or 48")),
        };
        return Ok(quad(
            format!("QFN-{n}"),
            n / 4,
            pitch,
            Dim::pm(body, 0.1),
            Dim { min: 0.30, max: 0.50 },
            Dim { min: 0.18, max: 0.30 },
            body,
            NO_LEAD,
            Some(pad),
            Some("the no-lead fillet goals are the standard's as remembered, not read from it; and an exposed pad's size is the package's maker's to give: check both against the datasheet"),
        ));
    }
    if let Some(rest) = lower.strip_prefix("header-") {
        let (grid, pitch) = match rest.split_once('-') {
            Some((g, p)) => (g, p.parse::<f64>().map_err(|_| format!("`{name}`: a header's pitch is 2.54, 2.0 or 1.27"))?),
            None => (rest, 2.54),
        };
        let (r, n) = grid.split_once('x').ok_or_else(|| format!("`{name}`: a header is `header-1x4` or `header-2x5`"))?;
        let (r, n) = (num(r)?, num(n)?);
        if !(1..=2).contains(&r) || n == 0 || n > 40 || ![2.54, 2.0, 1.27].contains(&pitch) {
            return Err(format!("`{name}`: a header is one or two rows of up to 40, at 2.54, 2.0 or 1.27"));
        }
        let (drill, pad) = match pitch {
            p if p > 2.5 => (1.0, 1.7),
            p if p > 1.9 => (0.8, 1.35),
            _ => (0.65, 1.0),
        };
        return Ok(holes(format!("header-{r}x{n}-{pitch}"), r, n, pitch, pitch, drill, pad, &[], [n as f64 * pitch, r as f64 * pitch], "a pin header's usual holes", None));
    }
    if let Some(rest) = lower.strip_prefix("mini-fit-jr-2x") {
        let n = num(rest)?;
        if !(1..=12).contains(&n) {
            return Err(format!("`{name}`: a Mini-Fit Jr. here is 2x1 to 2x12"));
        }
        return Ok(holes(format!("mini-fit-jr-2x{n}"), 2, n, 4.2, 4.2, 1.4, 2.1, &[], [n as f64 * 4.2 + 1.4, 9.6], "Mini-Fit Jr.'s 4.2 mm pitch", Some(CONNECTOR)));
    }
    if let Some(rest) = lower.strip_prefix("kk-254-") {
        let n = num(rest)?;
        if !(2..=12).contains(&n) {
            return Err(format!("`{name}`: a KK 254 here is 2 to 12 pins"));
        }
        return Ok(holes(format!("kk-254-{n}"), 1, n, 2.54, 2.54, 1.0, 1.7, &[], [n as f64 * 2.54 + 0.5, 5.8], "KK 254's 2.54 mm pitch", Some(CONNECTOR)));
    }
    Err(format!("`{name}` is not a footprint here: a chip (0201–2512), SOT-23(-5, -6), SOIC-N, TSSOP-N, LQFP-N, QFN-N, header-RxN(-pitch), mini-fit-jr-2xN or kk-254-N"))
}

/// The footprint a library part has unless its line says another: the
/// PC's connectors their families'.
pub fn default_for(sym: &Symbol) -> Option<&'static str> {
    Some(match sym.kind.as_str() {
        "atx-24" => "mini-fit-jr-2x12",
        "eps-8" | "pcie-8" => "mini-fit-jr-2x4",
        "pcie-6" => "mini-fit-jr-2x3",
        "fan-4pin" => "kk-254-4",
        "argb-5v" => "header-1x4",
        "usb2-header" => "header-2x5",
        "usb3-header" => "header-2x10-2.0",
        _ => return None,
    })
}

/// Pads a footprint has that a part's pins do not, and pins with no pad.
pub fn mismatch(sym: &Symbol, f: &Footprint) -> (Vec<String>, Vec<String>) {
    let has_pad = |n: &str| f.pads.iter().any(|p| p.number == n);
    // A key, or a pin not to be connected, needs no pad.
    let no_pad = sym.pins.iter().filter(|p| p.kind != crate::symbol::PinKind::Nc && !has_pad(&p.number)).map(|p| p.number.clone()).collect();
    let mut no_pin: Vec<String> = Vec::new();
    for p in &f.pads {
        if !sym.pins.iter().any(|x| x.number == p.number) && !no_pin.contains(&p.number) {
            no_pin.push(p.number.clone());
        }
    }
    (no_pad, no_pin)
}

/// A footprint drawn on its own small sheet, millimetres: its pads, holes,
/// numbers, body and courtyard — for the window to show a part's.
pub fn draw(f: &Footprint) -> proton_cad::drawing::Sheet {
    use proton_cad::drawing::{Anchor, Pen, Prim, Sheet, Style, P2};
    let [x0, y0, x1, y1] = f.courtyard;
    let m = 1.0;
    let (w, h) = (x1 - x0 + 2.0 * m, y1 - y0 + 2.0 * m);
    let at = |x: f64, y: f64| P2::new(x - x0 + m, y - y0 + m);
    let mut prims = Vec::new();
    let rect = |cx: f64, cy: f64, sx: f64, sy: f64| vec![at(cx - sx / 2.0, cy - sy / 2.0), at(cx + sx / 2.0, cy - sy / 2.0), at(cx + sx / 2.0, cy + sy / 2.0), at(cx - sx / 2.0, cy + sy / 2.0)];
    prims.push(Prim::Poly { pts: rect((x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0), closed: true, pen: Pen::Thin });
    prims.push(Prim::Poly { pts: rect(0.0, 0.0, f.body[0], f.body[1]), closed: true, pen: Pen::Hidden });
    for p in &f.pads {
        let pts = match p.shape {
            Shape::Round | Shape::Oval => (0..24).map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 24.0;
                at(p.at[0] + a.cos() * p.size[0] / 2.0, p.at[1] + a.sin() * p.size[1] / 2.0)
            }).collect(),
            _ => rect(p.at[0], p.at[1], p.size[0], p.size[1]),
        };
        prims.push(Prim::Fill { pts, pen: Pen::Dim });
        if let Some(d) = p.drill {
            prims.push(Prim::Circle { c: at(p.at[0], p.at[1]), r: d / 2.0, pen: Pen::Visible });
        }
        let size = (p.size[0].min(p.size[1]) * 0.5).clamp(0.25, 1.0);
        prims.push(Prim::Text { at: at(p.at[0], p.at[1] - size * 0.35), size, text: p.number.clone(), anchor: Anchor::Middle, angle: 0.0, pen: Pen::Frame });
    }
    Sheet { width: w, height: h, style: Style::Paper, prims, boxes: Vec::new() }
}

/// The footprint as KiCad's `.kicad_mod` (KiCad 6 and later): its pads on
/// the front copper, its courtyard, silk and fabrication outline. KiCad's
/// y runs down.
pub fn kicad_mod(f: &Footprint) -> String {
    let n = |x: f64| {
        let s = format!("{:.4}", x);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" { "0".into() } else { s }
    };
    let mut o = format!("(footprint \"{}\" (version 20221018) (generator \"proton\")\n  (layer \"F.Cu\")\n  (attr {})\n", f.name, if f.smd { "smd" } else { "through_hole" });
    let [x0, y0, x1, y1] = f.courtyard;
    o.push_str(&format!("  (fp_text reference \"REF**\" (at 0 {}) (layer \"F.SilkS\") (effects (font (size 1 1) (thickness 0.15))))\n", n(-(y1 + 1.0))));
    o.push_str(&format!("  (fp_text value \"{}\" (at 0 {}) (layer \"F.Fab\") (effects (font (size 1 1) (thickness 0.15))))\n", f.name, n(-(y0 - 1.0))));
    let (bw, bh) = (f.body[0] / 2.0, f.body[1] / 2.0);
    o.push_str(&format!("  (fp_rect (start {} {}) (end {} {}) (stroke (width 0.1) (type solid)) (fill none) (layer \"F.Fab\"))\n", n(-bw), n(-bh), n(bw), n(bh)));
    o.push_str(&format!("  (fp_rect (start {} {}) (end {} {}) (stroke (width 0.05) (type solid)) (fill none) (layer \"F.CrtYd\"))\n", n(x0), n(-y1), n(x1), n(-y0)));
    for p in &f.pads {
        let shape = match p.shape {
            Shape::Rect => "rect",
            Shape::Round => "circle",
            Shape::RoundRect => "roundrect",
            Shape::Oval => "oval",
        };
        let (kind, layers) = if p.drill.is_some() { ("thru_hole", "\"*.Cu\" \"*.Mask\"") } else { ("smd", "\"F.Cu\" \"F.Paste\" \"F.Mask\"") };
        o.push_str(&format!("  (pad \"{}\" {kind} {shape} (at {} {}) (size {} {})", p.number, n(p.at[0]), n(-p.at[1]), n(p.size[0]), n(p.size[1])));
        if let Some(d) = p.drill {
            o.push_str(&format!(" (drill {})", n(d)));
        }
        o.push_str(&format!(" (layers {layers})"));
        if p.shape == Shape::RoundRect {
            o.push_str(" (roundrect_rratio 0.25)");
        }
        o.push_str(")\n");
    }
    o.push_str(")\n");
    o
}
