//! The `.schematic` language: a board's parts and the nets between them,
//! one statement a line, as text that diffs.
//!
//! ```text
//! schematic "Fan hub"
//! sheet A4
//! part P1 sata-power at (40, 140)
//! part J1 fan-4pin at (150, 160)
//! part J2 fan-4pin at (150, 110)
//! part R1 resistor value 10k footprint 0603 at (100, 60) turn 90
//! part U1 ic "NE555" pins (1 GND power-in, 2 TRIG input, 3 OUT output, 8 VCC power-in) at (100, 110)
//! net +12V power P1.13, P1.14, P1.15, J1.2, J2.2 current 3
//! net GND P1.4, P1.5, P1.6, J1.1, J2.1
//! net PWM J1.PWM, J2.PWM
//! nc J1.3, J2.3
//! ```
//!
//! A pin is `PART.NUMBER` or `PART.NAME` where the name is the part's
//! only one. A net is `power` when something off the board drives it — a
//! supply's cable — and says the `current` it carries, amps, for its
//! tracks' widths. `#` after a space starts a comment: `PS_ON#` is a name.

use proton_cad::part::ast::Diag;

use crate::symbol::{Args, Pin, PinKind};

#[derive(Debug, Clone, PartialEq)]
pub struct PinRef {
    pub part: String,
    pub pin: String,
}

impl PinRef {
    pub fn text(&self) -> String {
        format!("{}.{}", self.part, self.pin)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub reference: String,
    pub kind: String,
    pub args: Args,
    pub value: Option<String>,
    pub footprint: Option<String>,
    /// Its middle on the sheet, millimetres from the bottom left.
    pub at: Option<[f64; 2]>,
    /// Degrees, anticlockwise: 0, 90, 180 or 270.
    pub turn: u32,
    pub mirror: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub name: String,
    /// Driven from off the board.
    pub power: bool,
    pub pins: Vec<PinRef>,
    /// Amps.
    pub current: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Blank,
    Comment(String),
    Name(String),
    Sheet(String),
    Part(Part),
    Net(Net),
    /// Pins meant to be left unconnected.
    Nc(Vec<PinRef>),
}

/// A statement, its line, and a comment after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub line: usize,
    pub item: Item,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schematic {
    pub items: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Word(String),
    Str(String),
    Sym(char),
}

/// A line's tokens, each with its column, and the comment after them.
pub(crate) type Lexed = (Vec<(Tok, usize)>, Option<String>);

/// A line's words, strings and `(`, `)`, `,`; and its comment.
pub(crate) fn lex(line: &str, no: usize) -> Result<Lexed, Diag> {
    let mut out = Vec::new();
    let cs: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' && (i == 0 || cs[i - 1].is_whitespace()) {
            let rest: String = cs[i + 1..].iter().collect();
            return Ok((out, Some(rest.trim().to_string())));
        } else if c == '"' {
            let start = i;
            let mut s = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err(Diag::new(no, start + 1, "a string is not closed")),
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('\\') if cs.get(i + 1) == Some(&'"') => {
                        s.push('"');
                        i += 2;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push((Tok::Str(s), start + 1));
        } else if matches!(c, '(' | ')' | ',') {
            out.push((Tok::Sym(c), i + 1));
            i += 1;
        } else {
            let start = i;
            while i < cs.len() && !cs[i].is_whitespace() && !matches!(cs[i], '(' | ')' | ',' | '"') {
                i += 1;
            }
            out.push((Tok::Word(cs[start..i].iter().collect()), start + 1));
        }
    }
    Ok((out, None))
}

pub(crate) struct Cur<'a> {
    pub(crate) toks: &'a [(Tok, usize)],
    pub(crate) i: usize,
    pub(crate) line: usize,
    pub(crate) end: usize,
}

impl Cur<'_> {
    pub(crate) fn col(&self) -> usize {
        self.toks.get(self.i).map_or(self.end, |t| t.1)
    }
    pub(crate) fn err(&self, m: impl Into<String>) -> Diag {
        Diag::new(self.line, self.col(), m)
    }
    pub(crate) fn word(&mut self, what: &str) -> Result<String, Diag> {
        match self.toks.get(self.i) {
            Some((Tok::Word(w), _)) => {
                self.i += 1;
                Ok(w.clone())
            }
            _ => Err(self.err(format!("expected {what}"))),
        }
    }
    pub(crate) fn string(&mut self, what: &str) -> Result<String, Diag> {
        match self.toks.get(self.i) {
            Some((Tok::Str(s), _)) => {
                self.i += 1;
                Ok(s.clone())
            }
            _ => Err(self.err(format!("expected {what} in quotes"))),
        }
    }
    pub(crate) fn sym(&mut self, c: char) -> Result<(), Diag> {
        match self.toks.get(self.i) {
            Some((Tok::Sym(x), _)) if *x == c => {
                self.i += 1;
                Ok(())
            }
            _ => Err(self.err(format!("expected `{c}`"))),
        }
    }
    pub(crate) fn peek_sym(&self, c: char) -> bool {
        matches!(self.toks.get(self.i), Some((Tok::Sym(x), _)) if *x == c)
    }
    pub(crate) fn number(&mut self, what: &str) -> Result<f64, Diag> {
        let at = self.col();
        let w = self.word(what)?;
        w.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| Diag::new(self.line, at, format!("`{w}` is not a number")))
    }
    pub(crate) fn done(&self) -> bool {
        self.i >= self.toks.len()
    }
    /// A word or a string: a value.
    fn value(&mut self, what: &str) -> Result<String, Diag> {
        match self.toks.get(self.i) {
            Some((Tok::Word(w), _)) | Some((Tok::Str(w), _)) => {
                self.i += 1;
                Ok(w.clone())
            }
            _ => Err(self.err(format!("expected {what}"))),
        }
    }
}

