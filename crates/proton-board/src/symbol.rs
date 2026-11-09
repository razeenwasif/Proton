//! Symbols: a part's pins — numbered, named, and what each does
//! electrically, for the rules check — and how the part is drawn.
//!
//! Proton's own, for the common parts; any chip made from its pin list;
//! headers of any size; and the PC's connectors, for the small boards a
//! compact build uses, each from the standard it comes from. Two whose
//! pinouts could not be checked against their standards carry a caution
//! the rules check repeats until they are.

/// What a pin does, as the rules check reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinKind {
    Input,
    Output,
    /// Both ways: a data line.
    Bidir,
    /// An output that lets go.
    Tristate,
    /// A resistor's, a connector's: no direction of its own.
    Passive,
    /// Takes power: it must be driven by a supply.
    PowerIn,
    /// Gives power: a regulator's output.
    PowerOut,
    OpenCollector,
    OpenEmitter,
    /// Not to be connected.
    Nc,
}

impl PinKind {
    pub const ALL: [PinKind; 10] = [
        PinKind::Input,
        PinKind::Output,
        PinKind::Bidir,
        PinKind::Tristate,
        PinKind::Passive,
        PinKind::PowerIn,
        PinKind::PowerOut,
        PinKind::OpenCollector,
        PinKind::OpenEmitter,
        PinKind::Nc,
    ];

    pub fn word(self) -> &'static str {
        match self {
            PinKind::Input => "input",
            PinKind::Output => "output",
            PinKind::Bidir => "bidir",
            PinKind::Tristate => "tristate",
            PinKind::Passive => "passive",
            PinKind::PowerIn => "power-in",
            PinKind::PowerOut => "power-out",
            PinKind::OpenCollector => "open-collector",
            PinKind::OpenEmitter => "open-emitter",
            PinKind::Nc => "nc",
        }
    }

    pub fn from_word(w: &str) -> Option<PinKind> {
        PinKind::ALL.into_iter().find(|k| k.word() == w)
    }

    /// It sets a net's level.
    pub fn drives(self) -> bool {
        !matches!(self, PinKind::Input | PinKind::PowerIn | PinKind::Nc)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    /// As printed on the part: `1`, `S2`.
    pub number: String,
    pub name: String,
    pub kind: PinKind,
}

/// A two-pin part's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Two {
    Resistor,
    Capacitor,
    Polarised,
    Inductor,
    Diode,
    Led,
    Zener,
    Fuse,
    Switch,
    Crystal,
}

/// A transistor's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tr {
    Npn,
    Pnp,
    Nmos,
    Pmos,
}

/// How a symbol is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Pin 1 at the left, pin 2 at the right — but a diode's anode, pin
    /// 2, at the left, so current runs left to right.
    Two(Two),
    /// Base or gate at the left, collector or drain up, emitter or source
    /// down.
    Transistor(Tr),
    /// A box with pins down each side, by their places in `pins`, top to
    /// bottom.
    Box { left: Vec<usize>, right: Vec<usize> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    /// The library's word for it: `resistor`, `atx-24`.
    pub kind: String,
    /// What it is, in words.
    pub title: String,
    /// A reference's letters: `R`, `J`, `U`.
    pub prefix: &'static str,
    pub pins: Vec<Pin>,
    pub body: Body,
    /// Where its pinout is from.
    pub source: &'static str,
    /// Why its pinout is not to be trusted yet, if it is not.
    pub caution: Option<&'static str>,
}

impl Symbol {
    /// A pin by its number, or its name if the name is the only one.
    pub fn pin(&self, which: &str) -> Option<usize> {
        if let Some(i) = self.pins.iter().position(|p| p.number == which) {
            return Some(i);
        }
        let named: Vec<usize> = self.pins.iter().enumerate().filter(|(_, p)| p.name == which).map(|(i, _)| i).collect();
        (named.len() == 1).then(|| named[0])
    }
}

