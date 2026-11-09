//! The parts library: parametric parts made from published standards and
//! specifications, each written as a `.part` file of the language — so a
//! part from the library is a file like any other, read, edited and kept
//! in Git — and each checked against its figures in tests.
//!
//! Where a figure is a standard's (ATX, SFX12V, ISO 4762, BSPP), the part
//! says so and holds it; where makers differ (a radiator's end tanks, a
//! fitting's body), the figure is a parameter with a typical default, and
//! the file says to check it against the part's own drawing. Nothing is
//! taken from another program's library.

pub mod t1;

use std::collections::HashMap;
use std::fmt::Write as _;

/// A parameter of a family: its name, its default, the values it takes if
/// it is a choice, and what it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub default: &'static str,
    pub choices: &'static [&'static str],
    pub note: &'static str,
}

/// A family of parts.
#[derive(Debug, Clone)]
pub struct Family {
    pub name: &'static str,
    pub title: &'static str,
    pub group: &'static str,
    pub params: Vec<Param>,
    make: fn(&Values) -> Result<Made, String>,
}

/// A part made: the file's name (without `.part`) and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub stem: String,
    pub text: String,
}

/// The values a part is made with, defaults filled in.
pub struct Values {
    v: HashMap<String, String>,
}

impl Values {
    fn text(&self, k: &str) -> &str {
        self.v.get(k).map(String::as_str).unwrap_or("")
    }

    fn num(&self, k: &str) -> Result<f64, String> {
        let s = self.text(k);
        s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("`{k}` is a number, not “{s}”"))
    }

    fn positive(&self, k: &str) -> Result<f64, String> {
        let x = self.num(k)?;
        if x > 0.0 {
            Ok(x)
        } else {
            Err(format!("`{k}` must be more than nothing, not {x}"))
        }
    }
}

const fn p(name: &'static str, default: &'static str, note: &'static str) -> Param {
    Param { name, default, choices: &[], note }
}

const fn choice(name: &'static str, default: &'static str, choices: &'static [&'static str], note: &'static str) -> Param {
    Param { name, default, choices, note }
}

