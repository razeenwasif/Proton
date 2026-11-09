//! A board written for a fab: Gerber X2 for each layer — copper, solder
//! mask, paste, silkscreen, the board's edge — Excellon for its drills,
//! plated and not, and a bill of materials and pick-and-place file for
//! its assembly; and readers of Proton's own for both formats, so what is
//! written is read back and checked.
//!
//! Millimetres, six decimals (`%FSLAX46Y46*%`). A zone is a region, then
//! each other net's copper near it cleared from it by its clearance (clear
//! polarity), then spokes across the gap to its own net's pads; every
//! track, via and pad is drawn over that. A rounded pad is written as its
//! rectangle, as the design rules check it. Solder mask opens 0.05 mm
//! round each pad and tents vias; paste is the surface-mount pads.

use std::fmt::Write as _;

use proton_cad::drawing::P2;

use crate::board::{core_distance, BoardBuilt, Copper, Core, Kind};
use crate::board_lang::LAYERS;

/// How much bigger than its pad the solder mask opens, each side.
pub const MASK: f64 = 0.05;

fn c(x: f64) -> i64 {
    (x * 1e6).round() as i64
}

fn mm(x: f64) -> String {
    let s = format!("{:.6}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A Gerber file being written: its apertures by shape, reused.
struct File {
    body: String,
    apertures: Vec<String>,
}

impl File {
    fn new() -> File {
        File { body: String::new(), apertures: Vec::new() }
    }

    /// The D-code of an aperture, made if new: `C,0.25` or `R,1.2X0.9`.
    fn aperture(&mut self, def: String) -> usize {
        let at = match self.apertures.iter().position(|a| *a == def) {
            Some(i) => i,
            None => {
                self.apertures.push(def);
                self.apertures.len() - 1
            }
        };
        at + 10
    }

    fn select(&mut self, def: String) {
        let d = self.aperture(def);
        let _ = writeln!(self.body, "D{d}*");
    }

    fn flash(&mut self, def: String, p: P2) {
        self.select(def);
        let _ = writeln!(self.body, "X{}Y{}D03*", c(p.x), c(p.y));
    }

    fn line(&mut self, w: f64, a: P2, b: P2) {
        self.select(format!("C,{}", mm(w.max(0.001))));
        let _ = writeln!(self.body, "X{}Y{}D02*", c(a.x), c(a.y));
        let _ = writeln!(self.body, "X{}Y{}D01*", c(b.x), c(b.y));
    }

    fn region(&mut self, ps: &[P2]) {
        let _ = writeln!(self.body, "G36*");
        let _ = writeln!(self.body, "X{}Y{}D02*", c(ps[0].x), c(ps[0].y));
        for p in &ps[1..] {
            let _ = writeln!(self.body, "X{}Y{}D01*", c(p.x), c(p.y));
        }
        let _ = writeln!(self.body, "X{}Y{}D01*", c(ps[0].x), c(ps[0].y));
        let _ = writeln!(self.body, "G37*");
    }

    fn polarity(&mut self, dark: bool) {
        let _ = writeln!(self.body, "%LP{}*%", if dark { "D" } else { "C" });
    }

    /// A piece of copper, grown by `grow` all round.
    fn copper(&mut self, item: &Copper, grow: f64) {
        match &item.core {
            Core::Pt(p) => self.flash(format!("C,{}", mm(2.0 * (item.radius + grow))), *p),
            Core::Seg(a, b) => self.line(2.0 * (item.radius + grow), *a, *b),
            // A zone is a region; a pad, as Gerber X2 asks, is flashed — a
            // rectangle, grown if asked.
            Core::Poly(ps) if item.kind == Kind::Zone => self.region(ps),
            Core::Poly(ps) => {
                let (x0, y0) = (ps.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), ps.iter().map(|p| p.y).fold(f64::INFINITY, f64::min));
                let (x1, y1) = (ps.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), ps.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max));
                self.flash(format!("R,{}X{}", mm(x1 - x0 + 2.0 * grow), mm(y1 - y0 + 2.0 * grow)), P2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0));
            }
        }
    }

    /// The file whole: its attributes, format, apertures and drawing.
    fn finish(&self, function: &str, polarity: &str) -> String {
        let mut o = String::new();
        let _ = writeln!(o, "%TF.GenerationSoftware,Proton,CAD,{}*%", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(o, "%TF.FileFunction,{function}*%");
        let _ = writeln!(o, "%TF.FilePolarity,{polarity}*%");
        let _ = writeln!(o, "%FSLAX46Y46*%");
        let _ = writeln!(o, "%MOMM*%");
        let _ = writeln!(o, "%LPD*%");
        for (i, a) in self.apertures.iter().enumerate() {
            let _ = writeln!(o, "%ADD{}{}*%", i + 10, a);
        }
        o.push_str(&self.body);
        o.push_str("M02*\n");
        o
    }
}

