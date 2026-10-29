//! State machines: written as text, run, tabulated, and built as circuits.
//!
//! ```text
//! inputs X
//! outputs Z
//! state S0 Z=0          # a Moore output, on the state
//! state S1 Z=1
//! S0 -> S1 when X       # a transition, when its condition holds
//! S1 -> S0 when X' / Z=1   # a Mealy output, on the transition
//! ```
//!
//! The first state is where it starts. At a clock edge it takes the first
//! transition from its state whose condition holds — an expression over the
//! inputs, as [`crate::analysis`] reads them; none written means always —
//! or stays. Outputs not given are 0. States are numbered in the order they
//! are first named and coded in binary, `Q0` the lowest bit.
//!
//! Built, a machine is two circuits: `<name>_logic`, the next state and
//! the outputs worked out from the state and the inputs, minimised and
//! laid out by [`crate::analysis::synthesise`]; and `<name>`, its inputs, a
//! clock `Clk`, a D or JK flip-flop a state bit, the logic placed as a
//! subcircuit, and its outputs, joined by tunnels named for each signal.
//! A test table of the machine run comes with it.

use std::collections::{BTreeMap, HashMap};

use crate::analysis::{self, Style, Table};
use crate::circ::{self, Drawing, Part, Project, Wire};

/// No more states than this, nor inputs.
pub const MOST_STATES: usize = 64;
pub const MOST_INPUTS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub from: usize,
    pub to: usize,
    /// As written; empty for always.
    pub when: String,
    /// Outputs set on this transition (Mealy).
    pub outputs: Vec<(String, bool)>,
    /// Which input rows it holds for: a bit a row of the inputs.
    holds: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub states: Vec<String>,
    /// Outputs set in each state (Moore).
    pub state_outputs: Vec<Vec<(String, bool)>>,
    pub transitions: Vec<Transition>,
}

/// One row of a state table: a state and an input row, where it goes, and
/// what it puts out on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub state: usize,
    pub inputs: u32,
    pub next: usize,
    pub outputs: Vec<bool>,
}

fn assignments(words: &[&str], outputs: &[String], n: usize) -> Result<Vec<(String, bool)>, String> {
    words
        .iter()
        .map(|w| {
            let (k, v) = w.split_once('=').ok_or_else(|| format!("line {n}: “{w}” is not an output set, as Z=1"))?;
            if !outputs.iter().any(|o| o == k) {
                return Err(format!("line {n}: {k} is not one of the outputs"));
            }
            match v {
                "0" => Ok((k.to_string(), false)),
                "1" => Ok((k.to_string(), true)),
                _ => Err(format!("line {n}: {k} is set to 0 or 1")),
            }
        })
        .collect()
}

