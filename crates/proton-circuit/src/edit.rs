//! Editing a drawing: what a click or a key does to a circuit's parts and
//! wires, as plain changes to a [`Drawing`] that the editor records as steps.
//!
//! Wires join only where their ends meet (see [`crate::dig`]), so drawing
//! one keeps that true the way Digital's editor does: a new wire ending in
//! the middle of another splits it there, and an old wire's end lying along
//! a new one splits the new one, so a junction drawn is a junction.

use std::collections::{BTreeMap, BTreeSet};

use crate::circ::{self, Drawing, Part, Wire};

/// Grid steps a part may be placed from the origin, each way.
pub const MOST_STEPS: i32 = 100_000;

/// The letters a new part's id starts with.
pub fn prefix(kind: &str) -> &'static str {
    match kind {
        "in" => "In",
        "out" => "Out",
        "clock" => "Clk",
        "const" | "ground" | "vdd" => "K",
        "tunnel" => "T",
        "split" => "P",
        "text" => "Note",
        "led" | "probe" => "L",
        "mux" | "demux" | "decoder" | "priority" => "M",
        "register" | "dff" | "tff" | "jkff" | "rsff" => "R",
        "ram2" => "RAM",
        k if k.starts_with("sub:") => "S",
        _ => "U",
    }
}

/// An id no part of the drawing has: the prefix and the next number.
pub fn fresh_id(d: &Drawing, prefix: &str) -> String {
    let taken: BTreeSet<&str> = d.parts.iter().map(|p| p.id.as_str()).collect();
    (1..).map(|n| format!("{prefix}{n}")).find(|id| !taken.contains(id.as_str())).unwrap_or_default()
}

/// Whether an id can name a part: a letter, then letters, digits and `_`.
pub fn is_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) && id.len() <= 32 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A part fit to go in circuit `circuit` of a project with these circuits:
/// a kind Proton knows, or another circuit that does not place this one,
/// and settings it can read.
pub fn check_part(p: &Part, circuit: &str, circuits: &BTreeMap<String, Drawing>) -> Result<(), String> {
    if !is_id(&p.id) {
        return Err(format!("“{}” cannot be a part's id: a letter, then letters, digits and _", p.id));
    }
    if p.x.abs() > MOST_STEPS || p.y.abs() > MOST_STEPS {
        return Err(format!("{} is too far from the rest", p.id));
    }
    if let Some(sub) = p.kind.strip_prefix("sub:") {
        if !circuits.contains_key(sub) {
            return Err(format!("there is no circuit {sub} to place"));
        }
        if places(sub, circuit, circuits, 0) {
            return Err(format!("{sub} cannot go in {circuit}: it would place itself"));
        }
    } else if !circ::kinds().any(|k| k == p.kind) {
        return Err(format!("“{}” is not a part Proton knows", p.kind));
    }
    for k in p.attrs.keys() {
        if !circ::settings().any(|s| s == k) {
            return Err(format!("{}: “{k}” is not a setting", p.id));
        }
    }
    Ok(())
}

/// Whether circuit `outer` places `inner`, at any depth, or is it.
fn places(outer: &str, inner: &str, circuits: &BTreeMap<String, Drawing>, depth: u32) -> bool {
    if outer == inner {
        return true;
    }
    if depth > 32 {
        return true;
    }
    circuits.get(outer).is_some_and(|d| d.parts.iter().filter_map(|p| p.kind.strip_prefix("sub:")).any(|s| places(s, inner, circuits, depth + 1)))
}

fn inside(p: (i32, i32), w: &Wire) -> bool {
    let (a, b) = (w.a, w.b);
    (a.0 == b.0 && p.0 == a.0 && p.1 > a.1.min(b.1) && p.1 < a.1.max(b.1)) || (a.1 == b.1 && p.1 == a.1 && p.0 > a.0.min(b.0) && p.0 < a.0.max(b.0))
}

/// A wire drawn from `a` to `b`, straight across or down, split and
/// splitting where it meets others' ends. False when there is nothing to
/// draw: no length, or not straight.
pub fn add_wire(d: &mut Drawing, a: (i32, i32), b: (i32, i32)) -> bool {
    if a == b || (a.0 != b.0 && a.1 != b.1) {
        return false;
    }
    // Old wires the new one ends in the middle of are split there.
    let mut wires: Vec<Wire> = Vec::with_capacity(d.wires.len() + 2);
    for w in d.wires.drain(..) {
        let cut: Vec<(i32, i32)> = [a, b].into_iter().filter(|p| inside(*p, &w)).collect();
        if cut.is_empty() {
            wires.push(w);
        } else {
            let mut pts = vec![w.a, w.b];
            pts.extend(cut);
            pts.sort();
            pts.dedup();
            wires.extend(pts.windows(2).map(|s| Wire::new(s[0], s[1])));
        }
    }
    // The new one split where old ends lie along it.
    let new = Wire::new(a, b);
    let mut pts: Vec<(i32, i32)> = vec![new.a, new.b];
    pts.extend(wires.iter().flat_map(|w| [w.a, w.b]).filter(|p| inside(*p, &new)));
    pts.sort();
    pts.dedup();
    wires.extend(pts.windows(2).map(|s| Wire::new(s[0], s[1])));
    wires.sort();
    wires.dedup();
    d.wires = wires;
    true
}

/// A quarter turn more, about the part's own origin, as `r` does.
pub fn rotate(p: &mut Part) {
    p.rot = (p.rot + 1) % 4;
}

