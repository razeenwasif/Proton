//! The simulator: a netlist of parts joined by nets, run as Digital runs
//! one.
//!
//! - **A step is a unit of delay.** When nets change, every part reading one
//!   of them reads its inputs; then all their outputs change at once, and
//!   the nets those drive are worked out again. That is one step; a change
//!   runs step after step until nothing changes.
//! - **Power-on settles one part at a time** in a seeded random order, so a
//!   latch made of two gates comes to one state or the other rather than
//!   swinging forever, and a Reset part stays low until it is done.
//! - **Oscillation is found, not hung on:** a change not settled within a
//!   number of steps stops the run and names the parts still changing.
//! - **Two drivers disagreeing is a conflict:** the run stops and names the
//!   net and its drivers.

use crate::parts::{eval, is_clocked, Kind, State};
use crate::value::{resolve, Value};

pub type NetId = usize;
pub type PartId = usize;

/// Steps a change may take before it is called an oscillation.
pub const MOST_STEPS: usize = 10_000;

#[derive(Debug, Clone)]
struct Net {
    bits: u8,
    value: Value,
    /// Which outputs drive it: (part, output number).
    drivers: Vec<(PartId, usize)>,
    readers: Vec<PartId>,
    /// A pull-up (true) or pull-down (false) on it.
    pull: Option<bool>,
    name: String,
}

#[derive(Debug, Clone)]
struct Part {
    kind: Kind,
    name: String,
    ins: Vec<NetId>,
    outs: Vec<NetId>,
    state: State,
    out_vals: Vec<Value>,
}

/// Why a run stopped short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    /// Two drivers of a net disagree.
    Conflict { net: String, drivers: Vec<String> },
    /// Still changing after `MOST_STEPS` steps: the parts still at it.
    Oscillation { parts: Vec<String> },
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimError::Conflict { net, drivers } => write!(f, "the net {net} is driven two ways at once, by {}", drivers.join(" and ")),
            SimError::Oscillation { parts } => write!(f, "the circuit does not settle: {} keep changing", parts.join(", ")),
        }
    }
}

/// A netlist being built: nets of widths, parts joined to them.
#[derive(Debug, Default, Clone)]
pub struct Netlist {
    nets: Vec<Net>,
    parts: Vec<Part>,
}

impl Netlist {
    pub fn net(&mut self, bits: u8, name: &str) -> NetId {
        self.nets.push(Net { bits, value: Value::high_z(bits), drivers: Vec::new(), readers: Vec::new(), pull: None, name: name.into() });
        self.nets.len() - 1
    }

    pub fn pull(&mut self, net: NetId, up: bool) {
        self.nets[net].pull = Some(up);
    }

    pub fn part(&mut self, kind: Kind, name: &str, ins: &[NetId], outs: &[NetId]) -> PartId {
        let id = self.parts.len();
        for (i, o) in outs.iter().enumerate() {
            self.nets[*o].drivers.push((id, i));
        }
        for n in ins {
            if !self.nets[*n].readers.contains(&id) {
                self.nets[*n].readers.push(id);
            }
        }
        let out_vals = outs.iter().map(|o| Value::high_z(self.nets[*o].bits)).collect();
        let mut state = State::default();
        if let Kind::Rom { .. } = &kind {
            state.mem.clear();
        }
        self.parts.push(Part { kind, name: name.into(), ins: ins.to_vec(), outs: outs.to_vec(), state, out_vals });
        id
    }

    /// A memory part's contents before it runs: a RAM's first words.
    pub fn preload(&mut self, part: PartId, words: Vec<u64>) {
        self.parts[part].state.mem = words;
    }

    /// A net, as written out: its width, its name, its pull if any.
    pub fn net_info(&self, net: NetId) -> (u8, &str, Option<bool>) {
        let n = &self.nets[net];
        (n.bits, &n.name, n.pull)
    }

    /// A part, as written out: what it is, its name, the nets it reads and
    /// drives, and the memory it starts with.
    pub fn part_info(&self, part: PartId) -> (&Kind, &str, &[NetId], &[NetId], &[u64]) {
        let p = &self.parts[part];
        (&p.kind, &p.name, &p.ins, &p.outs, &p.state.mem)
    }

    pub fn nets(&self) -> usize {
        self.nets.len()
    }

    pub fn parts(&self) -> usize {
        self.parts.len()
    }
}

/// A netlist running.
#[derive(Debug, Clone)]
pub struct Sim {
    nets: Vec<Net>,
    parts: Vec<Part>,
    settled: bool,
    /// Steps the last change took.
    pub steps: usize,
    seed: u64,
    /// Parts waiting for the next gate delay, when stepping by hand.
    waiting: Vec<PartId>,
}

