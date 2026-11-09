//! The FormD T1: the case as NCASE publishes it, and its published limits.
//!
//! Published (https://ncased.com/pages/specs-t1sw, T1 Sandwich Kit, read
//! 2026-11-09): 13.5 × 22 × 33.5 cm, 9.95 L; 1.5–15 mm aluminium, CNC
//! machined and anodised, and 1–2 mm powder-coated steel; Mini-ITX and
//! Deep-ITX; graphics cards no more than 140 × 325 mm and 3.25 slots; the
//! CPU cooler's height by the card's thickness (48 mm with 3.25 slots up to
//! 88 mm with 1.25 — 113 mm less 20 a slot); fans 1 or 2 × 120 top or
//! bottom, 2 × 120 or 2 × 140 on the side with the bracket kit; radiators 1 ×
//! 120, 1 × 240, 2 × 240 or 280 + 240 (the pairs with the bracket kit, and
//! only with a single-slot card); SFX, or SFX-L (not recommended with water
//! cooling). And from the revision history
//! (https://ncased.com/pages/revision-history-t1): V2.0 took a 240 radiator
//! and its fans up to 46 mm thick.
//!
//! Not published: where inside it the board, the card, the spine and the
//! power supply sit. The part is the case's shell and says so; those come
//! from the person's own measurements.

use super::{n, Made, Param, Text, Values};

pub fn params() -> Vec<Param> {
    vec![
        super::p("wall", "1.5", "the panels, mm: NCASE gives 1.5–15 mm aluminium and 1–2 mm steel — measure yours"),
        super::choice("version", "2.1", &["2.1", "2.5"], "V2.5 thickened its top and side struts"),
    ]
}

/// The case's outside, mm: width (across, the sandwich's way), depth (front
/// to back) and height.
pub const OUTSIDE: (f64, f64, f64) = (135.0, 335.0, 220.0);

pub fn make(v: &Values) -> Result<Made, String> {
    let wall = v.positive("wall")?;
    let (w, d, h) = OUTSIDE;
    if wall * 2.0 >= w {
        return Err(format!("walls of {} mm leave no inside", n(wall)));
    }
    let mut t = Text::new(&format!("FormD T1 V{}", v.text("version")), Some("aluminium-6061"), &[
        "FormD T1, from NCASE's published figures (ncased.com/pages/specs-t1sw, 2026-11-09): 13.5 × 22 × 33.5 cm, 9.95 L.",
        "x across (135, the sandwich's way), y front to back (335), z up (220); the origin at the bottom front left.",
        "Not published: the board's, card's, spine's and power supply's places inside — measure them and add them here.",
    ]);
    t.line(format!("param wall = {}  # measure yours: NCASE gives 1.5–15 mm aluminium, 1–2 mm steel", n(wall)));
    t.sketch("plan", "XY", &[format!("rect r corner (0, 0) size = ({}, {})", n(w), n(d))]);
    t.line(format!("extrude t1 from plan distance {}", n(h)));
    t.line("shell walls of t1 thickness wall");
    Ok(Made { stem: format!("formd-t1-v{}", v.text("version").replace('.', "-")), text: t.0 })
}

/* ── its limits ────────────────────────────────────────────────────── */

/// Where a radiator goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Top,
    Bottom,
    Side,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radiator {
    pub spot: Spot,
    /// 120, 240 or 280.
    pub size: u32,
    /// The radiator and its fans together, mm.
    pub thickness: f64,
}

/// What goes in the case, as far as its published limits care.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Build {
    /// A graphics card: length (as its maker gives it), height, slots.
    pub gpu: Option<(f64, f64, f64)>,
    /// The CPU cooler's height above the board — a water block's, for a loop.
    pub cooler: Option<f64>,
    pub radiators: Vec<Radiator>,
    /// `sfx` or `sfx-l`.
    pub psu: Option<String>,
    /// A loop or an all-in-one.
    pub water: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// It does not fit, by NCASE's figures; or, not, it is a caution.
    pub fits: bool,
    pub says: String,
}

/// The tallest CPU cooler NCASE gives for a card `slots` thick: 48 mm with
/// 3.25 slots, 5 mm more for each quarter slot less, to 88 mm with 1.25.
pub fn cooler_limit(slots: f64) -> f64 {
    113.0 - 20.0 * slots
}

/// The build against the T1's published limits: what does not fit, and
/// what NCASE cautions against.
pub fn check(b: &Build) -> Vec<Finding> {
    let mut out = Vec::new();
    let no = |s: String| Finding { fits: false, says: s };
    if let Some((len, height, slots)) = b.gpu {
        if len > 325.0 {
            out.push(no(format!("the card is {} mm long: the T1 takes 325 (NCASE does not say whether that counts the bracket)", n(len))));
        }
        if height > 140.0 {
            out.push(no(format!("the card is {} mm high: the T1 takes 140", n(height))));
        }
        if slots > 3.25 {
            out.push(no(format!("the card is {} slots thick: the T1 takes 3.25", n(slots))));
        }
        if let Some(c) = b.cooler {
            let max = cooler_limit(slots.max(1.25));
            if c > max {
                out.push(no(format!("the CPU cooler is {} mm high: with a {}-slot card the T1 takes {}", n(c), n(slots), n(max))));
            }
        }
        if b.radiators.len() >= 2 && slots > 1.0 {
            out.push(no(format!("two radiators need a single-slot card; this one is {} slots", n(slots))));
        }
    }
    let count = |s: u32| b.radiators.iter().filter(|r| r.size == s).count();
    match b.radiators.len() {
        0 | 1 => {}
        2 if count(240) == 2 || (count(280) == 1 && count(240) == 1) => {}
        _ => out.push(no("the T1 takes one 120 or 240 radiator, or with the bracket kit two 240s, or a 280 and a 240".into())),
    }
    for r in &b.radiators {
        if !matches!(r.size, 120 | 240 | 280) {
            out.push(no(format!("a {} radiator: the T1 takes 120, 240 and (on the side) 280", r.size)));
        }
        if r.size == 280 && r.spot != Spot::Side {
            out.push(no("a 280 radiator goes on the side, with the bracket kit".into()));
        }
        if r.size == 240 && r.spot != Spot::Side && r.thickness > 46.0 {
            out.push(no(format!("a 240 radiator and its fans are {} mm thick: the T1 takes 46", n(r.thickness))));
        }
    }
    if b.psu.as_deref() == Some("sfx-l") && (b.water || !b.radiators.is_empty()) {
        out.push(Finding { fits: true, says: "NCASE does not recommend an SFX-L power supply with an all-in-one or an open loop".into() });
    }
    out
}