fn middle(c: &Core) -> P2 {
    match c {
        Core::Pt(p) => *p,
        Core::Seg(a, b) => P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        Core::Poly(ps) => P2::new(ps.iter().map(|q| q.x).sum::<f64>() / ps.len() as f64, ps.iter().map(|q| q.y).sum::<f64>() / ps.len() as f64),
    }
}

/// A copper layer: zones poured round other nets, then the copper.
fn copper_layer(b: &BoardBuilt, layer: usize) -> File {
    let mut f = File::new();
    let on: Vec<&Copper> = b.copper_items.iter().filter(|c| c.layers.contains(&layer)).collect();
    for z in on.iter().filter(|c| c.kind == Kind::Zone) {
        let (gap, spoke) = z.zone.unwrap_or((b.clearance, 0.5));
        f.copper(z, 0.0);
        let near: Vec<&&Copper> = on.iter().filter(|c| c.kind != Kind::Zone && core_distance(&c.core, &z.core) <= c.radius + gap).collect();
        f.polarity(false);
        for c in &near {
            f.copper(c, gap);
        }
        f.polarity(true);
        // Spokes across the gap to the zone's own pads.
        for c in near.iter().filter(|c| c.net == z.net && c.kind == Kind::Pad) {
            let m = middle(&c.core);
            let (bx0, by0, bx1, by1) = match &c.core {
                Core::Poly(ps) => (ps.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), ps.iter().map(|p| p.y).fold(f64::INFINITY, f64::min), ps.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), ps.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max)),
                _ => (m.x - c.radius, m.y - c.radius, m.x + c.radius, m.y + c.radius),
            };
            let reach = ((bx1 - bx0).max(by1 - by0)) / 2.0 + gap + 0.1;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                f.line(spoke, m, P2::new(m.x + dx * reach, m.y + dy * reach));
            }
        }
    }
    for c in on.iter().filter(|c| c.kind != Kind::Zone) {
        f.copper(c, 0.0);
    }
    f
}

/// A side's solder mask: each pad opened a little bigger; vias tented.
fn mask(b: &BoardBuilt, layer: usize) -> File {
    let mut f = File::new();
    for c in b.copper_items.iter().filter(|c| c.kind == Kind::Pad && c.layers.contains(&layer)) {
        f.copper(c, MASK);
    }
    f
}

/// A side's paste: its surface-mount pads.
fn paste(b: &BoardBuilt, layer: usize) -> File {
    let mut f = File::new();
    for c in b.copper_items.iter().filter(|c| c.kind == Kind::Pad && c.layers == vec![layer]) {
        f.copper(c, 0.0);
    }
    f
}

/// A side's silkscreen: each part's outline, with a dot by its first pad.
fn silk(b: &BoardBuilt, back: bool) -> File {
    let mut f = File::new();
    for p in b.parts.iter().filter(|p| p.back == back && p.placed) {
        for i in 0..p.body.len() {
            f.line(0.12, p.body[i], p.body[(i + 1) % p.body.len()]);
        }
        if let Some(first) = b.copper_items.iter().find(|c| c.kind == Kind::Pad && c.what == format!("{}.1", p.reference)) {
            let m = middle(&first.core);
            let centre = P2::new(p.at[0], p.at[1]);
            let d = P2::new(m.x - centre.x, m.y - centre.y);
            let l = d.x.hypot(d.y).max(1e-9);
            let reach = l + first.radius.max(0.6) + 0.6;
            f.flash("C,0.3".into(), P2::new(centre.x + d.x / l * reach, centre.y + d.y / l * reach));
        }
    }
    f
}

/// The board's edge, and any cut-outs.
fn profile(b: &BoardBuilt) -> File {
    let mut f = File::new();
    for poly in &b.outline {
        for i in 0..poly.len() {
            f.line(0.1, poly[i], poly[(i + 1) % poly.len()]);
        }
    }
    f
}

/// Excellon: each drill's size a tool, then its holes.
pub fn excellon(holes: &[(P2, f64)], plated: bool) -> String {
    let mut tools: Vec<f64> = Vec::new();
    for &(_, d) in holes {
        if !tools.iter().any(|t| (t - d).abs() < 1e-9) {
            tools.push(d);
        }
    }
    tools.sort_by(f64::total_cmp);
    let mut o = String::from("M48\n");
    let _ = writeln!(o, "; Proton CAD {} — {} holes", env!("CARGO_PKG_VERSION"), if plated { "plated" } else { "unplated" });
    let _ = writeln!(o, "; #@! TF.FileFunction,{}", if plated { "Plated,1,2,PTH" } else { "NonPlated,1,2,NPTH" });
    o.push_str("FMAT,2\nMETRIC,TZ\n");
    for (i, t) in tools.iter().enumerate() {
        let _ = writeln!(o, "T{}C{:.3}", i + 1, t);
    }
    o.push_str("%\nG90\nG05\n");
    for (i, t) in tools.iter().enumerate() {
        let _ = writeln!(o, "T{}", i + 1);
        for &(p, _) in holes.iter().filter(|h| (h.1 - t).abs() < 1e-9) {
            let _ = writeln!(o, "X{:.3}Y{:.3}", p.x, p.y);
        }
    }
    o.push_str("T0\nM30\n");
    o
}

