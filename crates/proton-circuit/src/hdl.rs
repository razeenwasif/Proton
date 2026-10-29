//! Verilog and VHDL of a built circuit, for a real FPGA toolchain.
//!
//! Written from the netlist the simulator runs, so every part it knows is
//! written and a subcircuit is its parts, inline: one module (one entity)
//! a circuit, its ports the top circuit's inputs, clocks and outputs. A net
//! is a wire (a signal) `n<k>`; a net several parts drive is resolved as the
//! language resolves it, high-Z included; a pulled net is `tri1`/`tri0` (a
//! weak `'H'`/`'L'` driver). Flip-flops, registers, counters and memories
//! start at 0, as the simulator's do, and change on a rising edge.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::dig::Built;
use crate::parts::{Kind, Shift};
use crate::sim::{NetId, Netlist, PartId};

const VERILOG_WORDS: &[&str] = &[
    "always", "and", "assign", "begin", "buf", "case", "default", "else", "end", "endcase", "endmodule", "for", "if", "initial", "inout", "input", "integer", "module",
    "nand", "nor", "not", "or", "output", "posedge", "negedge", "reg", "tri", "tri0", "tri1", "wire", "xnor", "xor",
];
const VHDL_WORDS: &[&str] = &[
    "abs", "and", "architecture", "array", "begin", "case", "downto", "else", "elsif", "end", "entity", "for", "if", "in", "is", "library", "loop", "mod", "nand", "nor",
    "not", "of", "or", "others", "out", "port", "process", "rem", "signal", "then", "to", "type", "use", "when", "with", "xnor", "xor",
];

/// A name either language takes: letters, digits and `_`, a letter first,
/// no keyword, no `_` doubled or last (VHDL's rules, the stricter).
pub fn ident(name: &str, words: &[&str]) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    let s = if s.is_empty() || !s.starts_with(|c: char| c.is_ascii_alphabetic()) { format!("p_{s}") } else { s };
    if words.contains(&s.to_ascii_lowercase().as_str()) { format!("{s}_p") } else { s }
}

/// A port: its label, its width, and the part behind it.
type Port = (String, u8, PartId);

/// The top circuit's ports: inputs (and clocks) and outputs, by label,
/// sorted.
fn ports(built: &Built) -> (Vec<Port>, Vec<Port>) {
    let ins: BTreeMap<&String, &(PartId, u8)> = built.inputs.iter().collect();
    let outs: BTreeMap<&String, &PartId> = built.outputs.iter().collect();
    let nl = &built.netlist;
    let ins = ins.into_iter().map(|(l, (p, b))| (l.clone(), *b, *p)).collect();
    let outs = outs
        .into_iter()
        .map(|(l, p)| {
            let (_, _, i, _, _) = nl.part_info(*p);
            let bits = i.first().map_or(1, |n| nl.net_info(*n).0);
            (l.clone(), bits, *p)
        })
        .collect();
    (ins, outs)
}

fn width(nl: &Netlist, n: NetId) -> u8 {
    nl.net_info(n).0
}

/* ── Verilog ──────────────────────────────────────────────────────── */

