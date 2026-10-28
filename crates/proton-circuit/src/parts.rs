//! The parts the simulator runs, and what each does with its inputs. A
//! part's pins are numbered, inputs then outputs, in the order its kind
//! documents; where they are drawn is the drawing's business, not this.

use crate::value::{mask, Value};

/// Which way a shifter shifts, and what comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Logical,
    Arithmetic,
    Rotate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Set from outside: a switch, a number typed in. Out: the value.
    Input,
    /// Set by the clock. Out: the clock.
    Clock,
    /// Out: a fixed value — a constant, ground (0) or power (all ones).
    Const(u64),
    /// In: what is shown — an output, an LED, a probe.
    Output,
    /// In: a. Out: !a.
    Not,
    /// In: n inputs. Out: the gate of them, bit by bit.
    And,
    Or,
    Nand,
    Nor,
    Xor,
    Xnor,
    /// In: a. Out: a.
    Buffer,
    /// In: a, sel. Out: a while sel is 1 (0 if `inverted`), else high-Z.
    Driver { inverted: bool },
    /// In: sel, then 2^s data inputs. Out: the one selected.
    Mux,
    /// In: sel, a. Out: 2^s outputs, a on the one selected and 0 elsewhere.
    Demux,
    /// In: sel. Out: 2^s one-bit outputs, 1 on the one selected.
    Decoder,
    /// In: n one-bit inputs. Out: the number of the highest set, any set.
    PriorityEncoder,
    /// In: a, b, carry in. Out: sum, carry out.
    Add,
    /// In: a, b, borrow in. Out: difference, borrow out.
    Sub,
    /// In: a, b. Out: the product, twice as wide.
    Mul { signed: bool },
    /// In: a, b. Out: quotient, remainder.
    Div { signed: bool },
    /// In: a, b. Out: a > b, a = b, a < b.
    Comparator { signed: bool },
    /// In: a. Out: -a.
    Neg,
    /// In: a, shift. Out: a shifted.
    Shifter { left: bool, mode: Shift, signed_shift: bool },
    /// In: en, C, clr. Out: the count, overflow.
    Counter,
    /// In: D, C. Out: Q, !Q.
    DFlipFlop,
    /// In: T, C. Out: Q, !Q.
    TFlipFlop,
    /// In: J, C, K. Out: Q, !Q.
    JKFlipFlop,
    /// In: S, C, R. Out: Q, !Q.
    RSFlipFlop,
    /// In: D, C, en. Out: Q.
    Register,
    /// In: str, C, ld, 1A, 1Din, 2A. Out: 1D (high-Z unless ld), 2D.
    RamDual { addr_bits: u8 },
    /// In: A, sel. Out: D (high-Z unless sel).
    Rom { data: Vec<u64> },
    /// In: n one-bit inputs. Out: the table's entry for them.
    Lut { table: Vec<u64> },
    /// Bits regrouped: In: groups of `ins` widths, low bits first. Out:
    /// groups of `outs` widths, each (first bit, width) of the whole.
    Splitter { ins: Vec<u8>, outs: Vec<(u8, u8)> },
    /// In: a. Out: a sign-extended (or 0-extended) to the output's width.
    BitExtender { signed: bool },
    /// In: a, sel. Out: bit sel of a.
    BitSelector,
    /// Out: 0 while power-on settles, 1 after.
    Reset,
    /// In: a one-bit signal whose rising edge stops a quick run.
    Break,
}

/// What a part keeps between steps: the last clock it saw, and its memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub clock: bool,
    /// Whether the clock has been seen: the first look sets its level and
    /// is no edge, so nothing clocks itself at power-on.
    pub primed: bool,
    pub q: u64,
    pub mem: Vec<u64>,
}

fn word(bits: u8) -> impl Fn(u64) -> Value {
    move |v| Value::new(v, bits)
}

/// A clock input's rising edge, remembered.
fn rising(state: &mut State, c: &Value, settled: bool) -> bool {
    let now = c.bit(0);
    // While power-on settles, nets going from floating to driven are no
    // clock edges: the level is only watched.
    let up = settled && state.primed && now && !state.clock;
    state.primed = true;
    state.clock = now;
    up
}