impl Sim {
    /// The netlist, settled from power-on; or why it would not settle.
    pub fn new(net: Netlist, seed: u64) -> Result<Sim, SimError> {
        let mut sim = Sim { nets: net.nets, parts: net.parts, settled: false, steps: 0, seed: seed | 1, waiting: Vec::new() };
        sim.power_on()?;
        Ok(sim)
    }

    fn rand(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn inputs(&self, p: PartId) -> Vec<Value> {
        self.parts[p].ins.iter().map(|n| self.nets[*n].value).collect()
    }

    fn compute(&mut self, p: PartId) -> Vec<Value> {
        let ins = self.inputs(p);
        let outs: Vec<u8> = self.parts[p].outs.iter().map(|n| self.nets[*n].bits).collect();
        let settled = self.settled;
        let part = &mut self.parts[p];
        eval(&part.kind, &mut part.state, &ins, &outs, settled)
    }

    /// A net worked out again from its drivers; whether it changed.
    fn refresh(&mut self, n: NetId) -> Result<bool, SimError> {
        let net = &self.nets[n];
        let vals: Vec<Value> = net.drivers.iter().map(|(p, o)| self.parts[*p].out_vals.get(*o).copied().unwrap_or(Value::high_z(net.bits))).collect();
        match resolve(net.bits, &vals, net.pull) {
            Ok(v) => {
                let changed = v != net.value;
                self.nets[n].value = v;
                Ok(changed)
            }
            Err(_) => Err(SimError::Conflict {
                net: net.name.clone(),
                drivers: net.drivers.iter().map(|(p, _)| self.parts[*p].name.clone()).collect(),
            }),
        }
    }

    fn set_outputs(&mut self, p: PartId, vals: Vec<Value>) -> Vec<NetId> {
        let part = &mut self.parts[p];
        let mut touched = Vec::new();
        for (i, v) in vals.into_iter().enumerate() {
            if part.out_vals.get(i) != Some(&v) {
                if let Some(slot) = part.out_vals.get_mut(i) {
                    *slot = v;
                    touched.push(part.outs[i]);
                }
            }
        }
        touched
    }

    /// Every part worked out one at a time, in a random order seeded so a
    /// run is the same each time, until a whole pass changes nothing.
    fn power_on(&mut self) -> Result<(), SimError> {
        let mut order: Vec<PartId> = (0..self.parts.len()).collect();
        for pass in 0..MOST_STEPS / 10 {
            for i in (1..order.len()).rev() {
                let j = (self.rand() % (i as u64 + 1)) as usize;
                order.swap(i, j);
            }
            let mut changed = Vec::new();
            for &p in &order {
                let vals = self.compute(p);
                for n in self.set_outputs(p, vals) {
                    if self.refresh(n)? {
                        changed.push(p);
                    }
                }
            }
            if changed.is_empty() {
                self.settled = true;
                // The Reset parts go high now that it is done.
                let resets: Vec<PartId> = (0..self.parts.len()).filter(|p| matches!(self.parts[*p].kind, Kind::Reset)).collect();
                if !resets.is_empty() {
                    self.run_from(&resets)?;
                }
                return Ok(());
            }
            if pass + 1 == MOST_STEPS / 10 {
                return Err(SimError::Oscillation { parts: self.names(&changed) });
            }
        }
        Ok(())
    }

    fn names(&self, parts: &[PartId]) -> Vec<String> {
        let mut out: Vec<String> = parts.iter().map(|p| self.parts[*p].name.clone()).collect();
        out.sort();
        out.dedup();
        out.truncate(20);
        out
    }

    /// Steps from the parts given until nothing changes.
    fn run_from(&mut self, start: &[PartId]) -> Result<usize, SimError> {
        let mut pending: Vec<PartId> = start.to_vec();
        let mut steps = 0;
        while !pending.is_empty() {
            steps += 1;
            if steps > MOST_STEPS {
                return Err(SimError::Oscillation { parts: self.names(&pending) });
            }
            pending = self.delay(&pending)?;
        }
        self.steps = steps;
        Ok(steps)
    }

    /// One unit of delay: the parts given read their inputs, then all write
    /// at once. The parts whose inputs that changed, to go next.
    fn delay(&mut self, pending: &[PartId]) -> Result<Vec<PartId>, SimError> {
        let results: Vec<(PartId, Vec<Value>)> = pending.iter().map(|&p| (p, self.compute(p))).collect();
        let mut touched = Vec::new();
        for (p, vals) in results {
            touched.extend(self.set_outputs(p, vals));
        }
        touched.sort_unstable();
        touched.dedup();
        let mut next = Vec::new();
        let mut seen = vec![false; self.parts.len()];
        for n in touched {
            if self.refresh(n)? {
                for &r in &self.nets[n].readers {
                    // A part that drives nothing — an output, a Break — is
                    // read where it is; it has no delay to wait for.
                    if !seen[r] && !self.parts[r].outs.is_empty() {
                        seen[r] = true;
                        next.push(r);
                    }
                }
            }
        }
        Ok(next)
    }

    /// An input set without running anything: the parts it reaches wait,
    /// for [`Sim::step`] to take one gate delay at a time.
    pub fn hold(&mut self, part: PartId, value: u64) {
        self.parts[part].state.q = value;
        if !self.waiting.contains(&part) {
            self.waiting.push(part);
        }
    }

    /// One gate delay of what is waiting; the parts that worked, by name.
    pub fn step(&mut self) -> Result<Vec<String>, SimError> {
        let now = std::mem::take(&mut self.waiting);
        let names = self.names(&now);
        self.waiting = self.delay(&now)?;
        Ok(names)
    }

    /// How many parts wait for the next gate delay.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// What is waiting worked through until it settles.
    pub fn settle(&mut self) -> Result<usize, SimError> {
        let now = std::mem::take(&mut self.waiting);
        self.run_from(&now)
    }

    /// An input or clock set to a value, and the circuit run until it settles.
    pub fn set(&mut self, part: PartId, value: u64) -> Result<usize, SimError> {
        self.parts[part].state.q = value;
        let mut start = std::mem::take(&mut self.waiting);
        if !start.contains(&part) {
            start.push(part);
        }
        self.run_from(&start)
    }

    /// A clock's full cycle: up, settled, down, settled.
    pub fn tick(&mut self, clock: PartId) -> Result<(), SimError> {
        self.set(clock, 1)?;
        self.set(clock, 0)?;
        Ok(())
    }

    pub fn value(&self, net: NetId) -> Value {
        self.nets[net].value
    }

    /// What a part puts out on its output `i`: an input's value, a
    /// register's.
    pub fn output(&self, part: PartId, i: usize) -> Value {
        self.parts[part].out_vals.get(i).copied().unwrap_or(Value::high_z(1))
    }

    /// What a part reads on its first input: an output's value.
    pub fn shown(&self, part: PartId) -> Value {
        self.parts[part].ins.first().map_or(Value::high_z(1), |n| self.nets[*n].value)
    }

    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// A memory part's contents, for a RAM to be read back.
    pub fn memory(&self, part: PartId) -> &[u64] {
        &self.parts[part].state.mem
    }

    /// Words written into a memory part from address 0, and what it reads
    /// out run again.
    pub fn load(&mut self, part: PartId, words: &[u64]) -> Result<usize, SimError> {
        let mem = &mut self.parts[part].state.mem;
        if mem.len() < words.len() {
            mem.resize(words.len(), 0);
        }
        mem[..words.len()].copy_from_slice(words);
        self.run_from(&[part])
    }

    /// The nets whose names end in `name`, for looking inside a circuit.
    pub fn find(&self, name: &str) -> Vec<(NetId, &str)> {
        self.nets.iter().enumerate().filter(|(_, n)| n.name.ends_with(name)).map(|(i, n)| (i, n.name.as_str())).collect()
    }

    /// How many parts keep state: the clocked ones.
    pub fn clocked(&self) -> usize {
        self.parts.iter().filter(|p| is_clocked(&p.kind)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_adder_adds() {
        let mut n = Netlist::default();
        let (a, b, s, c) = (n.net(1, "a"), n.net(1, "b"), n.net(1, "s"), n.net(1, "c"));
        let ia = n.part(Kind::Input, "A", &[], &[a]);
        let ib = n.part(Kind::Input, "B", &[], &[b]);
        n.part(Kind::Xor, "x", &[a, b], &[s]);
        n.part(Kind::And, "y", &[a, b], &[c]);
        let mut sim = Sim::new(n, 7).unwrap();
        for (x, y) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            sim.set(ia, x).unwrap();
            sim.set(ib, y).unwrap();
            assert_eq!((sim.value(s).get(), sim.value(c).get()), (x ^ y, x & y));
        }
    }

    #[test]
    fn a_change_can_be_stepped_one_gate_delay_at_a_time() {
        // An input through three inverters: the change takes four delays
        // to come out, the input's own and one a gate.
        let mut n = Netlist::default();
        let nets: Vec<NetId> = (0..4).map(|i| n.net(1, &format!("n{i}"))).collect();
        let i = n.part(Kind::Input, "I", &[], &[nets[0]]);
        for k in 0..3 {
            n.part(Kind::Not, &format!("U{k}"), &[nets[k]], &[nets[k + 1]]);
        }
        let mut sim = Sim::new(n, 3).unwrap();
        assert_eq!(sim.value(nets[3]).get(), 1);
        sim.hold(i, 1);
        let mut names = Vec::new();
        while sim.waiting() > 0 {
            names.push(sim.step().unwrap().join(","));
        }
        assert_eq!(names, vec!["I", "U0", "U1", "U2"]);
        assert_eq!(sim.value(nets[3]).get(), 0);
        // Held and then set: what waited is run with it.
        sim.hold(i, 0);
        sim.step().unwrap();
        sim.settle().unwrap();
        assert_eq!((sim.waiting(), sim.value(nets[3]).get()), (0, 1));
    }

    #[test]
    fn a_latch_of_two_nor_gates_settles_and_holds() {
        // S and R into cross-coupled NORs: power-on settles it one way, and
        // it then remembers what was set.
        let mut n = Netlist::default();
        let (s, r, q, nq) = (n.net(1, "S"), n.net(1, "R"), n.net(1, "Q"), n.net(1, "!Q"));
        let is = n.part(Kind::Input, "S", &[], &[s]);
        let ir = n.part(Kind::Input, "R", &[], &[r]);
        n.part(Kind::Nor, "top", &[r, nq], &[q]);
        n.part(Kind::Nor, "bottom", &[s, q], &[nq]);
        let mut sim = Sim::new(n, 3).unwrap();
        assert_ne!(sim.value(q).get(), sim.value(nq).get(), "settled to one state");
        sim.set(is, 1).unwrap();
        sim.set(is, 0).unwrap();
        assert_eq!((sim.value(q).get(), sim.value(nq).get()), (1, 0), "set, and held");
        sim.set(ir, 1).unwrap();
        sim.set(ir, 0).unwrap();
        assert_eq!((sim.value(q).get(), sim.value(nq).get()), (0, 1), "reset, and held");
    }

    #[test]
    fn a_ring_of_odd_inverters_is_named_as_oscillating() {
        let mut n = Netlist::default();
        let nets: Vec<NetId> = (0..3).map(|i| n.net(1, &format!("n{i}"))).collect();
        for i in 0..3 {
            n.part(Kind::Not, &format!("inv{i}"), &[nets[i]], &[nets[(i + 1) % 3]]);
        }
        match Sim::new(n, 1) {
            Err(SimError::Oscillation { parts }) => assert!(parts.iter().any(|p| p.starts_with("inv")), "{parts:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn two_drivers_disagreeing_are_a_conflict_and_tri_states_share_a_bus() {
        let mut n = Netlist::default();
        let (a, en1, en2, bus) = (n.net(4, "a"), n.net(1, "en1"), n.net(1, "en2"), n.net(4, "bus"));
        let k = n.net(4, "k");
        n.part(Kind::Const(5), "five", &[], &[a]);
        n.part(Kind::Const(9), "nine", &[], &[k]);
        let e1 = n.part(Kind::Input, "en1", &[], &[en1]);
        let e2 = n.part(Kind::Input, "en2", &[], &[en2]);
        n.part(Kind::Driver { inverted: false }, "d1", &[a, en1], &[bus]);
        n.part(Kind::Driver { inverted: false }, "d2", &[k, en2], &[bus]);
        let mut sim = Sim::new(n, 1).unwrap();
        assert!(sim.value(bus).is_z());
        sim.set(e1, 1).unwrap();
        assert_eq!(sim.value(bus).get(), 5);
        sim.set(e1, 0).unwrap();
        sim.set(e2, 1).unwrap();
        assert_eq!(sim.value(bus).get(), 9);
        match sim.set(e1, 1) {
            Err(SimError::Conflict { net, drivers }) => assert_eq!((net.as_str(), drivers), ("bus", vec!["d1".to_string(), "d2".to_string()])),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_counter_of_t_flip_flops_counts_on_the_clock() {
        let mut n = Netlist::default();
        let (one, clk) = (n.net(1, "1"), n.net(1, "clk"));
        n.part(Kind::Const(1), "one", &[], &[one]);
        let c = n.part(Kind::Clock, "clk", &[], &[clk]);
        let q0 = n.net(1, "q0");
        let nq0 = n.net(1, "!q0");
        let q1 = n.net(1, "q1");
        let nq1 = n.net(1, "!q1");
        n.part(Kind::TFlipFlop, "t0", &[one, clk], &[q0, nq0]);
        // A ripple counter: the next one clocked by the first's !Q.
        n.part(Kind::TFlipFlop, "t1", &[one, nq0], &[q1, nq1]);
        let mut sim = Sim::new(n, 1).unwrap();
        let count = |s: &Sim| s.value(q0).get() | s.value(q1).get() << 1;
        let mut seen = Vec::new();
        for _ in 0..5 {
            sim.tick(c).unwrap();
            seen.push(count(&sim));
        }
        assert_eq!(seen, vec![1, 2, 3, 0, 1]);
    }
}