/// The library: each kind's word, what it is, and what it takes after
/// its word in a `part` line.
pub fn kinds() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("resistor", "Resistor", ""),
        ("capacitor", "Capacitor", ""),
        ("capacitor-polarised", "Capacitor, polarised", ""),
        ("inductor", "Inductor", ""),
        ("diode", "Diode", ""),
        ("led", "LED", ""),
        ("zener", "Zener diode", ""),
        ("fuse", "Fuse", ""),
        ("switch", "Switch", ""),
        ("crystal", "Crystal", ""),
        ("npn", "NPN transistor (SOT-23: 1 B, 2 E, 3 C)", ""),
        ("pnp", "PNP transistor (SOT-23: 1 B, 2 E, 3 C)", ""),
        ("nmos", "N-channel MOSFET (SOT-23: 1 G, 2 S, 3 D)", ""),
        ("pmos", "P-channel MOSFET (SOT-23: 1 G, 2 S, 3 D)", ""),
        ("header", "Pin header", "N, or ROWSxN"),
        ("ic", "A chip from its pins", "\"NAME\" pins (1 NAME KIND, …)"),
        ("kicad", "A KiCad library's symbol", "\"Library:Name\""),
        ("atx-24", "ATX 24-pin power", ""),
        ("eps-8", "EPS 8-pin CPU power", ""),
        ("pcie-6", "PCIe 6-pin graphics power", ""),
        ("pcie-8", "PCIe 8-pin (6+2) graphics power", ""),
        ("12v-2x6", "12V-2×6 graphics power (16-pin)", ""),
        ("sata-power", "SATA power, 15-pin", ""),
        ("fan-4pin", "4-pin PWM fan", ""),
        ("argb-5v", "5 V addressable RGB, 3-pin", ""),
        ("usb2-header", "USB 2.0 front-panel header, 9-pin", ""),
        ("usb3-header", "USB 3 front-panel header, 19-pin", ""),
        ("usb-c-header", "USB-C front-panel header, Key-A 20-pin", ""),
    ]
}

/// What a `part` line says after its kind: a header's size, a chip's name
/// and pins.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Args {
    /// A header's rows and pins in each.
    pub size: Option<(u32, u32)>,
    /// A chip's name.
    pub name: Option<String>,
    pub pins: Vec<Pin>,
}

fn pins(list: &[(&str, &str, PinKind)]) -> Vec<Pin> {
    list.iter().map(|(n, name, k)| Pin { number: (*n).into(), name: (*name).into(), kind: *k }).collect()
}

fn numbered(names: &[&str]) -> Vec<Pin> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| Pin { number: (i + 1).to_string(), name: (*n).to_string(), kind: if *n == "NC" || *n == "KEY" { PinKind::Nc } else { PinKind::Passive } })
        .collect()
}

/// The first half down the left, the rest down the right: a connector's
/// two rows.
fn rows(n: usize) -> Body {
    let half = n.div_ceil(2);
    Body::Box { left: (0..half).collect(), right: (half..n).collect() }
}

/// Odd pins at the left, even at the right: a two-row header's.
fn odd_even(n: usize) -> Body {
    Body::Box { left: (0..n).step_by(2).collect(), right: (1..n).step_by(2).collect() }
}

fn two(kind: &str, title: &str, prefix: &'static str, look: Two, names: [&str; 2]) -> Symbol {
    Symbol {
        kind: kind.into(),
        title: title.into(),
        prefix,
        pins: pins(&[("1", names[0], PinKind::Passive), ("2", names[1], PinKind::Passive)]),
        body: Body::Two(look),
        source: "Proton's own",
        caution: None,
    }
}

fn connector(kind: &str, title: &str, names: &[&str], body: Body, source: &'static str, caution: Option<&'static str>) -> Symbol {
    Symbol { kind: kind.into(), title: title.into(), prefix: "J", pins: numbered(names), body, source, caution }
}

