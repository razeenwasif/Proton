//! Drawings: a part's or an assembly's views on a sheet — projected from
//! the exact geometry, hidden lines found, sections hatched, dimensioned,
//! with a title block, and for an assembly balloons and a bill of
//! materials — written as SVG, PDF and DXF (docs/CAD.md, K10).
//!
//! - [`view`]: a view projected, its hidden lines found.
//! - [`lang`]: the `.drawing` language.
//! - [`build`]: a drawing laid out on its sheet.
//! - [`out`]: the sheet as SVG, PDF and DXF.

pub mod build;
pub mod lang;
pub mod out;
pub mod view;

/// A point on the sheet, millimetres, from its bottom left, y up.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

impl P2 {
    pub const fn new(x: f64, y: f64) -> P2 {
        P2 { x, y }
    }

    pub fn distance(self, o: P2) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }
}

impl std::ops::Add for P2 {
    type Output = P2;
    fn add(self, o: P2) -> P2 {
        P2::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for P2 {
    type Output = P2;
    fn sub(self, o: P2) -> P2 {
        P2::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for P2 {
    type Output = P2;
    fn mul(self, k: f64) -> P2 {
        P2::new(self.x * k, self.y * k)
    }
}

/// How a line is drawn: ISO 128's kinds, each a layer in a DXF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    /// Edges and outlines seen: thick, continuous.
    Visible,
    /// Edges behind: thin, dashed.
    Hidden,
    /// Axes and centres: thin, long dash and dot.
    Center,
    /// Hatching, leaders: thin, continuous.
    Thin,
    /// Dimension lines, arrows and their text.
    Dim,
    /// The sheet's frame, title block and tables.
    Frame,
}

impl Pen {
    pub fn width(self) -> f64 {
        match self {
            Pen::Visible => 0.5,
            Pen::Frame => 0.5,
            _ => 0.25,
        }
    }

    pub fn layer(self) -> &'static str {
        match self {
            Pen::Visible => "VISIBLE",
            Pen::Hidden => "HIDDEN",
            Pen::Center => "CENTER",
            Pen::Thin => "THIN",
            Pen::Dim => "DIM",
            Pen::Frame => "FRAME",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// What a sheet is drawn with.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Line { a: P2, b: P2, pen: Pen },
    /// A run of lines; closed, back to its first point.
    Poly { pts: Vec<P2>, closed: bool, pen: Pen },
    Circle { c: P2, r: f64, pen: Pen },
    /// Text `size` high, its baseline at `at`, turned `angle` degrees.
    Text { at: P2, size: f64, text: String, anchor: Anchor, angle: f64, pen: Pen },
    /// A filled shape: an arrowhead.
    Fill { pts: Vec<P2>, pen: Pen },
}

/// Paper (black on white) or blueprint (white on blue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Paper,
    Blueprint,
}

/// A sheet drawn: its size, millimetres, and what is on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub style: Style,
    pub prims: Vec<Prim>,
    /// Where each view, dimension and table is, by name: for the window to
    /// pick them.
    pub boxes: Vec<(String, [f64; 4])>,
}

/// A sheet size by name, landscape: ISO A4–A0, ANSI A–D.
pub fn size(name: &str) -> Option<(f64, f64)> {
    Some(match name.to_ascii_uppercase().as_str() {
        "A4" => (297.0, 210.0),
        "A3" => (420.0, 297.0),
        "A2" => (594.0, 420.0),
        "A1" => (841.0, 594.0),
        "A0" => (1189.0, 841.0),
        "ANSI-A" | "LETTER" => (279.4, 215.9),
        "ANSI-B" | "TABLOID" => (431.8, 279.4),
        "ANSI-C" => (558.8, 431.8),
        "ANSI-D" => (863.6, 558.8),
        _ => return None,
    })
}