/// Every family there is.
pub fn families() -> Vec<Family> {
    vec![
        Family { name: "mini-itx", title: "Mini-ITX motherboard", group: "Boards and power", params: vec![p("thickness", "1.57", "the board, mm (ATX: 0.062 in)"), p("io_depth", "35", "how far the rear I/O stack reaches in, mm (makers differ)")], make: mini_itx },
        Family { name: "sfx-psu", title: "SFX power supply", group: "Boards and power", params: vec![choice("size", "sfx", &["sfx", "sfx-l"], "SFX12V: 125 × 63.5 × 100; SFX-L 130 deep")], make: sfx_psu },
        Family { name: "m2", title: "M.2 drive", group: "Boards and power", params: vec![choice("length", "2280", &["2230", "2242", "2260", "2280", "22110"], "22 mm wide, this long"), p("thickness", "2.38", "double-sided 2.38, single 1.5 mm")], make: m2 },
        Family { name: "dimm", title: "Memory module (DIMM)", group: "Boards and power", params: vec![p("height", "31.25", "31.25 mm bare; heatspreaders run 33–45"), p("thickness", "6.5", "with its heatspreader, mm")], make: dimm },
        Family {
            name: "gpu",
            title: "Graphics card",
            group: "Graphics",
            params: vec![p("length", "304", "from the bracket's face, mm"), p("height", "137", "from the slot's edge, mm"), p("slots", "3", "thickness in slots of 20.32 mm"), p("ports", "0", "G1/4 ports on its top edge: 2 for a water block")],
            make: gpu,
        },
        Family {
            name: "fan",
            title: "Fan",
            group: "Cooling",
            params: vec![choice("size", "120", &["80", "92", "120", "140"], "square, mm"), choice("thickness", "25", &["15", "25", "38"], "mm")],
            make: fan,
        },
        Family {
            name: "radiator",
            title: "Radiator",
            group: "Cooling",
            params: vec![
                choice("fans", "2", &["1", "2", "3"], "fans along it"),
                choice("fan", "120", &["120", "140"], "fan size, mm"),
                p("thickness", "30", "the core and tanks, mm"),
                p("width", "", "mm; empty: the fan size and 4"),
                p("tanks", "30", "each end tank's length, mm (makers differ: check)"),
                p("port_spacing", "36", "between the G1/4 ports, mm (makers differ: check)"),
            ],
            make: radiator,
        },
        Family { name: "cpu-block", title: "CPU water block", group: "Cooling", params: vec![p("width", "68", "mm"), p("height", "26", "from the CPU's lid, mm"), p("port_spacing", "26", "mm")], make: cpu_block },
        Family { name: "pump", title: "Pump (D5 or DDC)", group: "Cooling", params: vec![choice("kind", "d5", &["d5", "ddc"], "D5: ⌀62 × 38; DDC: 62 × 62 × 38"), p("reservoir", "0", "a reservoir on top, mm long (60 across); 0 for none")], make: pump },
        Family {
            name: "fitting",
            title: "G1/4 fitting",
            group: "Cooling",
            params: vec![
                choice("kind", "straight", &["straight", "elbow", "extender", "plug"], "what it does (a 45° comes with routes)"),
                choice("tube", "14/16", &["14/16", "12/16", "10/13", "none"], "the tube it takes, ID/OD mm"),
                p("length", "10", "an extender's length, mm"),
            ],
            make: fitting,
        },
        Family {
            name: "screw",
            title: "Screw (ISO 4762, 7380, 10642)",
            group: "Fasteners",
            params: vec![choice("size", "M3", &["M2", "M2.5", "M3", "M4", "M5"], "ISO metric"), p("length", "8", "under the head, mm"), choice("head", "socket", &["socket", "button", "flat"], "ISO 4762, 7380, 10642")],
            make: screw,
        },
        Family { name: "nut", title: "Hex nut (ISO 4032)", group: "Fasteners", params: vec![choice("size", "M3", &["M2", "M2.5", "M3", "M4", "M5"], "ISO metric")], make: nut },
        Family { name: "standoff", title: "Standoff", group: "Fasteners", params: vec![choice("thread", "6-32", &["6-32", "M3"], "a motherboard's is 6-32"), p("length", "6.35", "mm")], make: standoff },
        Family { name: "formd-t1", title: "FormD T1 (V2.1)", group: "Cases", params: t1::params(), make: t1::make },
    ]
}

/// A part of `family` made with `values` (by name; the rest default).
pub fn make(family: &str, values: &[(String, String)]) -> Result<Made, String> {
    let f = families().into_iter().find(|f| f.name == family).ok_or_else(|| format!("the library has no `{family}`"))?;
    let mut v: HashMap<String, String> = f.params.iter().map(|p| (p.name.to_string(), p.default.to_string())).collect();
    for (k, x) in values {
        let Some(param) = f.params.iter().find(|p| p.name == k) else { return Err(format!("a {} has no `{k}`", f.title)) };
        if !param.choices.is_empty() && !param.choices.contains(&x.as_str()) {
            return Err(format!("`{k}` is one of {}, not “{x}”", param.choices.join(", ")));
        }
        v.insert(k.clone(), x.clone());
    }
    (f.make)(&Values { v })
}

/* ── writing parts ─────────────────────────────────────────────────── */

/// A part's text being written.
struct Text(String);

impl Text {
    fn new(title: &str, material: Option<&str>, notes: &[&str]) -> Text {
        let mut t = String::new();
        for n in notes {
            let _ = writeln!(t, "# {n}");
        }
        let _ = writeln!(t, "part \"{title}\"\nunits mm");
        if let Some(m) = material {
            let _ = writeln!(t, "material {m}");
        }
        Text(t)
    }

    fn line(&mut self, l: impl AsRef<str>) -> &mut Self {
        self.0.push_str(l.as_ref());
        self.0.push('\n');
        self
    }

    /// A sketch on `on` of the shapes given, one a line.
    fn sketch(&mut self, name: &str, on: &str, shapes: &[String]) -> &mut Self {
        self.line(format!("sketch {name} on {on}"));
        for s in shapes {
            self.line(format!("  {s}"));
        }
        self.line("end")
    }
}

/// A number for a file: briefly, to a nanometre.
fn n(x: f64) -> String {
    crate::part::print::num(x)
}