pub(crate) fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "_+-./#~".contains(c))
}

pub(crate) fn reference_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn pin_ref(p: &mut Cur) -> Result<PinRef, Diag> {
    let at = p.col();
    let w = p.word("a pin, `PART.PIN`")?;
    match w.split_once('.') {
        Some((part, pin)) if reference_ok(part) && !pin.is_empty() => Ok(PinRef { part: part.into(), pin: pin.into() }),
        _ => Err(Diag::new(p.line, at, format!("`{w}` is not a pin: a pin is `PART.PIN`, `J1.2` or `J1.PWM`"))),
    }
}

fn pin_list(p: &mut Cur) -> Result<Vec<PinRef>, Diag> {
    let mut out = vec![pin_ref(p)?];
    while p.peek_sym(',') {
        p.i += 1;
        out.push(pin_ref(p)?);
    }
    Ok(out)
}

/// A `.schematic` read; its first mistake at its line and column.
pub fn read(text: &str) -> Result<Schematic, Diag> {
    let mut items = Vec::new();
    for (k, raw) in text.lines().enumerate() {
        let no = k + 1;
        let (toks, note) = lex(raw, no)?;
        if toks.is_empty() {
            items.push(Line { line: no, item: note.clone().map_or(Item::Blank, Item::Comment), note: None });
            continue;
        }
        let mut p = Cur { toks: &toks, i: 0, line: no, end: raw.chars().count() + 1 };
        let head = p.word("a statement")?;
        let item = match head.as_str() {
            "schematic" => Item::Name(p.string("its title")?),
            "sheet" => {
                let at = p.col();
                let s = p.word("a sheet's size")?;
                if proton_cad::drawing::size(&s.replace('_', "-")).is_none() {
                    return Err(Diag::new(no, at, format!("`{s}` is not a sheet: A4, A3, A2, A1, A0, ANSI_A, ANSI_B, ANSI_C or ANSI_D")));
                }
                Item::Sheet(s)
            }
            "part" => {
                let at = p.col();
                let reference = p.word("its reference, `R1`")?;
                if !reference_ok(&reference) {
                    return Err(Diag::new(no, at, format!("`{reference}` is not a reference: letters then digits, `R1`, `J12`")));
                }
                let kind = p.word("what it is: `resistor`, `header 4`, `atx-24`…")?;
                let mut args = Args::default();
                if kind == "header" {
                    let at = p.col();
                    let s = p.word("its size, `4` or `2x5`")?;
                    let size = match s.split_once('x') {
                        Some((r, n)) => r.parse().ok().zip(n.parse().ok()),
                        None => s.parse().ok().map(|n| (1, n)),
                    };
                    args.size = Some(size.ok_or_else(|| Diag::new(no, at, format!("`{s}` is not a header's size: `4`, or `2x5`")))?);
                }
                if kind == "ic" {
                    args.name = Some(p.string("its name")?);
                    let at = p.col();
                    if p.word("`pins`")? != "pins" {
                        return Err(Diag::new(no, at, "expected `pins`"));
                    }
                    p.sym('(')?;
                    loop {
                        let number = p.word("a pin's number")?;
                        let name = p.word("its name")?;
                        let at = p.col();
                        let k = p.word("what it does: input, output, bidir, tristate, passive, power-in, power-out, open-collector, open-emitter or nc")?;
                        let kind = PinKind::from_word(&k).ok_or_else(|| Diag::new(no, at, format!("`{k}` is not what a pin does: input, output, bidir, tristate, passive, power-in, power-out, open-collector, open-emitter or nc")))?;
                        args.pins.push(Pin { number, name, kind });
                        if p.peek_sym(',') {
                            p.i += 1;
                        } else {
                            break;
                        }
                    }
                    p.sym(')')?;
                }
                let mut part = Part { reference, kind, args, value: None, footprint: None, at: None, turn: 0, mirror: false };
                while !p.done() {
                    let at = p.col();
                    match p.word("`value`, `footprint`, `at`, `turn` or `mirror`")?.as_str() {
                        "value" => part.value = Some(p.value("its value")?),
                        "footprint" => part.footprint = Some(p.value("its footprint")?),
                        "at" => {
                            p.sym('(')?;
                            let x = p.number("x")?;
                            p.sym(',')?;
                            let y = p.number("y")?;
                            p.sym(')')?;
                            part.at = Some([x, y]);
                        }
                        "turn" => {
                            let at = p.col();
                            let t = p.number("degrees")?;
                            if ![0.0, 90.0, 180.0, 270.0].contains(&t) {
                                return Err(Diag::new(no, at, "a part turns 0, 90, 180 or 270 degrees"));
                            }
                            part.turn = t as u32;
                        }
                        "mirror" => part.mirror = true,
                        w => return Err(Diag::new(no, at, format!("`{w}`: a part goes on with `value`, `footprint`, `at`, `turn` or `mirror`"))),
                    }
                }
                Item::Part(part)
            }
            "net" => {
                let at = p.col();
                let name = p.word("its name")?;
                if !name_ok(&name) {
                    return Err(Diag::new(no, at, format!("`{name}` is not a net's name")));
                }
                let power = matches!(p.toks.get(p.i), Some((Tok::Word(w), _)) if w == "power");
                if power {
                    p.i += 1;
                }
                let pins = pin_list(&mut p)?;
                let mut current = None;
                if !p.done() {
                    let at = p.col();
                    if p.word("`current`")? != "current" {
                        return Err(Diag::new(no, at, "a net goes on with `current`, amps"));
                    }
                    let at = p.col();
                    let a = p.number("amps")?;
                    if a <= 0.0 {
                        return Err(Diag::new(no, at, "a current is more than nothing"));
                    }
                    current = Some(a);
                }
                Item::Net(Net { name, power, pins, current })
            }
            "nc" => Item::Nc(pin_list(&mut p)?),
            w => return Err(Diag::new(no, 1, format!("`{w}` is not a statement of a schematic: schematic, sheet, part, net or nc"))),
        };
        if !p.done() {
            return Err(p.err("more than the statement says"));
        }
        items.push(Line { line: no, item, note });
    }
    Ok(Schematic { items })
}