/// The circuit as a Verilog-2001 module.
pub fn verilog(built: &Built, module: &str) -> String {
    let nl = &built.netlist;
    let (ins, outs) = ports(built);
    let range = |b: u8| if b == 1 { String::new() } else { format!("[{}:0] ", b - 1) };
    let mut s = String::new();
    let _ = writeln!(s, "// Written by Proton's Circuit mode from the simulator's netlist.\n`timescale 1ns / 1ps\n");
    let mut port_lines: Vec<String> = ins.iter().map(|(l, b, _)| format!("    input wire {}{}", range(*b), ident(l, VERILOG_WORDS))).collect();
    port_lines.extend(outs.iter().map(|(l, b, _)| format!("    output wire {}{}", range(*b), ident(l, VERILOG_WORDS))));
    let _ = writeln!(s, "module {} (\n{}\n);", ident(module, VERILOG_WORDS), port_lines.join(",\n"));
    for n in 0..nl.nets() {
        let (b, name, pull) = nl.net_info(n);
        let kind = match pull {
            Some(true) => "tri1",
            Some(false) => "tri0",
            None => "wire",
        };
        let _ = writeln!(s, "    {kind} {}n{n}; // {name}", range(b));
    }
    let port_of: BTreeMap<PartId, &String> = ins.iter().map(|(l, _, p)| (*p, l)).chain(outs.iter().map(|(l, _, p)| (*p, l))).collect();
    for p in 0..nl.parts() {
        let (kind, name, i, o, mem) = nl.part_info(p);
        let n = |k: usize| format!("n{}", i[k]);
        let out = |k: usize| format!("n{}", o[k]);
        let ow = |k: usize| width(nl, o[k]);
        let z = |k: usize| format!("{{{}{{1'bz}}}}", ow(k));
        let lit = |w: u8, v: u64| format!("{w}'h{:x}", v & crate::value::mask(w));
        let _ = writeln!(s, "    // {name}");
        match kind {
            Kind::Input | Kind::Clock => {
                if let Some(l) = port_of.get(&p) {
                    let _ = writeln!(s, "    assign {} = {};", out(0), ident(l, VERILOG_WORDS));
                }
            }
            Kind::Output => {
                if let (Some(l), true) = (port_of.get(&p), !i.is_empty()) {
                    let _ = writeln!(s, "    assign {} = {};", ident(l, VERILOG_WORDS), n(0));
                }
            }
            Kind::Const(v) => {
                let _ = writeln!(s, "    assign {} = {};", out(0), lit(ow(0), *v));
            }
            Kind::Not => {
                let _ = writeln!(s, "    assign {} = ~{};", out(0), n(0));
            }
            Kind::And | Kind::Or | Kind::Xor | Kind::Nand | Kind::Nor | Kind::Xnor => {
                let op = match kind {
                    Kind::And | Kind::Nand => " & ",
                    Kind::Or | Kind::Nor => " | ",
                    _ => " ^ ",
                };
                let e = (0..i.len()).map(n).collect::<Vec<_>>().join(op);
                let inv = matches!(kind, Kind::Nand | Kind::Nor | Kind::Xnor);
                let _ = writeln!(s, "    assign {} = {}({});", out(0), if inv { "~" } else { "" }, e);
            }
            Kind::Buffer => {
                let _ = writeln!(s, "    assign {} = {};", out(0), n(0));
            }
            Kind::Driver { inverted } => {
                let _ = writeln!(s, "    assign {} = ({} == 1'b{}) ? {} : {};", out(0), n(1), if *inverted { 0 } else { 1 }, n(0), z(0));
            }
            Kind::Mux => {
                let arms: Vec<String> = (1..i.len()).map(|k| format!("({} == {}) ? {}", n(0), k - 1, n(k))).collect();
                let _ = writeln!(s, "    assign {} = {} : {};", out(0), arms.join(" : "), lit(ow(0), 0));
            }
            Kind::Demux => {
                for k in 0..o.len() {
                    let _ = writeln!(s, "    assign {} = ({} == {k}) ? {} : {};", out(k), n(0), n(1), lit(ow(k), 0));
                }
            }
            Kind::Decoder => {
                for k in 0..o.len() {
                    let _ = writeln!(s, "    assign {} = ({} == {k});", out(k), n(0));
                }
            }
            Kind::PriorityEncoder => {
                let picks: Vec<String> = (0..i.len()).rev().map(|k| format!("{} ? {}", n(k), lit(ow(0), k as u64))).collect();
                let any = (0..i.len()).map(n).collect::<Vec<_>>().join(" | ");
                let _ = writeln!(s, "    assign {} = {} : {};\n    assign {} = {any};", out(0), picks.join(" : "), lit(ow(0), 0), out(1));
            }
            Kind::Add | Kind::Sub => {
                let w = ow(0);
                let op = if matches!(kind, Kind::Add) { "+" } else { "-" };
                let _ = writeln!(s, "    wire [{w}:0] t{p} = {{1'b0, {}}} {op} {{1'b0, {}}} {op} {};", n(0), n(1), if i.len() > 2 { n(2) } else { "1'b0".into() });
                let _ = writeln!(s, "    assign {} = t{p}[{}:0];\n    assign {} = t{p}[{w}];", out(0), w - 1, out(1));
            }
            Kind::Mul { signed } => {
                let (a, b) = if *signed { (format!("$signed({})", n(0)), format!("$signed({})", n(1))) } else { (n(0), n(1)) };
                let _ = writeln!(s, "    assign {} = {a} * {b};", out(0));
            }
            Kind::Div { signed } => {
                let (a, b) = if *signed { (format!("$signed({})", n(0)), format!("$signed({})", n(1))) } else { (n(0), n(1)) };
                let _ = writeln!(s, "    assign {} = ({} == 0) ? 0 : {a} / {b};\n    assign {} = ({} == 0) ? 0 : {a} % {b};", out(0), n(1), out(1), n(1));
            }
            Kind::Comparator { signed } => {
                let (a, b) = if *signed { (format!("$signed({})", n(0)), format!("$signed({})", n(1))) } else { (n(0), n(1)) };
                let _ = writeln!(s, "    assign {} = {a} > {b};\n    assign {} = {a} == {b};\n    assign {} = {a} < {b};", out(0), out(1), out(2));
            }
            Kind::Neg => {
                let _ = writeln!(s, "    assign {} = -{};", out(0), n(0));
            }
            Kind::Shifter { left, mode, signed_shift } => {
                let w = ow(0);
                let shift = |left: bool, amount: &str| -> String {
                    match (mode, left) {
                        (Shift::Rotate, true) => format!("(({a} << ({amount} % {w})) | ({a} >> (({w} - {amount} % {w}) % {w})))", a = n(0)),
                        (Shift::Rotate, false) => format!("(({a} >> ({amount} % {w})) | ({a} << (({w} - {amount} % {w}) % {w})))", a = n(0)),
                        (_, true) => format!("({} << {amount})", n(0)),
                        (Shift::Arithmetic, false) => format!("($signed({}) >>> {amount})", n(0)),
                        (Shift::Logical, false) => format!("({} >> {amount})", n(0)),
                    }
                };
                if *signed_shift {
                    // A negative shift goes the other way.
                    let _ = writeln!(s, "    assign {} = $signed({s1}) < 0 ? {} : {};", out(0), shift(!*left, &format!("(-$signed({s1}))", s1 = n(1))), shift(*left, &n(1)), s1 = n(1));
                } else {
                    let _ = writeln!(s, "    assign {} = {};", out(0), shift(*left, &n(1)));
                }
            }
            Kind::Counter => {
                let w = ow(0);
                let _ = writeln!(s, "    reg [{}:0] q{p} = 0;\n    always @(posedge {}) if ({}) q{p} <= 0; else if ({}) q{p} <= q{p} + 1;", w - 1, n(1), n(2), n(0));
                let _ = writeln!(s, "    assign {} = q{p};\n    assign {} = {} && (q{p} == {{{w}{{1'b1}}}});", out(0), out(1), n(0));
            }
            Kind::DFlipFlop | Kind::TFlipFlop | Kind::JKFlipFlop | Kind::RSFlipFlop | Kind::Register => {
                let w = ow(0);
                let next = match kind {
                    Kind::DFlipFlop => format!("q{p} <= {};", n(0)),
                    Kind::TFlipFlop => format!("if ({} !== 1'b0) q{p} <= ~q{p};", n(0)),
                    Kind::JKFlipFlop => format!("case ({{{}, {}}}) 2'b10: q{p} <= 1'b1; 2'b01: q{p} <= 1'b0; 2'b11: q{p} <= ~q{p}; default: ; endcase", n(0), n(2)),
                    Kind::RSFlipFlop => format!("case ({{{}, {}}}) 2'b10: q{p} <= 1'b1; 2'b01: q{p} <= 1'b0; default: ; endcase", n(0), n(2)),
                    _ => format!("if ({} !== 1'b0) q{p} <= {};", if i.len() > 2 { n(2) } else { "1'b1".into() }, n(0)),
                };
                let _ = writeln!(s, "    reg [{}:0] q{p} = 0;\n    always @(posedge {}) {next}", w - 1, n(1));
                let _ = writeln!(s, "    assign {} = q{p};", out(0));
                if o.len() > 1 {
                    let _ = writeln!(s, "    assign {} = ~q{p};", out(1));
                }
            }
            Kind::RamDual { addr_bits } | Kind::RamPort { addr_bits } => {
                let w = ow(0);
                let words = 1usize << addr_bits;
                let _ = writeln!(s, "    reg [{}:0] m{p} [0:{}];", w - 1, words - 1);
                memory_init(&mut s, p, words, mem);
                if matches!(kind, Kind::RamDual { .. }) {
                    // str, C, ld, 1A, 1Din, 2A → 1D, 2D.
                    let _ = writeln!(s, "    always @(posedge {}) if ({}) m{p}[{}] <= {};", n(1), n(0), n(3), n(4));
                    let _ = writeln!(s, "    assign {} = {} ? m{p}[{}] : {};\n    assign {} = m{p}[{}];", out(0), n(2), n(3), z(0), out(1), n(5));
                } else {
                    // A, Din, str, C, ld → D.
                    let _ = writeln!(s, "    always @(posedge {}) if ({}) m{p}[{}] <= {};", n(3), n(2), n(0), n(1));
                    let _ = writeln!(s, "    assign {} = {} ? m{p}[{}] : {};", out(0), n(4), n(0), z(0));
                }
            }
            Kind::Rom { data } | Kind::Lut { table: data } => {
                let w = ow(0);
                let words = data.len().max(1);
                let _ = writeln!(s, "    reg [{}:0] m{p} [0:{}];", w - 1, words - 1);
                memory_init(&mut s, p, words, data);
                if matches!(kind, Kind::Rom { .. }) {
                    let sel = if i.len() > 1 { n(1) } else { "1'b1".into() };
                    let _ = writeln!(s, "    assign {} = {sel} ? ({} < {words} ? m{p}[{}] : 0) : {};", out(0), n(0), n(0), z(0));
                } else {
                    let idx = (0..i.len()).rev().map(n).collect::<Vec<_>>().join(", ");
                    let _ = writeln!(s, "    wire [{}:0] x{p} = {{{idx}}};\n    assign {} = x{p} < {words} ? m{p}[x{p}] : 0;", i.len().max(1) - 1, out(0));
                }
            }
            Kind::Splitter { ins: groups, outs: parts } => {
                for (k, (lo, w)) in parts.iter().enumerate() {
                    let bits: Vec<String> = (0..*w).rev().map(|j| bit_of(groups, lo + j, &|g| n(g))).collect();
                    let _ = writeln!(s, "    assign {} = {{{}}};", out(k), bits.join(", "));
                }
            }
            Kind::BitExtender { .. } => {
                let (iw, w) = (width(nl, i[0]), ow(0));
                if w > iw {
                    let _ = writeln!(s, "    assign {} = {{{{{}{{{}[{}]}}}}, {}}};", out(0), w - iw, n(0), iw - 1, n(0));
                } else {
                    let _ = writeln!(s, "    assign {} = {}[{}:0];", out(0), n(0), w - 1);
                }
            }
            Kind::BitSelector => {
                let _ = writeln!(s, "    assign {} = {} >> {};", out(0), n(0), n(1));
            }
            Kind::Reset => {
                let _ = writeln!(s, "    assign {} = 1'b1;", out(0));
            }
            Kind::Break => {}
        }
    }
    s.push_str("endmodule\n");
    s
}