/// A rectangle by its centre and size.
fn rect(name: &str, cx: f64, cy: f64, w: f64, h: f64) -> String {
    format!("rect {name} corner ({}, {}) size = ({}, {})", n(cx - w / 2.0), n(cy - h / 2.0), n(w), n(h))
}

fn circle(name: &str, cx: f64, cy: f64, d: f64) -> String {
    format!("circle {name} center ({}, {}) diameter = {}", n(cx), n(cy), n(d))
}

fn stem(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c.to_ascii_lowercase() } else { '-' }).collect::<String>().split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
}

/* ── boards and power ─────────────────────────────────────────────── */

/// A Mini-ITX board's mounting holes, from its rear-left corner (x along
/// the rear edge, y toward the front), mm: ATX holes C, F, H and J
/// (ATX 2.2, Figure 3: C at 4.900 in from hole A, F 11.100 in and 0.900 in
/// forward, H and J 6.100 in forward; the 6.7 in board's left edge 0.25 in
/// from C, its rear edge 0.4 in behind it).
pub const MINI_ITX_HOLES: [(&str, f64, f64); 4] = [("c", 6.35, 10.16), ("f", 163.83, 33.02), ("h", 6.35, 165.1), ("j", 163.83, 165.1)];
/// 6.7 in.
pub const MINI_ITX_SIDE: f64 = 170.18;
/// ATX: 10× ⌀0.156 in.
pub const ATX_HOLE: f64 = 3.9624;

fn mini_itx(v: &Values) -> Result<Made, String> {
    let t = v.positive("thickness")?;
    let io = v.positive("io_depth")?;
    let side = MINI_ITX_SIDE;
    // The board in XY, its rear edge along y = side, so the holes are at
    // (x, side − y).
    let mut shapes = vec![format!("rect r corner (0, 0) size = ({}, {})", n(side), n(side))];
    for (name, x, y) in MINI_ITX_HOLES {
        shapes.push(circle(name, x, side - y, ATX_HOLE));
    }
    let mut t1 = Text::new("Mini-ITX motherboard", Some("fr4"), &["Mini-ITX: 170.18 mm (6.7 in) square, holes C, F, H and J of ATX 2.2 (Figure 3), ⌀3.96 mm.", "The rear I/O stack: ATX 2.2's aperture, 158.75 × 44.45 mm (6.25 × 1.75 in), starting 13.87 mm in from the left; its depth is the maker's."]);
    t1.sketch("pcb", "XY", &shapes).line(format!("extrude board from pcb distance {}", n(t)));
    // The I/O stack, from 3.81 mm below the board's top to the aperture's
    // top, and as far forward as `io_depth`, inside the board's edge.
    let (x0, x1) = (13.87, side);
    t1.sketch("io", "board.top", &[rect("s", (x0 + x1) / 2.0, side - io / 2.0, x1 - x0, io)]).line("extrude rear_io from io distance 40.64");
    Ok(Made { stem: "mini-itx".into(), text: t1.0 })
}

fn sfx_psu(v: &Values) -> Result<Made, String> {
    let deep = if v.text("size") == "sfx-l" { 130.0 } else { 100.0 };
    let mut t = Text::new(if deep > 100.0 { "SFX-L power supply" } else { "SFX power supply" }, Some("steel"), &["SFX12V: 125 wide × 63.5 high × 100 deep (SFX-L 130 deep), mm."]);
    t.sketch("box", "XY", &[rect("r", 0.0, deep / 2.0, 125.0, deep)]).line("extrude psu from box distance 63.5");
    // Its fan, on the top, as a grille 92 across: a shallow recess.
    t.sketch("grille", "psu.top", &[circle("f", 0.0, deep / 2.0, if deep > 100.0 { 120.0 } else { 92.0 })]).line("extrude fan from grille distance -1 cut psu");
    Ok(Made { stem: if deep > 100.0 { "sfx-l-psu".into() } else { "sfx-psu".into() }, text: t.0 })
}

fn m2(v: &Values) -> Result<Made, String> {
    let len: f64 = v.text("length")[2..].parse().map_err(|_| "a length like 2280")?;
    let th = v.positive("thickness")?;
    let mut t = Text::new(&format!("M.2 {} drive", v.text("length")), None, &["M.2: 22 mm wide; the length is in its name (2280: 80 mm)."]);
    t.sketch("card", "XY", &[rect("r", len / 2.0, 0.0, len, 22.0)]).line(format!("extrude drive from card distance {}", n(th)));
    Ok(Made { stem: format!("m2-{}", v.text("length")), text: t.0 })
}