/// A machine read from its text.
pub fn parse(text: &str) -> Result<Machine, String> {
    let mut m = Machine { inputs: vec![], outputs: vec![], states: vec![], state_outputs: vec![], transitions: vec![] };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut state = |m: &mut Machine, name: &str, n: usize| -> Result<usize, String> {
        if !crate::edit::is_id(name) {
            return Err(format!("line {n}: “{name}” cannot name a state"));
        }
        if let Some(i) = index.get(name) {
            return Ok(*i);
        }
        if m.states.len() == MOST_STATES {
            return Err(format!("more than {MOST_STATES} states"));
        }
        m.states.push(name.to_string());
        m.state_outputs.push(Vec::new());
        index.insert(name.to_string(), m.states.len() - 1);
        Ok(m.states.len() - 1)
    };
    let mut pending: Vec<(usize, usize, String, Vec<(String, bool)>, usize)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words[0] {
            "inputs" | "outputs" => {
                let names: Vec<String> = words[1..].iter().map(|s| s.to_string()).collect();
                if let Some(bad) = names.iter().find(|s| !crate::edit::is_id(s)) {
                    return Err(format!("line {n}: “{bad}” cannot name a signal"));
                }
                if words[0] == "inputs" {
                    if names.len() > MOST_INPUTS {
                        return Err(format!("line {n}: no more than {MOST_INPUTS} inputs"));
                    }
                    m.inputs = names;
                } else {
                    m.outputs = names;
                }
            }
            "state" => {
                let name = words.get(1).ok_or_else(|| format!("line {n}: a state needs a name"))?;
                let s = state(&mut m, name, n)?;
                let outs = assignments(&words[2..], &m.outputs, n)?;
                m.state_outputs[s] = outs;
            }
            _ => {
                let (left, out) = match line.split_once('/') {
                    Some((l, o)) => (l, o.split_whitespace().collect::<Vec<_>>()),
                    None => (line, Vec::new()),
                };
                let (from, rest) = left.split_once("->").ok_or_else(|| format!("line {n}: “{line}” is not inputs, outputs, a state or a transition (A -> B when …)"))?;
                let (to, when) = match rest.split_once(" when ") {
                    Some((t, w)) => (t, w.trim().to_string()),
                    None => (rest, String::new()),
                };
                let (f, t) = (state(&mut m, from.trim(), n)?, state(&mut m, to.trim(), n)?);
                let outs = assignments(&out, &m.outputs, n)?;
                pending.push((f, t, when, outs, n));
            }
        }
    }
    if m.states.is_empty() {
        return Err("a machine needs a state".into());
    }
    let k = m.inputs.len();
    for (from, to, when, outputs, n) in pending {
        let holds = if when.is_empty() {
            vec![true; 1 << k]
        } else {
            // Read as an expression over the machine's inputs, every row.
            let t = analysis::expressions(&format!("H = {when}")).map_err(|e| format!("line {n}: {e}"))?;
            if let Some(stray) = t.inputs.iter().find(|v| !m.inputs.contains(v)) {
                return Err(format!("line {n}: {stray} is not one of the inputs"));
            }
            (0..1u32 << k)
                .map(|row| {
                    let pos: usize = t.inputs.iter().enumerate().fold(0, |acc, (j, v)| {
                        let i = m.inputs.iter().position(|x| x == v).unwrap_or(0);
                        acc | ((row as usize >> (k - 1 - i) & 1) << (t.inputs.len() - 1 - j))
                    });
                    t.values[0][pos] == b'1'
                })
                .collect()
        };
        m.transitions.push(Transition { from, to, when, outputs, holds });
    }
    Ok(m)
}

impl Machine {
    /// Whether outputs are on transitions.
    pub fn mealy(&self) -> bool {
        self.transitions.iter().any(|t| !t.outputs.is_empty())
    }

    /// From a state, on an input row: the next state and the outputs.
    pub fn step(&self, state: usize, inputs: u32) -> (usize, Vec<bool>) {
        let taken = self.transitions.iter().find(|t| t.from == state && t.holds.get(inputs as usize).copied().unwrap_or(false));
        let next = taken.map_or(state, |t| t.to);
        let mut outs: Vec<bool> = self.outputs.iter().map(|o| self.state_outputs[state].iter().any(|(k, v)| k == o && *v)).collect();
        if let Some(t) = taken {
            for (i, o) in self.outputs.iter().enumerate() {
                if let Some((_, v)) = t.outputs.iter().find(|(k, _)| k == o) {
                    outs[i] = *v;
                }
            }
        }
        (next, outs)
    }

    /// What it shows in a state with these inputs, between clock edges: a
    /// Moore machine its state's outputs; a Mealy one those of the
    /// transition it would take.
    pub fn showing(&self, state: usize, inputs: u32) -> Vec<bool> {
        self.step(state, inputs).1
    }

    /// Bits to number the states.
    pub fn bits(&self) -> usize {
        let mut b = 1;
        while (1usize << b) < self.states.len() {
            b += 1;
        }
        b
    }

    /// The state table: every state, every input row.
    pub fn table(&self) -> Vec<Row> {
        let mut out = Vec::new();
        for s in 0..self.states.len() {
            for row in 0..1u32 << self.inputs.len() {
                let (next, outputs) = self.step(s, row);
                out.push(Row { state: s, inputs: row, next, outputs });
            }
        }
        out
    }
}