/// Every file a fab and an assembler take, by name: `NAME-F_Cu.gbr`…,
/// the drills, the bill of materials, the placements.
pub fn fab(b: &BoardBuilt, sch: &crate::Built, name: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let layers: Vec<usize> = if b.layers == 4 { vec![0, 1, 2, 3] } else { vec![0, 3] };
    for (k, &l) in layers.iter().enumerate() {
        let side = if l == 0 { ",Top" } else if l == 3 { ",Bot" } else { ",Inr" };
        let function = format!("Copper,L{}{side}", k + 1);
        out.push((format!("{name}-{}.gbr", LAYERS[l].replace('.', "_")), copper_layer(b, l).finish(&function, "Positive")));
    }
    out.push((format!("{name}-F_Mask.gbr"), mask(b, 0).finish("Soldermask,Top", "Negative")));
    out.push((format!("{name}-B_Mask.gbr"), mask(b, 3).finish("Soldermask,Bot", "Negative")));
    out.push((format!("{name}-F_Paste.gbr"), paste(b, 0).finish("Paste,Top", "Positive")));
    out.push((format!("{name}-B_Paste.gbr"), paste(b, 3).finish("Paste,Bot", "Positive")));
    out.push((format!("{name}-F_Silkscreen.gbr"), silk(b, false).finish("Legend,Top", "Positive")));
    out.push((format!("{name}-B_Silkscreen.gbr"), silk(b, true).finish("Legend,Bot", "Positive")));
    out.push((format!("{name}-Edge_Cuts.gbr"), profile(b).finish("Profile,NP", "Positive")));
    let plated: Vec<(P2, f64)> = b.holes.iter().filter(|h| h.2).map(|h| (h.0, h.1)).collect();
    let bare: Vec<(P2, f64)> = b.holes.iter().filter(|h| !h.2).map(|h| (h.0, h.1)).collect();
    out.push((format!("{name}-PTH.drl"), excellon(&plated, true)));
    out.push((format!("{name}-NPTH.drl"), excellon(&bare, false)));
    out.push((format!("{name}-bom.csv"), crate::netlist::bom(sch)));
    let mut pos = String::from("Ref,Val,Package,PosX,PosY,Rot,Side\n");
    for p in b.parts.iter().filter(|p| p.placed) {
        let part = sch.parts.iter().find(|x| x.reference == p.reference);
        let val = part.and_then(|x| x.value.clone()).unwrap_or_else(|| part.map_or(String::new(), |x| x.symbol.title.clone()));
        let pkg = p.footprint.as_ref().map_or(String::new(), |f| f.name.clone());
        let _ = writeln!(pos, "{},{},{},{:.4},{:.4},{:.1},{}", p.reference, val.replace(',', " "), pkg, p.at[0], p.at[1], p.turn as f64, if p.back { "bottom" } else { "top" });
    }
    out.push((format!("{name}-pos.csv"), pos));
    out
}

/* ── read back ───────────────────────────────────────────────────────── */

/// What a Gerber file draws, as read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Read {
    pub function: Option<String>,
    /// Each aperture's definition by its D-code.
    pub apertures: Vec<(usize, String)>,
    /// Flashes: aperture, where, dark or clear.
    pub flashes: Vec<(usize, P2, bool)>,
    /// Lines: aperture, from, to, dark or clear.
    pub lines: Vec<(usize, P2, P2, bool)>,
    /// Regions' points, dark or clear.
    pub regions: Vec<(Vec<P2>, bool)>,
}