fn dimm(v: &Values) -> Result<Made, String> {
    let (h, th) = (v.positive("height")?, v.positive("thickness")?);
    let mut t = Text::new("Memory module", None, &["A DIMM: 133.35 mm long (5.25 in); its height and its heatspreader's thickness are the maker's."]);
    t.sketch("side", "XZ", &[rect("r", 0.0, h / 2.0, 133.35, h)]).line(format!("extrude dimm from side distance {} symmetric", n(th)));
    Ok(Made { stem: format!("dimm-{}", n(h)), text: t.0 })
}

/* ── graphics ─────────────────────────────────────────────────────── */

/// A slot's pitch: 0.8 in.
pub const SLOT: f64 = 20.32;

fn gpu(v: &Values) -> Result<Made, String> {
    let (len, h, slots) = (v.positive("length")?, v.positive("height")?, v.positive("slots")?);
    let ports = v.num("ports")?.round().max(0.0) as usize;
    let thick = slots * SLOT - 1.0;
    let bracket = slots.ceil() * SLOT;
    let mut t = Text::new(&format!("Graphics card {}×{}, {} slot", n(len), n(h), n(slots)), None, &[
        "A card by its published length (from the bracket's face), height and thickness in slots of 20.32 mm.",
        "x along it from the bracket, y across its slots, z up from the slot's edge.",
    ]);
    // The cooler and board as one block; the bracket a plate at x = 0, a
    // full-height bracket's 120.02 mm.
    t.sketch("body", "XZ", &[rect("r", len / 2.0, h / 2.0, len, h)]).line(format!("extrude card from body distance {}", n(thick)));
    // On YZ the sketch's x is the world's y: the card runs to y = −thick.
    t.sketch("plate", "YZ", &[rect("r", -thick / 2.0, 120.02 / 2.0, bracket, 120.02)]).line("extrude bracket from plate distance -1 join card");
    if ports > 0 {
        // G1/4 ports on its top edge, at their tap drill, 8 deep.
        let holes: Vec<String> = (0..ports).map(|k| circle(&format!("p{}", k + 1), len - 40.0 - 22.0 * k as f64, -thick / 2.0, 11.8)).collect();
        t.sketch("ports", &format!("XY offset {}", n(h)), &holes).line("extrude port from ports distance -8 cut card");
    }
    Ok(Made { stem: stem(&format!("gpu-{}x{}-{}slot", n(len), n(h), n(slots))), text: t.0 })
}

/* ── cooling ──────────────────────────────────────────────────────── */

/// A fan's screw holes apart, by its size: 80 → 71.5, 92 → 82.5, 120 → 105,
/// 140 → 124.5 mm.
pub fn fan_spacing(size: f64) -> Option<f64> {
    match size as u32 {
        80 => Some(71.5),
        92 => Some(82.5),
        120 => Some(105.0),
        140 => Some(124.5),
        _ => None,
    }
}

fn fan(v: &Values) -> Result<Made, String> {
    let (size, th) = (v.positive("size")?, v.positive("thickness")?);
    let s = fan_spacing(size).ok_or("fans are 80, 92, 120 or 140")?;
    let mut shapes = vec![rect("r", 0.0, 0.0, size, size), circle("bore", 0.0, 0.0, size - 4.0)];
    for (k, (x, y)) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().enumerate() {
        shapes.push(circle(&format!("h{}", k + 1), x * s / 2.0, y * s / 2.0, 4.3));
    }
    let mut t = Text::new(&format!("Fan {} × {}", n(size), n(th)), Some("pbt"), &["A fan's frame, its blades' opening and its hub; screw holes ⌀4.3 at its size's spacing (80: 71.5, 92: 82.5, 120: 105, 140: 124.5 mm)."]);
    t.sketch("frame", "XY", &shapes).line(format!("extrude fan from frame distance {}", n(th)));
    t.sketch("motor", "XY", &[circle("hub", 0.0, 0.0, (size * 0.33).round())]).line(format!("extrude hub from motor distance {} join fan", n(th)));
    Ok(Made { stem: format!("fan-{}x{}", n(size), n(th)), text: t.0 })
}