/// Which flip-flop a state bit is kept in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipFlop {
    D,
    Jk,
}

/// The logic's truth table: from the state bits (`Q…`, highest first) and
/// the inputs, the flip-flops' inputs and the outputs — the outputs of the
/// state shown, as wired, which for a Moore machine depend on it alone.
pub fn logic_table(m: &Machine, ff: FlipFlop) -> Table {
    let b = m.bits();
    let k = m.inputs.len();
    let mut inputs: Vec<String> = (0..b).rev().map(|i| format!("Q{i}")).collect();
    inputs.extend(m.inputs.iter().cloned());
    let mut outputs = Vec::new();
    for i in (0..b).rev() {
        match ff {
            FlipFlop::D => outputs.push(format!("D{i}")),
            FlipFlop::Jk => {
                outputs.push(format!("J{i}"));
                outputs.push(format!("K{i}"));
            }
        }
    }
    outputs.extend(m.outputs.iter().cloned());
    let mut values = vec![Vec::with_capacity(1 << (b + k)); outputs.len()];
    for row in 0..1u32 << (b + k) {
        let code = (row >> k) as usize;
        let ins = row & ((1 << k) - 1);
        let mut col = 0;
        let mut push = |c: u8| {
            values[col].push(c);
            col += 1;
        };
        if code >= m.states.len() {
            // A code no state has: nothing cares.
            for _ in 0..outputs.len() {
                push(b'x');
            }
            continue;
        }
        let (next, outs) = m.step(code, ins);
        for i in (0..b).rev() {
            let (q, q2) = (code >> i & 1 == 1, next >> i & 1 == 1);
            match ff {
                FlipFlop::D => push(if q2 { b'1' } else { b'0' }),
                FlipFlop::Jk => {
                    // The excitation table: J matters from 0, K from 1.
                    let (j, kk) = match (q, q2) {
                        (false, false) => (b'0', b'x'),
                        (false, true) => (b'1', b'x'),
                        (true, false) => (b'x', b'1'),
                        (true, true) => (b'x', b'0'),
                    };
                    push(j);
                    push(kk);
                }
            }
        }
        for o in outs {
            push(if o { b'1' } else { b'0' });
        }
    }
    Table { inputs, outputs, values }
}

/// A test table of the machine run: from its first state, each input row
/// in turn and then a walk of every transition, a clock pulse a row, the
/// outputs it shows after.
pub fn test_text(m: &Machine) -> String {
    let k = m.inputs.len();
    let header: Vec<&str> = m.inputs.iter().map(String::as_str).chain(["Clk"]).chain(m.outputs.iter().map(String::as_str)).collect();
    let mut s = format!("{}\n", header.join(" "));
    let mut state = 0;
    let mut seq: Vec<u32> = (0..1u32 << k).collect();
    // A deterministic walk long enough to take most transitions.
    let mut x = 0x2545_f491u32;
    for _ in 0..(4 * m.states.len() * (1 << k)).min(256) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        seq.push(x & ((1 << k) - 1));
    }
    for row in seq {
        let (next, _) = m.step(state, row);
        state = next;
        let shown = m.showing(state, row);
        let mut cells: Vec<String> = (0..k).map(|i| ((row >> (k - 1 - i)) & 1).to_string()).collect();
        cells.push("C".into());
        cells.extend(shown.iter().map(|b| (*b as u8).to_string()));
        s.push_str(&cells.join(" "));
        s.push('\n');
    }
    s
}