/// What a part puts out, given its inputs and the widths of its outputs.
pub fn eval(kind: &Kind, state: &mut State, ins: &[Value], outs: &[u8], settled: bool) -> Vec<Value> {
    let ob = |i: usize| outs.get(i).copied().unwrap_or(1);
    let w = word(ob(0));
    let all = |i: usize| ins.get(i).map_or(0, Value::get);
    let gate = |f: fn(u64, u64) -> u64, start: u64, invert: bool| {
        let m = mask(ob(0));
        let mut acc = start & m;
        for v in ins {
            acc = f(acc, v.get());
        }
        vec![Value::new(if invert { !acc } else { acc }, ob(0))]
    };
    match kind {
        Kind::Input | Kind::Clock => vec![Value::new(state.q, ob(0))],
        Kind::Const(v) => vec![w(*v)],
        Kind::Output | Kind::Break => Vec::new(),
        Kind::Not => vec![w(!all(0))],
        Kind::And => gate(|a, b| a & b, u64::MAX, false),
        Kind::Nand => gate(|a, b| a & b, u64::MAX, true),
        Kind::Or => gate(|a, b| a | b, 0, false),
        Kind::Nor => gate(|a, b| a | b, 0, true),
        Kind::Xor => gate(|a, b| a ^ b, 0, false),
        Kind::Xnor => gate(|a, b| a ^ b, 0, true),
        Kind::Buffer => vec![w(all(0))],
        Kind::Driver { inverted } => {
            let on = ins.get(1).is_some_and(|s| s.bit(0)) != *inverted;
            vec![if on { w(all(0)) } else { Value::high_z(ob(0)) }]
        }
        Kind::Mux => vec![ins.get(1 + all(0) as usize).map_or(Value::high_z(ob(0)), |v| w(v.get()))],
        Kind::Demux => {
            let sel = all(0) as usize;
            (0..outs.len()).map(|i| word(ob(i))(if i == sel { all(1) } else { 0 })).collect()
        }
        Kind::Decoder => {
            let sel = all(0) as usize;
            (0..outs.len()).map(|i| Value::new((i == sel) as u64, 1)).collect()
        }
        Kind::PriorityEncoder => {
            let top = (0..ins.len()).rev().find(|i| ins[*i].bit(0));
            vec![w(top.unwrap_or(0) as u64), Value::new(top.is_some() as u64, 1)]
        }
        Kind::Add => {
            let b = ob(0);
            let s = all(0) as u128 + all(1) as u128 + (all(2) & 1) as u128;
            vec![w(s as u64), Value::new(((s >> b) & 1) as u64, 1)]
        }
        Kind::Sub => {
            let b = ob(0);
            let d = (all(0) as i128) - (all(1) as i128) - ((all(2) & 1) as i128);
            vec![w(d as u64), Value::new((d < 0 || (d >> b) & 1 == 1) as u64, 1)]
        }
        Kind::Mul { signed } => {
            let p = if *signed { (ins[0].signed() as i128 * ins[1].signed() as i128) as u128 } else { all(0) as u128 * all(1) as u128 };
            vec![w(p as u64)]
        }
        Kind::Div { signed } => {
            let (a, b) = (all(0), all(1));
            if b == 0 {
                return vec![w(0), word(ob(1))(0)];
            }
            if *signed {
                let (x, y) = (ins[0].signed(), ins[1].signed());
                vec![w(x.wrapping_div(y) as u64), word(ob(1))(x.wrapping_rem(y) as u64)]
            } else {
                vec![w(a / b), word(ob(1))(a % b)]
            }
        }
        Kind::Comparator { signed } => {
            let o = if *signed { ins[0].signed().cmp(&ins[1].signed()) } else { all(0).cmp(&all(1)) };
            let one = |b: bool| Value::new(b as u64, 1);
            vec![one(o.is_gt()), one(o.is_eq()), one(o.is_lt())]
        }
        Kind::Neg => vec![w(all(0).wrapping_neg())],
        Kind::Shifter { left, mode, signed_shift } => {
            let b = ob(0) as u32;
            let s = if *signed_shift && ins.get(1).is_some_and(|v| v.signed() < 0) { -ins[1].signed() } else { ins.get(1).map_or(0, |v| v.get() as i64) };
            let (left, s) = if *signed_shift && ins.get(1).is_some_and(|v| v.signed() < 0) { (!*left, s as u32) } else { (*left, s as u32) };
            let a = all(0) & mask(b as u8);
            let m = mask(b as u8);
            let r = match (mode, left) {
                (Shift::Rotate, _) => {
                    let s = s % b.max(1);
                    if s == 0 { a } else if left { ((a << s) | (a >> (b - s))) & m } else { ((a >> s) | (a << (b - s))) & m }
                }
                (_, true) => if s >= b { 0 } else { (a << s) & m },
                (Shift::Arithmetic, false) => {
                    let x = ins[0].signed();
                    ((if s >= 64 { x >> 63 } else { x >> s.min(63) }) as u64) & m
                }
                (_, false) => if s >= b { 0 } else { a >> s },
            };
            vec![w(r)]
        }
        Kind::Counter => {
            let (en, c, clr) = (ins.first().is_some_and(|v| v.bit(0)), ins.get(1).copied().unwrap_or(Value::new(0, 1)), ins.get(2).is_some_and(|v| v.bit(0)));
            if rising(state, &c, settled) {
                if clr {
                    state.q = 0;
                } else if en {
                    state.q = (state.q + 1) & mask(ob(0));
                }
            }
            let ovf = en && state.q == mask(ob(0));
            vec![w(state.q), Value::new(ovf as u64, 1)]
        }
        Kind::DFlipFlop | Kind::TFlipFlop | Kind::JKFlipFlop | Kind::RSFlipFlop | Kind::Register => {
            let c = ins.get(1).copied().unwrap_or(Value::new(0, 1));
            if rising(state, &c, settled) {
                state.q = match kind {
                    Kind::DFlipFlop => all(0),
                    Kind::Register if ins.get(2).is_none_or(|e| e.bit(0)) => all(0),
                    Kind::Register => state.q,
                    Kind::TFlipFlop if ins[0].bit(0) || ins[0].z == 1 => state.q ^ 1,
                    Kind::TFlipFlop => state.q,
                    Kind::JKFlipFlop => match (ins[0].bit(0), ins.get(2).is_some_and(|k| k.bit(0))) {
                        (false, false) => state.q,
                        (true, false) => 1,
                        (false, true) => 0,
                        (true, true) => state.q ^ 1,
                    },
                    _ => match (ins[0].bit(0), ins.get(2).is_some_and(|r| r.bit(0))) {
                        (true, false) => 1,
                        (false, true) => 0,
                        _ => state.q,
                    },
                } & mask(ob(0));
            }
            if matches!(kind, Kind::Register) {
                vec![w(state.q)]
            } else {
                vec![w(state.q), w(!state.q)]
            }
        }
        Kind::RamDual { addr_bits } => {
            let size = 1usize << (*addr_bits).min(24);
            if state.mem.len() != size {
                state.mem = vec![0; size];
            }
            let str_ = ins[0].bit(0);
            let ld = ins[2].bit(0);
            let a1 = (all(3) as usize) & (size - 1);
            if rising(state, &ins[1], settled) && str_ {
                state.mem[a1] = all(4) & mask(ob(0));
            }
            let a2 = (all(5) as usize) & (size - 1);
            vec![if ld { w(state.mem[a1]) } else { Value::high_z(ob(0)) }, word(ob(1))(state.mem[a2])]
        }
        Kind::Rom { data } => {
            let sel = ins.get(1).is_none_or(|s| s.bit(0));
            let v = data.get(all(0) as usize).copied().unwrap_or(0);
            vec![if sel { w(v) } else { Value::high_z(ob(0)) }]
        }
        Kind::Lut { table } => {
            let mut idx = 0usize;
            for (i, v) in ins.iter().enumerate() {
                idx |= (v.bit(0) as usize) << i;
            }
            vec![w(table.get(idx).copied().unwrap_or(0))]
        }
        Kind::Splitter { ins: widths, outs: groups } => {
            let mut bus = 0u128;
            let mut z = 0u128;
            let mut at = 0u32;
            for (i, wd) in widths.iter().enumerate() {
                let v = ins.get(i).copied().unwrap_or(Value::high_z(*wd));
                bus |= ((v.v & mask(*wd)) as u128) << at;
                z |= ((v.z & mask(*wd)) as u128) << at;
                at += *wd as u32;
            }
            groups
                .iter()
                .map(|(lo, wd)| {
                    let m = mask(*wd) as u128;
                    Value { v: ((bus >> lo) & m) as u64, z: ((z >> lo) & m) as u64, bits: *wd }
                })
                .collect()
        }
        Kind::BitExtender { signed } => vec![if *signed { w(ins[0].signed() as u64) } else { w(all(0)) }],
        Kind::BitSelector => vec![Value::new(all(0) >> all(1).min(63) & 1, 1)],
        Kind::Reset => vec![Value::new(settled as u64, 1)],
    }
}