fn radiator(v: &Values) -> Result<Made, String> {
    let fans = v.positive("fans")?.round() as usize;
    let fan = v.positive("fan")?;
    let th = v.positive("thickness")?;
    let w = if v.text("width").trim().is_empty() { fan + 4.0 } else { v.positive("width")? };
    let tank = v.positive("tanks")?;
    let ps = v.positive("port_spacing")?;
    let s = fan_spacing(fan).ok_or("radiator fans are 120 or 140")?;
    let len = fans as f64 * fan + 2.0 * tank;
    let mut t = Text::new(&format!("Radiator {} × {}", fans as f64 * fan, n(th)), Some("copper"), &[
        "A radiator: fans on its top, M3 holes at the fans' spacing; two G1/4 ports in one end tank, on its back.",
        "The end tanks' length and the ports' spacing are makers': check them against yours.",
    ]);
    t.sketch("core", "XY", &[rect("r", 0.0, 0.0, len, w)]).line(format!("extrude rad from core distance {}", n(th)));
    let mut holes = Vec::new();
    for k in 0..fans {
        let cx = -(fans as f64 * fan) / 2.0 + fan / 2.0 + k as f64 * fan;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            holes.push(format!("({}, {})", n(cx + x * s / 2.0), n(y * s / 2.0)));
        }
    }
    t.line(format!("hole fan_holes on rad.top at {} thread M3 depth 5", holes.join(", ")));
    // The ports, on the back of the tank at +x: the bottom's frame faces
    // down, its x the sketch's, its y turned over.
    let px = len / 2.0 - tank / 2.0;
    t.line(format!("hole ports on rad.bottom at ({}, {}), ({}, {}) thread G1/4 depth 8", n(px), n(ps / 2.0), n(px), n(-ps / 2.0)));
    Ok(Made { stem: format!("radiator-{}x{}", fans as f64 * fan, n(th)), text: t.0 })
}

fn cpu_block(v: &Values) -> Result<Made, String> {
    let (w, h, ps) = (v.positive("width")?, v.positive("height")?, v.positive("port_spacing")?);
    let mut t = Text::new("CPU water block", Some("copper"), &["A CPU block's envelope: a base plate and a top, two G1/4 ports on top. Makers differ: check yours."]);
    t.sketch("base", "XY", &[rect("r", 0.0, 0.0, w, w)]).line("extrude plate from base distance 6");
    t.sketch("top", "plate.top", &[circle("c", 0.0, 0.0, w - 8.0)]).line(format!("extrude cap from top distance {} join plate", n(h - 6.0)));
    t.line(format!("hole ports on cap.top at (0, {}), (0, {}) thread G1/4 depth 8", n(ps / 2.0), n(-ps / 2.0)));
    Ok(Made { stem: "cpu-block".into(), text: t.0 })
}

fn pump(v: &Values) -> Result<Made, String> {
    let ddc = v.text("kind") == "ddc";
    let res = v.num("reservoir")?.max(0.0);
    let mut t = Text::new(if ddc { "DDC pump" } else { "D5 pump" }, None, &["A pump's envelope (D5: ⌀62 × 38 mm; DDC: 62 × 62 × 38 mm) with an outlet on its side; a reservoir, if any, on its top, 60 across."]);
    if ddc {
        t.sketch("foot", "XY", &[rect("r", 0.0, 0.0, 62.0, 62.0)]);
    } else {
        t.sketch("foot", "XY", &[circle("c", 0.0, 0.0, 62.0)]);
    }
    t.line("extrude pump from foot distance 38");
    if res > 0.0 {
        t.sketch("tube", "pump.top", &[circle("c", 0.0, 0.0, 60.0)]).line(format!("extrude res from tube distance {} join pump", n(res)));
        t.line("hole inlet on res.top at (0, 0) thread G1/4 depth 8");
    } else {
        t.line("hole inlet on pump.top at (0, 0) thread G1/4 depth 8");
    }
    Ok(Made { stem: if res > 0.0 { format!("{}-res-{}", if ddc { "ddc" } else { "d5" }, n(res)) } else if ddc { "ddc".into() } else { "d5".into() }, text: t.0 })
}