/// A number as the file writes it: briefly.
pub fn num(x: f64) -> String {
    let s = format!("{:.4}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

pub(crate) fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// A value: a word if it reads as one, else in quotes.
fn value(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| !c.is_whitespace() && !matches!(c, '(' | ')' | ',' | '"' | '#')) {
        s.into()
    } else {
        quoted(s)
    }
}

/// One statement as the file writes it.
pub fn statement(item: &Item) -> String {
    let pins = |ps: &[PinRef]| ps.iter().map(PinRef::text).collect::<Vec<_>>().join(", ");
    match item {
        Item::Blank => String::new(),
        Item::Comment(c) => format!("# {c}"),
        Item::Name(t) => format!("schematic {}", quoted(t)),
        Item::Sheet(s) => format!("sheet {s}"),
        Item::Part(p) => {
            let mut l = format!("part {} {}", p.reference, p.kind);
            if let Some((r, n)) = p.args.size {
                l.push_str(&if r == 1 { format!(" {n}") } else { format!(" {r}x{n}") });
            }
            if let Some(name) = &p.args.name {
                l.push_str(&format!(" {} pins ({})", quoted(name), p.args.pins.iter().map(|x| format!("{} {} {}", x.number, x.name, x.kind.word())).collect::<Vec<_>>().join(", ")));
            }
            if let Some(v) = &p.value {
                l.push_str(&format!(" value {}", value(v)));
            }
            if let Some(f) = &p.footprint {
                l.push_str(&format!(" footprint {}", value(f)));
            }
            if let Some([x, y]) = p.at {
                l.push_str(&format!(" at ({}, {})", num(x), num(y)));
            }
            if p.turn != 0 {
                l.push_str(&format!(" turn {}", p.turn));
            }
            if p.mirror {
                l.push_str(" mirror");
            }
            l
        }
        Item::Net(n) => format!("net {}{} {}{}", n.name, if n.power { " power" } else { "" }, pins(&n.pins), n.current.map_or(String::new(), |a| format!(" current {}", num(a)))),
        Item::Nc(ps) => format!("nc {}", pins(ps)),
    }
}

/// The schematic as text.
pub fn write(s: &Schematic) -> String {
    let mut out = String::new();
    for l in &s.items {
        out.push_str(&statement(&l.item));
        if let Some(n) = &l.note {
            out.push_str(&format!("  # {n}"));
        }
        out.push('\n');
    }
    out
}