/// A Gerber file read: its format, apertures, flashes, lines and regions;
/// what it does not keep to is refused.
pub fn read_gerber(text: &str) -> Result<Read, String> {
    let mut r = Read::default();
    let mut at = P2::new(0.0, 0.0);
    let (mut ap, mut dark, mut region): (Option<usize>, bool, Option<Vec<P2>>) = (None, true, None);
    let (mut format, mut units, mut ended) = (false, false, false);
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        if ended {
            return Err(format!("line {}: after M02", i + 1));
        }
        if !l.ends_with('*') && !l.ends_with("*%") {
            return Err(format!("line {}: `{l}` does not end with `*`", i + 1));
        }
        let body = l.trim_end_matches('%').trim_end_matches('*').trim_start_matches('%');
        if let Some(f) = body.strip_prefix("TF.FileFunction,") {
            r.function = Some(f.to_string());
        } else if body == "FSLAX46Y46" {
            format = true;
        } else if body == "MOMM" {
            units = true;
        } else if body == "LPD" || body == "LPC" {
            dark = body == "LPD";
        } else if let Some(def) = body.strip_prefix("ADD") {
            let digits: String = def.chars().take_while(|c| c.is_ascii_digit()).collect();
            r.apertures.push((digits.parse().map_err(|_| format!("line {}: an aperture's number", i + 1))?, def[digits.len()..].to_string()));
        } else if body.starts_with("TF.") || body.starts_with("TA.") || body.starts_with("TO.") || body.starts_with("TD") {
        } else if body == "G36" {
            region = Some(Vec::new());
        } else if body == "G37" {
            r.regions.push((region.take().ok_or(format!("line {}: G37 with no G36", i + 1))?, dark));
        } else if body == "M02" {
            ended = true;
        } else if body.starts_with('D') && body[1..].chars().all(|c| c.is_ascii_digit()) {
            let d: usize = body[1..].parse().map_err(|_| format!("line {}: `{body}`", i + 1))?;
            if !r.apertures.iter().any(|a| a.0 == d) {
                return Err(format!("line {}: aperture D{d} is not defined", i + 1));
            }
            ap = Some(d);
        } else if body.starts_with('X') {
            if !format || !units {
                return Err(format!("line {}: a coordinate before the format and units", i + 1));
            }
            let y_at = body.find('Y').ok_or(format!("line {}: no Y", i + 1))?;
            let d_at = body.find('D').ok_or(format!("line {}: no D", i + 1))?;
            let x: i64 = body[1..y_at].parse().map_err(|_| format!("line {}: X", i + 1))?;
            let y: i64 = body[y_at + 1..d_at].parse().map_err(|_| format!("line {}: Y", i + 1))?;
            let p = P2::new(x as f64 / 1e6, y as f64 / 1e6);
            match &body[d_at..] {
                "D02" => {
                    if let Some(reg) = region.as_mut() {
                        reg.push(p);
                    }
                }
                "D01" => {
                    if let Some(reg) = region.as_mut() {
                        reg.push(p);
                    } else {
                        r.lines.push((ap.ok_or(format!("line {}: a line with no aperture", i + 1))?, at, p, dark));
                    }
                }
                "D03" => r.flashes.push((ap.ok_or(format!("line {}: a flash with no aperture", i + 1))?, p, dark)),
                other => return Err(format!("line {}: `{other}` is not an operation", i + 1)),
            }
            at = p;
        } else {
            return Err(format!("line {}: `{body}` is not read here", i + 1));
        }
    }
    if !ended {
        return Err("no M02 at the end".into());
    }
    Ok(r)
}

/// An Excellon file read: each hole's place and drill.
pub fn read_excellon(text: &str) -> Result<Vec<(P2, f64)>, String> {
    let mut tools: Vec<(usize, f64)> = Vec::new();
    let mut tool = None;
    let mut out = Vec::new();
    let (mut header, mut metric, mut ended) = (true, false, false);
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with(';') {
            continue;
        }
        if header {
            if l == "%" {
                header = false;
            } else if l == "METRIC,TZ" || l == "METRIC" {
                metric = true;
            } else if let Some(t) = l.strip_prefix('T') {
                let (n, d) = t.split_once('C').ok_or(format!("line {}: a tool without its size", i + 1))?;
                tools.push((n.parse().map_err(|_| format!("line {}: a tool's number", i + 1))?, d.parse().map_err(|_| format!("line {}: a tool's size", i + 1))?));
            }
            continue;
        }
        if l == "M30" {
            ended = true;
        } else if let Some(t) = l.strip_prefix('T') {
            let n: usize = t.parse().map_err(|_| format!("line {}: `{l}`", i + 1))?;
            tool = if n == 0 { None } else { Some(tools.iter().find(|x| x.0 == n).ok_or(format!("line {}: tool T{n} is not defined", i + 1))?.1) };
        } else if let Some(xy) = l.strip_prefix('X') {
            let (x, y) = xy.split_once('Y').ok_or(format!("line {}: no Y", i + 1))?;
            let d = tool.ok_or(format!("line {}: a hole with no tool", i + 1))?;
            out.push((P2::new(x.parse().map_err(|_| format!("line {}: X", i + 1))?, y.parse().map_err(|_| format!("line {}: Y", i + 1))?), d));
        }
    }
    if !metric {
        return Err("the file does not say it is metric".into());
    }
    if !ended {
        return Err("no M30 at the end".into());
    }
    Ok(out)
}