/// A G1/4 thread: BSPP 1/4, its major diameter 13.157 mm.
pub const G14: f64 = 13.157;

/// A tube's outside diameter by its name, `14/16` → 16.
fn tube_od(tube: &str) -> Option<f64> {
    tube.split_once('/').and_then(|(_, od)| od.parse().ok())
}

fn fitting(v: &Values) -> Result<Made, String> {
    let kind = v.text("kind");
    let tube = v.text("tube");
    let od = tube_od(tube);
    let mut t = Text::new(&format!("G1/4 fitting, {kind}{}", od.map_or(String::new(), |_| format!(", {tube} tube"))), Some("brass"), &[
        "A G1/4 fitting's envelope: its thread 13.157 across and 5 long below z = 0; above, its body, 22 across.",
        "A tube's socket is `NAME.socket`: its axis is where the tube goes. Makers differ by a few mm: check yours.",
    ]);
    t.sketch("thread", "XY", &[circle("c", 0.0, 0.0, G14)]).line("extrude male from thread distance -5");
    let body = 22.0;
    match kind {
        "plug" => {
            t.sketch("head", "XY", &[circle("c", 0.0, 0.0, body)]).line("extrude cap from head distance 4 join male");
        }
        "extender" => {
            let len = v.positive("length")?;
            t.sketch("head", "XY", &[circle("c", 0.0, 0.0, 18.0)]).line(format!("extrude ext from head distance {} join male", n(len)));
            t.line("hole socket on ext.top at (0, 0) thread G1/4 depth 6");
        }
        "straight" => {
            let od = od.ok_or("a fitting for a tube says which: 14/16, 12/16 or 10/13")?;
            t.sketch("base", "XY", &[circle("c", 0.0, 0.0, body)]).line("extrude body from base distance 22 join male");
            t.line(format!("hole socket on body.top at (0, 0) diameter {} depth 10", n(od)));
        }
        "elbow" => {
            let od = od.ok_or("a fitting for a tube says which: 14/16, 12/16 or 10/13")?;
            // A collar, a block on it, and the socket out of the block's
            // side, square to the thread.
            t.sketch("base", "XY", &[circle("c", 0.0, 0.0, body)]).line("extrude body from base distance 6 join male");
            let (side, up) = (20.0, 22.0);
            t.sketch("block", "body.top", &[rect("r", 0.0, 0.0, side, side)]).line(format!("extrude bend from block distance {} join male", n(up)));
            t.sketch("out", "bend.side(r.right)", &[circle("c", side / 2.0, up / 2.0, od + 2.0)]).line("extrude nut from out distance 10 join male");
            t.line(format!("hole socket on nut.top at (0, 0) diameter {} depth 10", n(od)));
        }
        other => return Err(format!("“{other}” is not a fitting")),
    }
    let tube_part = if od.is_some() && kind != "plug" && kind != "extender" { format!("-{}", tube.replace('/', "-")) } else { String::new() };
    Ok(Made { stem: format!("g14-{kind}{tube_part}"), text: t.0 })
}

/* ── fasteners ────────────────────────────────────────────────────── */

/// ISO 4762 socket head: head ⌀ and height; ISO 7380 button; ISO 10642 flat.
pub fn screw_head(size: &str, head: &str) -> Option<(f64, f64)> {
    let i = ["M2", "M2.5", "M3", "M4", "M5"].iter().position(|s| *s == size)?;
    Some(match head {
        "socket" => [(3.8, 2.0), (4.5, 2.5), (5.5, 3.0), (7.0, 4.0), (8.5, 5.0)][i],
        "button" => [(3.5, 1.3), (4.7, 1.5), (5.7, 1.65), (7.6, 2.2), (9.5, 2.75)][i],
        "flat" => [(4.4, 1.2), (5.5, 1.5), (6.72, 1.86), (8.96, 2.48), (11.2, 3.1)][i],
        _ => return None,
    })
}

fn metric(size: &str) -> Option<f64> {
    size.strip_prefix('M')?.parse().ok()
}