/// The machine built into a project as `<name>` and `<name>_logic`, with
/// its run as the test "machine".
pub fn build(m: &Machine, ff: FlipFlop, name: &str, project: &mut Project) -> Result<(), String> {
    let logic_name = format!("{name}_logic");
    for n in [name, logic_name.as_str()] {
        if project.circuits.contains_key(n) {
            return Err(format!("there is a circuit {n} already"));
        }
    }
    let table = logic_table(m, ff);
    let logic = analysis::synthesise(&table, Style::AndOr)?;
    project.circuits.insert(logic_name.clone(), logic);

    let b = m.bits();
    let part = |id: &str, kind: &str, x: i32, y: i32, attrs: &[(&str, &str)]| Part {
        id: id.into(),
        kind: kind.into(),
        x,
        y,
        rot: 0,
        mirror: false,
        attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    };
    // The parts, then where their pins are, then a stub and a tunnel a pin.
    let mut d = Drawing::default();
    let mut y = 0;
    for i in &m.inputs {
        d.parts.push(part(i, "in", 0, y, &[("label", i)]));
        y += 2;
    }
    d.parts.push(part("Clk", "clock", 0, y, &[("label", "Clk")]));
    d.parts.push(part("Logic", &format!("sub:{logic_name}"), 8, 0, &[]));
    let ff_kind = if ff == FlipFlop::D { "dff" } else { "jkff" };
    let logic_rows = table.inputs.len().max(table.outputs.len()) as i32;
    let mut fy = logic_rows + 4;
    for i in 0..b {
        d.parts.push(part(&format!("F{i}"), ff_kind, 8, fy, &[]));
        fy += 5;
    }
    for (j, o) in m.outputs.iter().enumerate() {
        d.parts.push(part(o, "out", 22, 2 * j as i32, &[("label", o)]));
    }
    project.circuits.insert(name.to_string(), d.clone());
    let pins: HashMap<String, Vec<circ::PinAt>> = project.pins(name).into_iter().map(|(id, p)| p.map(|p| (id, p))).collect::<Result<_, _>>()?;
    // Each pin's signal.
    let logic_ins: Vec<String> = sub_pin_names(&table.inputs);
    let logic_outs: Vec<String> = sub_pin_names(&table.outputs);
    let mut signals: Vec<(circ::PinAt, String)> = Vec::new();
    for i in &m.inputs {
        signals.push((pins[i][0], i.clone()));
    }
    signals.push((pins["Clk"][0], "Clk".into()));
    let lp = &pins["Logic"];
    for (p, s) in lp.iter().filter(|p| p.input).zip(&logic_ins) {
        signals.push((*p, s.clone()));
    }
    for (p, s) in lp.iter().filter(|p| !p.input).zip(&logic_outs) {
        signals.push((*p, s.clone()));
    }
    for i in 0..b {
        let fp = &pins[&format!("F{i}")];
        // D, C → Q, !Q; or J, C, K → Q, !Q.
        let names: Vec<String> = match ff {
            FlipFlop::D => vec![format!("D{i}"), "Clk".into(), format!("Q{i}"), String::new()],
            FlipFlop::Jk => vec![format!("J{i}"), "Clk".into(), format!("K{i}"), format!("Q{i}"), String::new()],
        };
        for (p, s) in fp.iter().zip(names) {
            if !s.is_empty() {
                signals.push((*p, s));
            }
        }
    }
    for o in &m.outputs {
        signals.push((pins[o][0], o.clone()));
    }
    let mut t = 0;
    for (p, s) in signals {
        // A part's input pins are on its left: the stub goes left, the
        // tunnel faces away; outputs the other way. The circuit's own inputs
        // are drivers, so their stubs go right.
        let driver = !p.input;
        let dx = if driver { 1 } else { -1 };
        let end = (p.x + dx, p.y);
        d.wires.push(Wire::new((p.x, p.y), end));
        t += 1;
        d.parts.push(Part {
            id: format!("T{t}"),
            kind: "tunnel".into(),
            x: end.0,
            y: end.1,
            rot: if driver { 0 } else { 2 },
            mirror: false,
            attrs: [("net".to_string(), s)].into_iter().collect(),
        });
    }
    let d = circ::parse(&circ::write(&d))?;
    project.circuits.insert(name.to_string(), d);
    project.tests.entry(name.to_string()).or_default().insert("machine".into(), test_text(m));
    Ok(())
}

