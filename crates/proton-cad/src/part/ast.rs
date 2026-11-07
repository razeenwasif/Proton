//! What a `.part` file says, kept as it was written — its comments and
//! blank lines too — so that it prints back the same.

use std::fmt;

/// A number worked out from parameters: `t`, `80 - 2 * t`, `sqrt(2) * w`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Name(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// A statement, the comment written after it, and the line it was read
/// from (nothing to compare: a statement is the same wherever it is).
#[derive(Debug, Clone)]
pub struct Noted<T> {
    pub item: T,
    pub note: Option<String>,
    pub line: usize,
}

impl<T> Noted<T> {
    pub fn new(item: T) -> Noted<T> {
        Noted { item, note: None, line: 0 }
    }
}

impl<T: PartialEq> PartialEq for Noted<T> {
    fn eq(&self, o: &Noted<T>) -> bool {
        self.item == o.item && self.note == o.note
    }
}

/// A whole file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Part {
    pub items: Vec<Noted<Item>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Blank,
    /// `# …`, the text after the `#`.
    Comment(String),
    /// `part "Pump bracket"`
    Name(String),
    /// `units mm`
    Units(Units),
    /// `material aluminium-6061`, as written.
    Material(String),
    /// `param t = 2`
    Param { name: String, value: Expr },
    Sketch(Sketch),
    /// A feature — `extrude`, `hole`, `fillet` … — kept as written until
    /// the milestone that builds it reads it.
    Feature { keyword: String, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Mm,
    Cm,
    M,
    In,
}

impl Units {
    pub fn word(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::M => "m",
            Units::In => "in",
        }
    }

    /// Millimetres in one of them.
    pub fn mm(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Cm => 10.0,
            Units::M => 1000.0,
            Units::In => 25.4,
        }
    }
}

/// `sketch NAME on PLACE … end`
#[derive(Debug, Clone, PartialEq)]
pub struct Sketch {
    pub name: String,
    pub on: Placement,
    pub items: Vec<Noted<SketchItem>>,
    /// The comment after its `end`.
    pub end_note: Option<String>,
}

/// Where a sketch is drawn: `XY`, `XZ`, `YZ`, or a face — `plate.top` —
/// moved along its normal by `offset`.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub reference: String,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SketchItem {
    Blank,
    Comment(String),
    Entity(Entity),
    Constraint(Constraint),
}

/// Something drawn. `construction` geometry guides the rest and is never
/// part of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub construction: bool,
    pub name: String,
    pub shape: Shape,
}

/// A point of a shape: where it is, `(x, y)`, or a point already drawn,
/// `a.end` — then the two are one point.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    At(f64, f64),
    Ref(String),
}

/// A circle's or an arc's size, as a dimension.
#[derive(Debug, Clone, PartialEq)]
pub enum Size {
    Radius(Expr),
    Diameter(Expr),
}

/// The shapes. Numbers in a shape say where it is now; a value after `=`
/// is a dimension, which the solver keeps.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// `point p (x, y)`
    Point { at: (f64, f64) },
    /// `line a SPOT SPOT [length = EXPR]`
    Line { start: Spot, end: Spot, length: Option<Expr> },
    /// `circle c center SPOT radius R` or `… radius = EXPR` / `diameter = EXPR`
    Circle { center: Spot, radius: f64, size: Option<Size> },
    /// `arc k center SPOT from SPOT to SPOT [radius = EXPR]`, anticlockwise
    /// from its start to its end.
    Arc { center: Spot, start: Spot, end: Spot, size: Option<Size> },
    /// `rect r corner SPOT size (W, H)` or `size = (EXPR, EXPR)`: four lines,
    /// kept square to the axes.
    Rect { corner: Spot, size: (f64, f64), dims: Option<(Expr, Expr)> },
    /// `spline s SPOT SPOT …`: a cubic B-spline on these control points.
    Spline { points: Vec<Spot> },
}

/// A constraint or a dimension: `tangent a c`, `distance p q = 20`.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub kind: Kind,
    pub refs: Vec<String>,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Coincident,
    On,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Midpoint,
    /// `symmetric A B about L`
    Symmetric,
    Concentric,
    Fix,
    Distance,
    HDistance,
    VDistance,
    Length,
    Radius,
    Diameter,
    /// In degrees.
    Angle,
}

impl Kind {
    pub const ALL: [Kind; 19] = [
        Kind::Coincident,
        Kind::On,
        Kind::Horizontal,
        Kind::Vertical,
        Kind::Parallel,
        Kind::Perpendicular,
        Kind::Tangent,
        Kind::Equal,
        Kind::Midpoint,
        Kind::Symmetric,
        Kind::Concentric,
        Kind::Fix,
        Kind::Distance,
        Kind::HDistance,
        Kind::VDistance,
        Kind::Length,
        Kind::Radius,
        Kind::Diameter,
        Kind::Angle,
    ];

    pub fn word(self) -> &'static str {
        match self {
            Kind::Coincident => "coincident",
            Kind::On => "on",
            Kind::Horizontal => "horizontal",
            Kind::Vertical => "vertical",
            Kind::Parallel => "parallel",
            Kind::Perpendicular => "perpendicular",
            Kind::Tangent => "tangent",
            Kind::Equal => "equal",
            Kind::Midpoint => "midpoint",
            Kind::Symmetric => "symmetric",
            Kind::Concentric => "concentric",
            Kind::Fix => "fix",
            Kind::Distance => "distance",
            Kind::HDistance => "hdistance",
            Kind::VDistance => "vdistance",
            Kind::Length => "length",
            Kind::Radius => "radius",
            Kind::Diameter => "diameter",
            Kind::Angle => "angle",
        }
    }

    pub fn from_word(w: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.word() == w)
    }

    /// How many things it names, fewest and most.
    pub fn arity(self) -> (usize, usize) {
        match self {
            Kind::Horizontal | Kind::Vertical => (1, 2),
            Kind::Fix | Kind::Length | Kind::Radius | Kind::Diameter => (1, 1),
            Kind::Symmetric => (3, 3),
            _ => (2, 2),
        }
    }

    /// Whether it is a dimension, with a value after `=`.
    pub fn valued(self) -> bool {
        matches!(self, Kind::Distance | Kind::HDistance | Kind::VDistance | Kind::Length | Kind::Radius | Kind::Diameter | Kind::Angle)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// A mistake in a file, where it is: line and column from 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Diag {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl Diag {
    pub fn new(line: usize, col: usize, message: impl Into<String>) -> Diag {
        Diag { line, col, message: message.into() }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}
