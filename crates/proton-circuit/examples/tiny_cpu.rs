//! Writes `tests/tiny-cpu.circuit/`: a small CPU, an accumulator machine
//! of eight bits, for the speed checks to run and anyone to open. Each
//! pin has a short wire out to a tunnel named for its net, so the parts
//! need no routing.
//!
//! `cargo run -p proton-circuit --example tiny_cpu`
//!
//! An instruction is twelve bits, the operation over an operand `k`:
//! 0 `LDI k`, 1 `ADD k`, 2 `SUB k`, 3 `JMP k`, 4 `JNZ k` (jump while the
//! accumulator is not zero); the rest do nothing. The program counts the
//! accumulator down from ten, adds seven and stops in a loop.

use std::collections::BTreeMap;

use proton_circuit::circ::{self, Drawing, Part, Project, Wire};

/// A part, its settings, and the net on each of its pins in pin order
/// (`""` leaves a pin unjoined).
struct Spec {
    id: &'static str,
    kind: &'static str,
    at: (i32, i32),
    attrs: &'static [(&'static str, &'static str)],
    nets: &'static [&'static str],
}

const PARTS: &[Spec] = &[
    Spec { id: "CLK", kind: "clock", at: (2, 2), attrs: &[("label", "CLK")], nets: &["clk"] },
    Spec { id: "K1", kind: "const", at: (2, 6), attrs: &[("value", "1")], nets: &["one"] },
    Spec { id: "K2", kind: "const", at: (2, 10), attrs: &[("bits", "8"), ("value", "1")], nets: &["one8"] },
    Spec { id: "K3", kind: "const", at: (2, 14), attrs: &[("value", "0")], nets: &["zero"] },
    Spec { id: "K4", kind: "const", at: (2, 18), attrs: &[("bits", "8"), ("value", "0")], nets: &["zero8"] },
    // Fetch: the program counter, one on, a jump, the program.
    Spec { id: "PC", kind: "register", at: (10, 2), attrs: &[("bits", "8"), ("label", "PC")], nets: &["npc", "clk", "one", "pc"] },
    Spec { id: "INC", kind: "add", at: (10, 10), attrs: &[("bits", "8")], nets: &["pc", "one8", "zero", "pc1", ""] },
    Spec { id: "PM", kind: "mux", at: (10, 18), attrs: &[("bits", "8")], nets: &["jump", "pc1", "k", "npc"] },
    Spec { id: "ROM", kind: "rom", at: (10, 26), attrs: &[("addr", "8"), ("bits", "12"), ("file", "program"), ("label", "program")], nets: &["pc", "one", "ir"] },
    Spec { id: "IR", kind: "split", at: (10, 34), attrs: &[("in", "12"), ("out", "8,4")], nets: &["ir", "k", "op"] },
    // Execute: the accumulator and what it can become.
    Spec { id: "ACC", kind: "register", at: (24, 2), attrs: &[("bits", "8"), ("label", "A")], nets: &["nacc", "clk", "one", "acc"] },
    Spec { id: "ADD", kind: "add", at: (24, 10), attrs: &[("bits", "8")], nets: &["acc", "k", "zero", "sum", ""] },
    Spec { id: "SUB", kind: "subtract", at: (24, 18), attrs: &[("bits", "8")], nets: &["acc", "k", "zero", "diff", ""] },
    Spec { id: "AM", kind: "mux", at: (24, 26), attrs: &[("bits", "8"), ("sel", "2")], nets: &["asel", "k", "sum", "diff", "acc", "nacc"] },
    Spec { id: "Z", kind: "compare", at: (24, 36), attrs: &[("bits", "8")], nets: &["acc", "zero8", "", "z", ""] },
    // Control: the operation and the zero flag look up what to do.
    Spec { id: "CA", kind: "split", at: (38, 2), attrs: &[("in", "4,1"), ("out", "5")], nets: &["op", "z", "caddr"] },
    Spec { id: "CTL", kind: "rom", at: (38, 10), attrs: &[("addr", "5"), ("bits", "3"), ("file", "control"), ("label", "control")], nets: &["caddr", "one", "ctl"] },
    Spec { id: "CS", kind: "split", at: (38, 18), attrs: &[("in", "3"), ("out", "2,1")], nets: &["ctl", "asel", "jump"] },
    Spec { id: "OUT", kind: "out", at: (38, 26), attrs: &[("bits", "8"), ("label", "ACC")], nets: &["acc"] },
];

/// LDI 10, SUB 1, JNZ 1, ADD 7, JMP 4.
const PROGRAM: [u64; 5] = [0x00a, 0x201, 0x401, 0x107, 0x304];

/// What each operation does, by zero flag then operation: the
/// accumulator's source (0 `k`, 1 the sum, 2 the difference, 3 itself)
/// and, in the third bit, a jump.
fn control() -> Vec<u64> {
    let mut words = Vec::new();
    for z in 0..2 {
        for op in 0..16 {
            words.push(match (op, z) {
                (0, _) => 0,
                (1, _) => 1,
                (2, _) => 2,
                (3, _) | (4, 0) => 0b111,
                _ => 0b011,
            });
        }
    }
    words
}

fn main() {
    let mut d = Drawing::default();
    for s in PARTS {
        let attrs: BTreeMap<String, String> = s.attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        d.parts.push(Part { id: s.id.into(), kind: s.kind.into(), x: s.at.0, y: s.at.1, rot: 0, mirror: false, attrs });
    }
    let mut project = Project { top: "cpu".into(), circuits: BTreeMap::from([("cpu".to_string(), d.clone())]), ..Project::new() };
    project.circuits.remove("main");
    project.data.insert("program".into(), proton_circuit::hex::write(&PROGRAM));
    project.data.insert("control".into(), proton_circuit::hex::write(&control()));
    // A stub out of each joined pin — left from the left edge, right for an
    // output, down otherwise — and a tunnel at its end.
    let mut n = 0;
    for ((id, pins), s) in project.pins("cpu").into_iter().zip(PARTS) {
        let pins = pins.unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(pins.len(), s.nets.len(), "{id} has {} pins", pins.len());
        for (pin, net) in pins.iter().zip(s.nets) {
            if net.is_empty() {
                continue;
            }
            let (dx, dy, rot) = if pin.x == s.at.0 && pin.input { (-1, 0, 2) } else if !pin.input { (1, 0, 0) } else { (0, 1, 3) };
            let end = (pin.x + dx, pin.y + dy);
            d.wires.push(Wire::new((pin.x, pin.y), end));
            n += 1;
            let attrs = BTreeMap::from([("net".to_string(), net.to_string())]);
            d.parts.push(Part { id: format!("T{n}"), kind: "tunnel".into(), x: end.0, y: end.1, rot, mirror: false, attrs });
        }
    }
    d.wires.sort();
    project.circuits.insert("cpu".into(), d);
    // Ten counted down, two cycles a count, then seven added and kept.
    let mut rows = String::from("CLK ACC\nC 10\nC 9\nC 9\nC 8\nC 8\n");
    rows.push_str("repeat(30) C X\nC 7\nC 7\n");
    project.tests.insert("cpu".into(), BTreeMap::from([("countdown".to_string(), rows)]));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tiny-cpu.circuit");
    circ::write_project(&dir, &project).unwrap();
    println!("wrote {}", dir.display());
}