/// Bit `at` of a splitter's bus: which input group holds it, and where.
fn bit_of(groups: &[(u8, u8)], at: u8, name: &dyn Fn(usize) -> String) -> String {
    for (g, (lo, w)) in groups.iter().enumerate() {
        if at >= *lo && at < lo + w {
            return format!("{}[{}]", name(g), at - lo);
        }
    }
    "1'bz".into()
}

fn memory_init(s: &mut String, p: PartId, words: usize, mem: &[u64]) {
    let _ = writeln!(s, "    integer i{p};\n    initial begin\n        for (i{p} = 0; i{p} < {words}; i{p} = i{p} + 1) m{p}[i{p}] = 0;");
    for (a, v) in mem.iter().enumerate().take(words) {
        if *v != 0 {
            let _ = writeln!(s, "        m{p}[{a}] = 'h{v:x};");
        }
    }
    s.push_str("    end\n");
}

/* ── VHDL ─────────────────────────────────────────────────────────── */

/// The circuit as a VHDL-2008 entity and architecture. Every net is a
/// `std_logic_vector`, one bit wide or more; one-bit ports are `std_logic`.
pub fn vhdl(built: &Built, entity: &str) -> String {
    let nl = &built.netlist;
    let (ins, outs) = ports(built);
    let entity = ident(entity, VHDL_WORDS);
    let ty = |b: u8| if b == 1 { "std_logic".to_string() } else { format!("std_logic_vector({} downto 0)", b - 1) };
    let vec = |b: u8| format!("std_logic_vector({} downto 0)", b - 1);
    let zeros = "(others => '0')";
    let mut decls: Vec<String> = Vec::new();
    let mut body: Vec<String> = Vec::new();
    for n in 0..nl.nets() {
        let (b, name, pull) = nl.net_info(n);
        decls.push(format!("    signal n{n} : {}; -- {name}", vec(b)));
        if let Some(up) = pull {
            body.push(format!("    n{n} <= (others => '{}');", if up { 'H' } else { 'L' }));
        }
    }
    let port_of: BTreeMap<PartId, (&String, u8)> = ins.iter().map(|(l, b, p)| (*p, (l, *b))).chain(outs.iter().map(|(l, b, p)| (*p, (l, *b)))).collect();
    let u = |e: String| format!("unsigned({e})");
    let sg = |e: String| format!("signed({e})");
    let num = |e: String| format!("to_integer(unsigned({e}))");
    for p in 0..nl.parts() {
        let (kind, name, i, o, mem) = nl.part_info(p);
        let n = |k: usize| format!("n{}", i[k]);
        let out = |k: usize| format!("n{}", o[k]);
        let ow = |k: usize| width(nl, o[k]);
        let iw = |k: usize| width(nl, i[k]);
        let lit = |w: u8, v: u64| format!("std_logic_vector(to_unsigned({}, {w}))", v & crate::value::mask(w).min(i64::MAX as u64));
        let mut b = |line: String| body.push(line);
        b(format!("    -- {name}"));
        match kind {
            Kind::Input | Kind::Clock => {
                if let Some((l, w)) = port_of.get(&p) {
                    let l = ident(l, VHDL_WORDS);
                    b(if *w == 1 { format!("    {}(0) <= {l};", out(0)) } else { format!("    {} <= {l};", out(0)) });
                }
            }
            Kind::Output => {
                if let (Some((l, w)), true) = (port_of.get(&p), !i.is_empty()) {
                    let l = ident(l, VHDL_WORDS);
                    b(if *w == 1 { format!("    {l} <= {}(0);", n(0)) } else { format!("    {l} <= {};", n(0)) });
                }
            }
            Kind::Const(v) => b(format!("    {} <= {};", out(0), lit(ow(0), *v))),
            Kind::Not => b(format!("    {} <= not {};", out(0), n(0))),
            Kind::And | Kind::Or | Kind::Xor | Kind::Nand | Kind::Nor | Kind::Xnor => {
                let op = match kind {
                    Kind::And | Kind::Nand => " and ",
                    Kind::Or | Kind::Nor => " or ",
                    _ => " xor ",
                };
                let e = (0..i.len()).map(n).collect::<Vec<_>>().join(op);
                let inv = matches!(kind, Kind::Nand | Kind::Nor | Kind::Xnor);
                b(format!("    {} <= {}({e});", out(0), if inv { "not " } else { "" }));
            }
            Kind::Buffer => b(format!("    {} <= {};", out(0), n(0))),
            Kind::Driver { inverted } => b(format!("    {} <= {} when {}(0) = '{}' else (others => 'Z');", out(0), n(0), n(1), if *inverted { 0 } else { 1 })),
            Kind::Mux => {
                let arms: Vec<String> = (1..i.len()).map(|k| format!("{} when {} = {} else", n(k), num(n(0)), k - 1)).collect();
                b(format!("    {} <= {} {zeros};", out(0), arms.join(" ")));
            }
            Kind::Demux => {
                for k in 0..o.len() {
                    b(format!("    {} <= {} when {} = {k} else {zeros};", out(k), n(1), num(n(0))));
                }
            }
            Kind::Decoder => {
                for k in 0..o.len() {
                    b(format!("    {} <= \"1\" when {} = {k} else \"0\";", out(k), num(n(0))));
                }
            }
            Kind::PriorityEncoder => {
                let picks: Vec<String> = (0..i.len()).rev().map(|k| format!("{} when {}(0) = '1' else", lit(ow(0), k as u64), n(k))).collect();
                let any = (0..i.len()).map(n).collect::<Vec<_>>().join(" or ");
                b(format!("    {} <= {} {zeros};\n    {} <= {any};", out(0), picks.join(" "), out(1)));
            }
            Kind::Add | Kind::Sub => {
                let w = ow(0) as u32;
                let op = if matches!(kind, Kind::Add) { "+" } else { "-" };
                decls.push(format!("    signal t{p} : unsigned({w} downto 0);"));
                let c = if i.len() > 2 { format!(" {op} resize({}, {})", u(n(2)), w + 1) } else { String::new() };
                b(format!("    t{p} <= resize({}, {}) {op} resize({}, {}){c};", u(n(0)), w + 1, u(n(1)), w + 1));
                b(format!("    {} <= std_logic_vector(t{p}({} downto 0));\n    {}(0) <= t{p}({w});", out(0), w - 1, out(1)));
            }
            Kind::Mul { signed } => {
                let (a, bb) = if *signed { (sg(n(0)), sg(n(1))) } else { (u(n(0)), u(n(1))) };
                b(format!("    {} <= std_logic_vector(resize({a} * {bb}, {}));", out(0), ow(0)));
            }
            Kind::Div { signed } => {
                let (a, bb) = if *signed { (sg(n(0)), sg(n(1))) } else { (u(n(0)), u(n(1))) };
                b(format!("    {} <= {zeros} when {} = 0 else std_logic_vector({a} / {bb});", out(0), u(n(1))));
                b(format!("    {} <= {zeros} when {} = 0 else std_logic_vector({a} rem {bb});", out(1), u(n(1))));
            }
            Kind::Comparator { signed } => {
                let (a, bb) = if *signed { (sg(n(0)), sg(n(1))) } else { (u(n(0)), u(n(1))) };
                for (k, op) in [">", "=", "<"].iter().enumerate() {
                    b(format!("    {} <= \"1\" when {a} {op} {bb} else \"0\";", out(k)));
                }
            }
            Kind::Neg => b(format!("    {} <= std_logic_vector(-{});", out(0), sg(n(0)))),
            Kind::Shifter { left, mode, signed_shift } => {
                let shift = |left: bool, amount: String| -> String {
                    match (mode, left) {
                        (Shift::Rotate, true) => format!("std_logic_vector(rotate_left({}, {amount}))", u(n(0))),
                        (Shift::Rotate, false) => format!("std_logic_vector(rotate_right({}, {amount}))", u(n(0))),
                        (_, true) => format!("std_logic_vector(shift_left({}, {amount}))", u(n(0))),
                        (Shift::Arithmetic, false) => format!("std_logic_vector(shift_right({}, {amount}))", sg(n(0))),
                        (Shift::Logical, false) => format!("std_logic_vector(shift_right({}, {amount}))", u(n(0))),
                    }
                };
                if *signed_shift {
                    let s1 = format!("to_integer({})", sg(n(1)));
                    b(format!("    {} <= {} when {s1} < 0 else {};", out(0), shift(!*left, format!("-{s1}")), shift(*left, s1.clone())));
                } else {
                    b(format!("    {} <= {};", out(0), shift(*left, num(n(1)))));
                }
            }
            Kind::Counter => {
                let w = ow(0);
                decls.push(format!("    signal q{p} : unsigned({} downto 0) := (others => '0');", w - 1));
                b(format!(
                    "    process ({c}) begin\n        if rising_edge({c}(0)) then\n            if {clr}(0) = '1' then q{p} <= (others => '0');\n            elsif {en}(0) = '1' then q{p} <= q{p} + 1;\n            end if;\n        end if;\n    end process;",
                    c = n(1),
                    clr = n(2),
                    en = n(0)
                ));
                b(format!("    {} <= std_logic_vector(q{p});\n    {}(0) <= '1' when {}(0) = '1' and q{p} = (q{p}'range => '1') else '0';", out(0), out(1), n(0)));
            }
            Kind::DFlipFlop | Kind::TFlipFlop | Kind::JKFlipFlop | Kind::RSFlipFlop | Kind::Register => {
                let w = ow(0);
                decls.push(format!("    signal q{p} : {} := (others => '0');", vec(w)));
                let next = match kind {
                    Kind::DFlipFlop => format!("q{p} <= {};", n(0)),
                    Kind::TFlipFlop => format!("if {}(0) /= '0' then q{p} <= not q{p}; end if;", n(0)),
                    Kind::JKFlipFlop => format!(
                        "if {j}(0) = '1' and {k}(0) = '1' then q{p} <= not q{p}; elsif {j}(0) = '1' then q{p} <= \"1\"; elsif {k}(0) = '1' then q{p} <= \"0\"; end if;",
                        j = n(0),
                        k = n(2)
                    ),
                    Kind::RSFlipFlop => format!("if {s}(0) = '1' and {r}(0) = '0' then q{p} <= \"1\"; elsif {r}(0) = '1' and {s}(0) = '0' then q{p} <= \"0\"; end if;", s = n(0), r = n(2)),
                    _ if i.len() > 2 => format!("if {}(0) /= '0' then q{p} <= {}; end if;", n(2), n(0)),
                    _ => format!("q{p} <= {};", n(0)),
                };
                b(format!("    process ({c}) begin\n        if rising_edge({c}(0)) then\n            {next}\n        end if;\n    end process;", c = n(1)));
                b(format!("    {} <= q{p};", out(0)));
                if o.len() > 1 {
                    b(format!("    {} <= not q{p};", out(1)));
                }
            }
            Kind::RamDual { addr_bits } | Kind::RamPort { addr_bits } => {
                let w = ow(0);
                let words = 1usize << addr_bits;
                decls.push(memory_decl(p, w, words, mem));
                if matches!(kind, Kind::RamDual { .. }) {
                    b(format!(
                        "    process ({c}) begin\n        if rising_edge({c}(0)) then\n            if {st}(0) = '1' then m{p}({a}) <= {d}; end if;\n        end if;\n    end process;",
                        c = n(1),
                        st = n(0),
                        a = num(n(3)),
                        d = n(4)
                    ));
                    b(format!("    {} <= m{p}({}) when {}(0) = '1' else (others => 'Z');\n    {} <= m{p}({});", out(0), num(n(3)), n(2), out(1), num(n(5))));
                } else {
                    b(format!(
                        "    process ({c}) begin\n        if rising_edge({c}(0)) then\n            if {st}(0) = '1' then m{p}({a}) <= {d}; end if;\n        end if;\n    end process;",
                        c = n(3),
                        st = n(2),
                        a = num(n(0)),
                        d = n(1)
                    ));
                    b(format!("    {} <= m{p}({}) when {}(0) = '1' else (others => 'Z');", out(0), num(n(0)), n(4)));
                }
            }
            Kind::Rom { data } | Kind::Lut { table: data } => {
                let w = ow(0);
                let words = data.len().max(1);
                decls.push(memory_decl(p, w, words, data));
                if matches!(kind, Kind::Rom { .. }) {
                    let sel = if i.len() > 1 { format!("{}(0) = '1'", n(1)) } else { "true".into() };
                    b(format!(
                        "    {o} <= (others => 'Z') when not ({sel}) else m{p}({a}) when {a} < {words} else {zeros};",
                        o = out(0),
                        a = num(n(0))
                    ));
                } else {
                    let idx = (0..i.len()).rev().map(|k| format!("{}(0)", n(k))).collect::<Vec<_>>().join(" & ");
                    decls.push(format!("    signal x{p} : std_logic_vector({} downto 0);", i.len().max(1) - 1));
                    b(format!("    x{p} <= {idx};\n    {o} <= m{p}({a}) when {a} < {words} else {zeros};", o = out(0), a = num(format!("x{p}"))));
                }
            }
            Kind::Splitter { ins: groups, outs: parts } => {
                for (k, (lo, w)) in parts.iter().enumerate() {
                    for j in 0..*w {
                        let src = bit_of(groups, lo + j, &|g| n(g)).replace('[', "(").replace(']', ")").replace("1'bz", "'Z'");
                        b(format!("    {}({j}) <= {src};", out(k)));
                    }
                }
            }
            Kind::BitExtender { .. } => b(format!("    {} <= std_logic_vector(resize({}, {}));", out(0), sg(n(0)), ow(0))),
            Kind::BitSelector => {
                b(format!("    {}(0) <= {a}({s}) when {s} < {w} else '0';", out(0), a = n(0), s = num(n(1)), w = iw(0)));
            }
            Kind::Reset => b(format!("    {} <= \"1\";", out(0))),
            Kind::Break => {}
        }
    }
    let mut port_lines: Vec<String> = ins.iter().map(|(l, b, _)| format!("        {} : in {}", ident(l, VHDL_WORDS), ty(*b))).collect();
    port_lines.extend(outs.iter().map(|(l, b, _)| format!("        {} : out {}", ident(l, VHDL_WORDS), ty(*b))));
    let ports = if port_lines.is_empty() { String::new() } else { format!("    port (\n{}\n    );\n", port_lines.join(";\n")) };
    format!(
        "-- Written by Proton's Circuit mode from the simulator's netlist.\nlibrary ieee;\nuse ieee.std_logic_1164.all;\nuse ieee.numeric_std.all;\n\nentity {entity} is\n{ports}end entity;\n\narchitecture netlist of {entity} is\n{}\nbegin\n{}\nend architecture;\n",
        decls.join("\n"),
        body.join("\n")
    )
}