/// A subcircuit's pins are its `in` and `out` parts in id order; the
/// table's signals sorted so, the way the logic's parts are.
fn sub_pin_names(names: &[String]) -> Vec<String> {
    let probe = Drawing {
        parts: names.iter().map(|n| Part { id: n.clone(), kind: "in".into(), x: 0, y: 0, rot: 0, mirror: false, attrs: BTreeMap::new() }).collect(),
        wires: vec![],
        width: None,
    };
    circ::ordered(&probe).iter().map(|p| p.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Says 1 for a clock after it has seen two 1s in a row: Moore.
    const TWO_ONES: &str = "inputs X\noutputs Z\nstate S0\nstate S1\nstate S2 Z=1\nS0 -> S1 when X\nS1 -> S2 when X\nS1 -> S0 when X'\nS2 -> S0 when !X\n";
    /// Says 1 as an input differs from the last: Mealy.
    const CHANGE: &str = "inputs X\noutputs Z\nA -> B when X / Z=1\nB -> A when X' / Z=1\n";

    #[test]
    fn a_machine_is_read_run_and_tabulated() {
        let m = parse(TWO_ONES).unwrap();
        assert_eq!((m.states.len(), m.bits(), m.mealy()), (3, 2, false));
        let mut s = 0;
        let mut said = Vec::new();
        for x in [1, 1, 1, 0, 1, 0, 1, 1] {
            s = m.step(s, x).0;
            said.push(m.showing(s, x)[0] as u8);
        }
        assert_eq!(said, vec![0, 1, 1, 0, 0, 0, 0, 1]);
        assert_eq!(m.table().len(), 6);
        let c = parse(CHANGE).unwrap();
        assert!(c.mealy());
        assert_eq!(c.step(0, 1), (1, vec![true]));
        assert_eq!(c.step(0, 0), (0, vec![false]), "no transition: it stays, and says nothing");
        assert!(parse("inputs X\nA -> B when Y").unwrap_err().contains("Y is not one of the inputs"));
        assert!(parse("outputs Z\nstate A W=1").unwrap_err().contains("W"));
        assert!(parse("").is_err());
    }

    #[test]
    fn the_logic_tables_are_the_excitation_of_the_transitions() {
        let m = parse(TWO_ONES).unwrap();
        let d = logic_table(&m, FlipFlop::D);
        assert_eq!((d.inputs.join(" "), d.outputs.join(" ")), ("Q1 Q0 X".into(), "D1 D0 Z".into()));
        // Code 3 is no state: nothing cares.
        assert_eq!(String::from_utf8(d.values[0].clone()).unwrap(), "000101xx", "S2 on 1 has no transition: it stays");
        let jk = logic_table(&m, FlipFlop::Jk);
        assert_eq!(jk.outputs.join(" "), "J1 K1 J0 K0 Z");
        // From S2 (10) on 0 to S0 (00): J1 x, K1 1.
        assert_eq!((jk.values[0][4], jk.values[1][4]), (b'x', b'1'));
    }

    #[test]
    fn a_machine_built_with_either_flip_flop_passes_its_own_run() {
        for (text, ff) in [(TWO_ONES, FlipFlop::D), (TWO_ONES, FlipFlop::Jk), (CHANGE, FlipFlop::D), (CHANGE, FlipFlop::Jk)] {
            let m = parse(text).unwrap();
            let mut p = Project::new();
            build(&m, ff, "seq", &mut p).unwrap();
            assert!(p.circuits.contains_key("seq_logic"));
            let built = p.build("seq").unwrap_or_else(|e| panic!("{ff:?}: {e}"));
            let outcomes = crate::testlang::run_all(&built);
            assert_eq!(outcomes.len(), 1);
            assert!(outcomes[0].passed() && outcomes[0].rows > 8, "{ff:?} {text}: {:?} {:?}\n{}", outcomes[0].failures, outcomes[0].error, circ::write(&p.circuits["seq"]));
            assert!(build(&m, ff, "seq", &mut p).unwrap_err().contains("already"));
        }
    }
}