fn screw(v: &Values) -> Result<Made, String> {
    let size = v.text("size");
    let head = v.text("head");
    let d = metric(size).ok_or("a size like M3")?;
    let len = v.positive("length")?;
    let (dk, k) = screw_head(size, head).ok_or("an ISO head: socket, button or flat")?;
    let mut t = Text::new(&format!("{size} × {} {head} screw", n(len)), Some("steel"), &["ISO 4762 socket, ISO 7380 button or ISO 10642 flat head; the shank to the thread's diameter, the length under the head (all of it, for a flat head)."]);
    t.sketch("shank", "XY", &[circle("c", 0.0, 0.0, d)]).line(format!("extrude screw from shank distance -{}", n(len)));
    if head == "flat" {
        // A 90° cone, its top at z = 0, down to the shank.
        t.sketch("cone", "XZ", &[
            "line a (0, 0) (".to_string() + &n(dk / 2.0) + ", 0)",
            format!("line b a.end ({}, {})", n(d / 2.0), n(-(dk - d) / 2.0)),
            format!("line c b.end (0, {})", n(-(dk - d) / 2.0)),
            "line e c.end a.start".to_string(),
        ]).line("revolve head from cone about e join screw");
    } else {
        t.sketch("top", "XY", &[circle("c", 0.0, 0.0, dk)]).line(format!("extrude head from top distance {} join screw", n(k)));
    }
    Ok(Made { stem: format!("{}x{}-{head}", size.to_ascii_lowercase(), n(len)), text: t.0 })
}

fn nut(v: &Values) -> Result<Made, String> {
    let size = v.text("size");
    let i = ["M2", "M2.5", "M3", "M4", "M5"].iter().position(|s| *s == size).ok_or("a size like M3")?;
    let (s, m) = [(4.0, 1.6), (5.0, 2.0), (5.5, 2.4), (7.0, 3.2), (8.0, 4.7)][i];
    let d = metric(size).ok_or("a size like M3")?;
    let mut t = Text::new(&format!("{size} hex nut"), Some("steel"), &["ISO 4032: across flats and thickness by size; the hole at the thread's diameter."]);
    t.sketch("hex", "XY", &hexagon(s)).line(format!("extrude nut from hex distance {}", n(m)));
    t.line(format!("hole bore on nut.top at (0, 0) diameter {} through", n(d)));
    Ok(Made { stem: format!("{}-nut", size.to_ascii_lowercase()), text: t.0 })
}

/// A hexagon `s` across its flats, centred, as six lines.
fn hexagon(s: f64) -> Vec<String> {
    let r = s / 3f64.sqrt();
    let pts: Vec<(f64, f64)> = (0..6).map(|k| {
        let a = (k as f64 * 60.0).to_radians();
        (r * a.cos(), r * a.sin())
    }).collect();
    (0..6)
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % 6]);
            if k == 0 {
                format!("line e0 ({}, {}) ({}, {})", n(a.0), n(a.1), n(b.0), n(b.1))
            } else if k == 5 {
                "line e5 e4.end e0.start".to_string()
            } else {
                format!("line e{k} e{}.end ({}, {})", k - 1, n(b.0), n(b.1))
            }
        })
        .collect()
}

fn standoff(v: &Values) -> Result<Made, String> {
    let six32 = v.text("thread") == "6-32";
    let len = v.positive("length")?;
    // 6-32: 3/16 in (4.76 mm) across its flats; M3: 5.5.
    let (s, d) = if six32 { (4.7625, 3.505) } else { (5.5, 3.0) };
    let mut t = Text::new(&format!("{} standoff {}", if six32 { "6-32" } else { "M3" }, n(len)), Some("brass"), &["A male–female standoff: a hex body, a stud 5 long below, a tapped hole above."]);
    t.sketch("hex", "XY", &hexagon(s)).line(format!("extrude body from hex distance {}", n(len)));
    t.sketch("pin", "XY", &[circle("c", 0.0, 0.0, d)]).line("extrude stud from pin distance -5 join body");
    t.line(format!("hole tap on body.top at (0, 0) thread {} depth {}", if six32 { "6-32" } else { "M3" }, n((len - 1.0).min(5.0))));
    Ok(Made { stem: format!("standoff-{}-{}", if six32 { "6-32" } else { "m3" }, n(len)), text: t.0 })
}