/// The parts and wires named, as `.circ` text: what is copied.
pub fn copy(d: &Drawing, ids: &[String], wires: &[Wire]) -> String {
    let parts = d.parts.iter().filter(|p| ids.contains(&p.id)).cloned().collect();
    let wires = d.wires.iter().filter(|w| wires.contains(w)).copied().collect();
    circ::write(&Drawing { parts, wires, width: None })
}

/// `.circ` text put into a drawing moved by (dx, dy): a part whose id is
/// taken gets a fresh one. The ids it ended with.
pub fn paste(d: &mut Drawing, text: &str, dx: i32, dy: i32) -> Result<Vec<String>, String> {
    let pasted = circ::parse(text)?;
    if pasted.parts.is_empty() && pasted.wires.is_empty() {
        return Err("there is nothing in that to paste".into());
    }
    let mut made = Vec::new();
    for mut p in pasted.parts {
        if !is_id(&p.id) || d.parts.iter().any(|q| q.id == p.id) {
            p.id = fresh_id(d, prefix(&p.kind));
        }
        p.x += dx;
        p.y += dy;
        made.push(p.id.clone());
        d.parts.push(p);
    }
    for w in pasted.wires {
        add_wire(d, (w.a.0 + dx, w.a.1 + dy), (w.b.0 + dx, w.b.1 + dy));
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wires(d: &Drawing) -> Vec<[i32; 4]> {
        d.wires.iter().map(|w| [w.a.0, w.a.1, w.b.0, w.b.1]).collect()
    }

    #[test]
    fn a_wire_drawn_to_the_middle_of_another_makes_a_junction() {
        let mut d = Drawing::default();
        assert!(add_wire(&mut d, (0, 0), (10, 0)));
        // A branch from the middle: the long wire is split where it lands.
        assert!(add_wire(&mut d, (4, 5), (4, 0)));
        assert_eq!(wires(&d), vec![[0, 0, 4, 0], [4, 0, 4, 5], [4, 0, 10, 0]]);
        // A wire across an old end is split there too.
        assert!(add_wire(&mut d, (2, 5), (8, 5)));
        assert_eq!(wires(&d), vec![[0, 0, 4, 0], [2, 5, 4, 5], [4, 0, 4, 5], [4, 0, 10, 0], [4, 5, 8, 5]]);
        // Nothing to draw: no length, or a slant.
        assert!(!add_wire(&mut d, (1, 1), (1, 1)));
        assert!(!add_wire(&mut d, (0, 0), (3, 3)));
        // The circuit it makes joins at the junction: an input to two outputs.
        let mut p = circ::Project::new();
        let mut d = circ::parse("part I in at 0,0 label=I\npart X out at 10,0 label=X\npart Y out at 4,5 label=Y\n").unwrap();
        add_wire(&mut d, (0, 0), (10, 0));
        add_wire(&mut d, (4, 0), (4, 5));
        p.circuits.insert("main".into(), d);
        let b = p.build("main").unwrap();
        let mut sim = crate::Sim::new(b.netlist, 1).unwrap();
        sim.set(b.inputs["I"].0, 1).unwrap();
        assert_eq!((sim.shown(b.outputs["X"]).get(), sim.shown(b.outputs["Y"]).get()), (1, 1));
    }

    #[test]
    fn ids_are_fresh_and_a_paste_keeps_what_is_free() {
        let mut d = circ::parse("part U1 and at 0,0\npart U2 or at 0,4\n").unwrap();
        assert_eq!(fresh_id(&d, "U"), "U3");
        let text = copy(&d, &["U1".into()], &[]);
        assert_eq!(text, "part U1 and at 0,0\n");
        let made = paste(&mut d, &format!("{text}part Z xor at 1,1\nwire 0,0 0,2\n"), 5, 0).unwrap();
        assert_eq!(made, vec!["U3", "Z"]);
        let u3 = d.parts.iter().find(|p| p.id == "U3").unwrap();
        assert_eq!((u3.x, u3.y), (5, 0));
        assert_eq!(wires(&d), vec![[5, 0, 5, 2]]);
        assert!(paste(&mut d, "", 0, 0).is_err());
        assert!(!is_id("1a") && !is_id("a b") && is_id("Cin_2"));
    }

    #[test]
    fn a_part_is_checked_against_the_project() {
        let mut circuits = BTreeMap::new();
        circuits.insert("half".to_string(), circ::parse("part A in at 0,0 label=A\n").unwrap());
        circuits.insert("main".to_string(), circ::parse("part H sub:half at 0,0\n").unwrap());
        let part = |kind: &str| Part { id: "P1".into(), kind: kind.into(), x: 0, y: 0, rot: 0, mirror: false, attrs: Default::default() };
        assert!(check_part(&part("and"), "main", &circuits).is_ok());
        assert!(check_part(&part("sub:half"), "main", &circuits).is_ok());
        assert!(check_part(&part("frob"), "main", &circuits).unwrap_err().contains("frob"));
        assert!(check_part(&part("sub:nowhere"), "main", &circuits).is_err());
        assert!(check_part(&part("sub:main"), "half", &circuits).unwrap_err().contains("itself"), "half would place main, which places half");
        assert!(check_part(&part("sub:main"), "main", &circuits).is_err());
        let mut p = part("and");
        p.attrs.insert("colour".into(), "red".into());
        assert!(check_part(&p, "main", &circuits).is_err());
    }
}