/// Whether a part keeps state across steps, so the settling at power-on
/// and the step counting treat it as a memory, not a gate.
pub fn is_clocked(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Counter | Kind::DFlipFlop | Kind::TFlipFlop | Kind::JKFlipFlop | Kind::RSFlipFlop | Kind::Register | Kind::RamDual { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: Kind, ins: &[(u64, u8)], outs: &[u8]) -> Vec<u64> {
        let ins: Vec<Value> = ins.iter().map(|(v, b)| Value::new(*v, *b)).collect();
        eval(&kind, &mut State::default(), &ins, outs, true).iter().map(|v| v.get()).collect()
    }

    #[test]
    fn each_part_does_what_its_table_says() {
        for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let ab = [(a, 1), (b, 1)];
            assert_eq!(run(Kind::And, &ab, &[1]), vec![a & b]);
            assert_eq!(run(Kind::Or, &ab, &[1]), vec![a | b]);
            assert_eq!(run(Kind::Nand, &ab, &[1]), vec![1 - (a & b)]);
            assert_eq!(run(Kind::Nor, &ab, &[1]), vec![1 - (a | b)]);
            assert_eq!(run(Kind::Xor, &ab, &[1]), vec![a ^ b]);
            assert_eq!(run(Kind::Xnor, &ab, &[1]), vec![1 - (a ^ b)]);
        }
        assert_eq!(run(Kind::And, &[(0b1100, 4), (0b1010, 4), (0b1111, 4)], &[4]), vec![0b1000], "wide and many inputs");
        assert_eq!(run(Kind::Not, &[(0b0101, 4)], &[4]), vec![0b1010]);
        assert_eq!(run(Kind::Mux, &[(2, 2), (10, 8), (11, 8), (12, 8), (13, 8)], &[8]), vec![12]);
        assert_eq!(run(Kind::Demux, &[(1, 1), (7, 4)], &[4, 4]), vec![0, 7]);
        assert_eq!(run(Kind::Decoder, &[(2, 2)], &[1, 1, 1, 1]), vec![0, 0, 1, 0]);
        assert_eq!(run(Kind::PriorityEncoder, &[(1, 1), (0, 1), (1, 1), (0, 1)], &[2, 1]), vec![2, 1]);
        assert_eq!(run(Kind::Add, &[(0xffff, 16), (1, 16), (0, 1)], &[16, 1]), vec![0, 1]);
        assert_eq!(run(Kind::Sub, &[(3, 8), (5, 8), (0, 1)], &[8, 1]), vec![254, 1]);
        assert_eq!(run(Kind::Mul { signed: false }, &[(200, 8), (200, 8)], &[16]), vec![40000]);
        assert_eq!(run(Kind::Div { signed: true }, &[(0xf9, 8), (2, 8)], &[8, 8]), vec![0xfd, 0xff], "-7 / 2 = -3 remainder -1");
        assert_eq!(run(Kind::Comparator { signed: true }, &[(0xff, 8), (1, 8)], &[1, 1, 1]), vec![0, 0, 1]);
        assert_eq!(run(Kind::Comparator { signed: false }, &[(0xff, 8), (1, 8)], &[1, 1, 1]), vec![1, 0, 0]);
        assert_eq!(run(Kind::Neg, &[(1, 8)], &[8]), vec![0xff]);
        let sh = |left, mode, a, s| run(Kind::Shifter { left, mode, signed_shift: false }, &[(a, 8), (s, 3)], &[8])[0];
        assert_eq!(sh(true, Shift::Logical, 0b1001_0001, 2), 0b0100_0100);
        assert_eq!(sh(false, Shift::Arithmetic, 0b1000_0000, 3), 0b1111_0000);
        assert_eq!(sh(false, Shift::Rotate, 0b0000_0011, 1), 0b1000_0001);
        assert_eq!(run(Kind::Splitter { ins: vec![4, 4], outs: vec![(0, 8), (2, 4)] }, &[(0xa, 4), (0x5, 4)], &[8, 4]), vec![0x5a, 0x6]);
        assert_eq!(run(Kind::BitExtender { signed: true }, &[(0b100, 3)], &[8]), vec![0xfc]);
        assert_eq!(run(Kind::BitSelector, &[(0b100, 3), (2, 2)], &[1]), vec![1]);
    }

    #[test]
    fn flip_flops_take_their_input_on_a_rising_edge_only() {
        let mut s = State::default();
        let v = |x| Value::new(x, 1);
        let mut d = |dv, c| eval(&Kind::DFlipFlop, &mut s, &[v(dv), v(c)], &[1, 1], true)[0].get();
        assert_eq!(d(1, 0), 0);
        assert_eq!(d(1, 1), 1, "rising");
        assert_eq!(d(0, 1), 1, "held high: no edge");
        assert_eq!(d(0, 0), 1);
        let mut s = State::default();
        let mut jk = |j, c, k| eval(&Kind::JKFlipFlop, &mut s, &[v(j), v(c), v(k)], &[1, 1], true)[0].get();
        jk(1, 0, 1);
        assert_eq!(jk(1, 1, 1), 1, "toggles");
        jk(1, 0, 1);
        assert_eq!(jk(1, 1, 1), 0, "toggles back");
        let mut s = State::default();
        let w = |x| Value::new(x, 4);
        let mut ram = |str_, c, ld, a1, din, a2| eval(&Kind::RamDual { addr_bits: 2 }, &mut s, &[v(str_), v(c), v(ld), Value::new(a1, 2), w(din), Value::new(a2, 2)], &[4, 4], true);
        ram(1, 0, 0, 2, 9, 0);
        ram(1, 1, 0, 2, 9, 0);
        let out = ram(0, 0, 1, 2, 0, 2);
        assert_eq!((out[0].get(), out[1].get()), (9, 9));
        assert!(ram(0, 0, 0, 2, 0, 2)[0].is_z(), "port 1 is high-Z unless ld");
    }
}