fn memory_decl(p: PartId, w: u8, words: usize, mem: &[u64]) -> String {
    let mut init: Vec<String> = mem.iter().enumerate().take(words).filter(|(_, v)| **v != 0).map(|(a, v)| format!("{a} => std_logic_vector(to_unsigned({v}, {w}))")).collect();
    init.push("others => (others => '0')".into());
    format!("    type m{p}_t is array (0 to {}) of std_logic_vector({} downto 0);\n    signal m{p} : m{p}_t := ({});", words - 1, w - 1, init.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circ::{parse, Project};

    fn adder() -> Built {
        let mut p = Project::new();
        let t = crate::analysis::expressions("S = A ^ B ^ C; Co = A B + A C + B C").unwrap();
        p.circuits.insert("main".into(), crate::analysis::synthesise(&t, crate::analysis::Style::AndOr).unwrap());
        p.build("main").unwrap()
    }

    #[test]
    fn names_are_ones_both_languages_take() {
        assert_eq!(ident("carry out", VHDL_WORDS), "carry_out");
        assert_eq!(ident("2x", VHDL_WORDS), "p_2x");
        assert_eq!(ident("in", VHDL_WORDS), "in_p");
        assert_eq!(ident("a__b_", VHDL_WORDS), "a_b");
    }

    #[test]
    fn a_full_adder_is_written_as_a_module_and_an_entity() {
        let b = adder();
        let v = verilog(&b, "full adder");
        assert!(v.contains("module full_adder (\n    input wire A,\n    input wire B,\n    input wire C,\n    output wire Co,\n    output wire S\n);"), "{v}");
        assert!(v.contains(" & ") && v.contains(" | ") && v.trim_end().ends_with("endmodule"));
        assert_eq!(v.matches("assign S = ").count(), 1);
        let h = vhdl(&b, "full adder");
        assert!(h.contains("entity full_adder is") && h.contains("A : in std_logic") && h.contains("S : out std_logic") && h.contains("end architecture;"), "{h}");
        assert!(h.contains(" and ") && h.contains("S <= n"));
    }

    #[test]
    fn state_memories_drivers_and_pulls_are_written() {
        let mut p = Project::new();
        p.data.insert("t".into(), "v2.0 raw\n11 22\n".into());
        p.circuits.insert(
            "main".into(),
            parse(
                "part Clk clock at 0,1 label=Clk\npart D in at 0,0 bits=4 label=D\npart F register at 2,0 bits=4\npart Q out at 6,1 bits=4 label=Q\n\
                 part A in at 0,8 label=A\npart E in at 0,9 label=E\npart T driver at 2,8\npart P pullup at 6,8\npart Y out at 6,8 label=Y\n\
                 part R rom at 2,12 addr=1 bits=8 file=t\npart Ra in at 0,12 label=Ra\npart Rd out at 6,12 bits=8 label=Rd\n\
                 wire 0,0 2,0\nwire 0,1 2,1\nwire 5,1 6,1\nwire 0,8 2,8\nwire 0,9 2,9\nwire 4,8 6,8\nwire 0,12 2,12\nwire 5,12 6,12\n",
            )
            .unwrap(),
        );
        let b = p.build("main").unwrap();
        let v = verilog(&b, "main");
        assert!(v.contains("always @(posedge") && v.contains("reg [3:0] q"), "{v}");
        assert!(v.contains("tri1 "), "a pulled-up net");
        assert!(v.contains("{1{1'bz}}"), "a driver lets go");
        assert!(v.contains("] = 'h22;"), "the ROM's contents");
        let h = vhdl(&b, "main");
        assert!(h.contains("rising_edge(") && h.contains("(others => 'H')") && h.contains("1 => std_logic_vector(to_unsigned(34, 8))"), "{h}");
    }

    /// Icarus Verilog, when the Mac has it: the module compiled and driven
    /// through every row, its outputs those the simulator gives.
    #[test]
    fn icarus_runs_the_verilog_when_it_is_here() {
        let Ok(out) = std::process::Command::new("iverilog").arg("-V").output() else {
            eprintln!("iverilog is not on this Mac: skipped");
            return;
        };
        if !out.status.success() {
            return;
        }
        let b = adder();
        let dir = std::env::temp_dir().join(format!("proton-hdl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tb = "module tb; reg A, B, C; wire Co, S; integer i;\n  main_adder u (.A(A), .B(B), .C(C), .Co(Co), .S(S));\n  initial begin for (i = 0; i < 8; i = i + 1) begin {A, B, C} = i; #1 $display(\"%b%b\", Co, S); end end\nendmodule\n";
        std::fs::write(dir.join("a.v"), verilog(&b, "main_adder") + tb).unwrap();
        let built = std::process::Command::new("iverilog").arg("-o").arg(dir.join("a.out")).arg(dir.join("a.v")).output().unwrap();
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let ran = std::process::Command::new("vvp").arg(dir.join("a.out")).output().unwrap();
        let lines: Vec<String> = String::from_utf8_lossy(&ran.stdout).lines().map(str::to_string).collect();
        assert_eq!(lines, vec!["00", "01", "01", "10", "01", "10", "10", "11"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