/// A kind from the library, made with what its line gives.
pub fn make(kind: &str, args: &Args) -> Result<Symbol, String> {
    let passive = PinKind::Passive;
    Ok(match kind {
        "resistor" => two(kind, "Resistor", "R", Two::Resistor, ["~", "~"]),
        "capacitor" => two(kind, "Capacitor", "C", Two::Capacitor, ["~", "~"]),
        "capacitor-polarised" => two(kind, "Capacitor, polarised", "C", Two::Polarised, ["+", "-"]),
        "inductor" => two(kind, "Inductor", "L", Two::Inductor, ["~", "~"]),
        "diode" => two(kind, "Diode", "D", Two::Diode, ["K", "A"]),
        "led" => two(kind, "LED", "D", Two::Led, ["K", "A"]),
        "zener" => two(kind, "Zener diode", "D", Two::Zener, ["K", "A"]),
        "fuse" => two(kind, "Fuse", "F", Two::Fuse, ["~", "~"]),
        "switch" => two(kind, "Switch", "SW", Two::Switch, ["~", "~"]),
        "crystal" => two(kind, "Crystal", "Y", Two::Crystal, ["~", "~"]),
        "npn" | "pnp" => Symbol {
            kind: kind.into(),
            title: if kind == "npn" { "NPN transistor" } else { "PNP transistor" }.into(),
            prefix: "Q",
            pins: pins(&[("1", "B", passive), ("2", "E", passive), ("3", "C", passive)]),
            body: Body::Transistor(if kind == "npn" { Tr::Npn } else { Tr::Pnp }),
            source: "SOT-23's usual order",
            caution: None,
        },
        "nmos" | "pmos" => Symbol {
            kind: kind.into(),
            title: if kind == "nmos" { "N-channel MOSFET" } else { "P-channel MOSFET" }.into(),
            prefix: "Q",
            pins: pins(&[("1", "G", passive), ("2", "S", passive), ("3", "D", passive)]),
            body: Body::Transistor(if kind == "nmos" { Tr::Nmos } else { Tr::Pmos }),
            source: "SOT-23's usual order",
            caution: None,
        },
        "header" => {
            let (r, n) = args.size.ok_or("a header says its size: `header 4`, or `header 2x5`")?;
            if !(1..=2).contains(&r) || n == 0 || n > 64 {
                return Err(format!("a header is one or two rows of 1 to 64 pins, not {r}x{n}"));
            }
            let count = (r * n) as usize;
            let ps: Vec<Pin> = (1..=count).map(|i| Pin { number: i.to_string(), name: format!("{i}"), kind: passive }).collect();
            Symbol {
                kind: kind.into(),
                title: if r == 1 { format!("Pin header, 1×{n}") } else { format!("Pin header, 2×{n}") },
                prefix: "J",
                pins: ps,
                body: if r == 1 { Body::Box { left: (0..count).collect(), right: Vec::new() } } else { odd_even(count) },
                source: "Proton's own",
                caution: None,
            }
        }
        "ic" => {
            let name = args.name.clone().ok_or("a chip says its name: `ic \"NE555\" pins (…)`")?;
            if args.pins.is_empty() {
                return Err("a chip lists its pins: `pins (1 GND power-in, 2 TRIG input, …)`".into());
            }
            let n = args.pins.len();
            let half = n.div_ceil(2);
            // As a DIP's: down the left, then up the right.
            Symbol {
                kind: kind.into(),
                title: name,
                prefix: "U",
                pins: args.pins.clone(),
                body: Body::Box { left: (0..half).collect(), right: (half..n).rev().collect() },
                source: "its pin list",
                caution: None,
            }
        }
        // ATX12V and ATX 3.x: the 24-pin main power, rows 1–12 and 13–24.
        "atx-24" => connector(
            kind,
            "ATX 24-pin power",
            &["+3.3V", "+3.3V", "COM", "+5V", "COM", "+5V", "COM", "PWR_OK", "+5VSB", "+12V1", "+12V1", "+3.3V", "+3.3V", "-12V", "COM", "PS_ON#", "COM", "COM", "COM", "NC", "+5V", "+5V", "+5V", "COM"],
            rows(24),
            "Intel's ATX12V / ATX 3 design guide",
            None,
        ),
        "eps-8" => connector(kind, "EPS 8-pin CPU power", &["COM", "COM", "COM", "COM", "+12V2", "+12V2", "+12V2", "+12V2"], rows(8), "EPS12V", None),
        "pcie-6" => connector(kind, "PCIe 6-pin graphics power", &["+12V", "+12V", "+12V", "GND", "SENSE", "GND"], rows(6), "NVIDIA's Quadro power guidelines, table 2", None),
        "pcie-8" => connector(kind, "PCIe 8-pin graphics power", &["+12V", "+12V", "+12V", "SENSE1", "GND", "SENSE0", "GND", "GND"], rows(8), "NVIDIA's Quadro power guidelines, table 2", None),
        "12v-2x6" => {
            let mut s = connector(
                kind,
                "12V-2×6 graphics power",
                &["+12V", "+12V", "+12V", "+12V", "+12V", "+12V", "GND", "GND", "GND", "GND", "GND", "GND"],
                rows(12),
                "the PCIe CEM 5.x 12V-2×6 connector",
                Some("the 12V-2×6's pin numbers and its sideband's S1–S4 are as commonly drawn, not yet checked against PCI-SIG's sideband ECN: check them before a board is made"),
            );
            for (n, name) in [("S1", "CARD_PWR_STABLE"), ("S2", "CARD_CBL_PRES#"), ("S3", "SENSE0"), ("S4", "SENSE1")] {
                s.pins.push(Pin { number: n.into(), name: name.into(), kind: passive });
            }
            s.body = Body::Box { left: vec![0, 1, 2, 3, 4, 5, 12, 13], right: vec![6, 7, 8, 9, 10, 11, 14, 15] };
            s
        }
        "sata-power" => connector(
            kind,
            "SATA power",
            &["+3.3V", "+3.3V", "+3.3V", "GND", "GND", "GND", "+5V", "+5V", "+5V", "GND", "DAS/DSS", "GND", "+12V", "+12V", "+12V"],
            Body::Box { left: (0..15).collect(), right: Vec::new() },
            "Serial ATA's power segment, P1–P15",
            None,
        ),
        "fan-4pin" => connector(kind, "4-pin PWM fan", &["GND", "+12V", "SENSE", "PWM"], Body::Box { left: (0..4).collect(), right: Vec::new() }, "Intel's 4-wire PWM fan specification", None),
        "argb-5v" => connector(kind, "5 V addressable RGB", &["+5V", "DATA", "KEY", "GND"], Body::Box { left: (0..4).collect(), right: Vec::new() }, "the 5 V 3-pin header's usual pinout", None),
        "usb2-header" => connector(kind, "USB 2.0 front-panel header", &["VBUS", "VBUS", "D1-", "D2-", "D1+", "D2+", "GND", "GND", "KEY", "NC"], odd_even(10), "Intel's front-panel I/O design guide", None),
        "usb3-header" => connector(
            kind,
            "USB 3 front-panel header",
            &[
                "VBUS", "P1_SSRX-", "P1_SSRX+", "GND", "P1_SSTX-", "P1_SSTX+", "GND", "P1_D-", "P1_D+", "ID", "P2_D+", "P2_D-", "GND", "P2_SSTX+", "P2_SSTX-", "GND", "P2_SSRX+",
                "P2_SSRX-", "VBUS", "KEY",
            ],
            rows(20),
            "Intel's USB 3.0 internal connector specification, table 2-1",
            None,
        ),
        "usb-c-header" => connector(
            kind,
            "USB-C front-panel header, Key-A",
            &["VBUS", "TX1+", "TX1-", "GND", "RX1+", "RX1-", "VBUS", "CC1", "SBU1", "SBU2", "VBUS", "TX2+", "TX2-", "GND", "RX2+", "RX2-", "GND", "D-", "D+", "CC2"],
            rows(20),
            "an Intel NUC's signal table, read second hand",
            Some("the USB-C Key-A header's pinout is from a summary of Intel's NUC signal table, not read from it: check it against your board's manual before a board is made"),
        ),
        "kicad" => return Err("a KiCad symbol is read from its library, and there is none here".into()),
        other => return Err(format!("`{other}` is not in the library: {}", kinds().iter().map(|k| k.0).collect::<Vec<_>>().join(", "))),
    })
}
