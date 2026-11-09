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
    Extrude(Extrude),
    Revolve(Revolve),
    Hole(Hole),
    Pattern(Pattern),
    Mirror(Mirror),
    Combined(Combined),
    Fillet(Fillet),
    Chamfer(Chamfer),
    Shell(Shell),
    Sweep(SweepAlong),
    Loft(Loft),
    /// A feature — `thread` — kept as written until the milestone that
    /// builds it reads it.
    Feature { keyword: String, text: String },
}

/// How a feature's solid is made into a body already there: joined to it,
/// cut from it, or what the two share kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    Join,
    Cut,
    Intersect,
}

impl Combine {
    pub fn word(self) -> &'static str {
        match self {
            Combine::Join => "join",
            Combine::Cut => "cut",
            Combine::Intersect => "intersect",
        }
    }

    pub fn from_word(w: &str) -> Option<Combine> {
        [Combine::Join, Combine::Cut, Combine::Intersect].into_iter().find(|c| c.word() == w)
    }
}

/// `extrude NAME from SKETCH distance EXPR [symmetric] [join|cut|intersect
/// BODY]`, or `… through …`: the sketch's profile pushed along its normal —
/// the other way for a negative distance, half each way when symmetric,
/// right through everything both ways when `through` — as a body of its
/// own, or made into `BODY`.
#[derive(Debug, Clone, PartialEq)]
pub struct Extrude {
    pub name: String,
    pub sketch: String,
    /// None: through.
    pub distance: Option<Expr>,
    pub symmetric: bool,
    pub into: Option<(Combine, String)>,
}

/// `revolve NAME from SKETCH about LINE [angle EXPR] [join|cut|intersect
/// BODY]`: the profile turned about a line of the sketch — `xaxis`,
/// `yaxis`, or one drawn — by the angle in degrees, all the way round if
/// none is given.
#[derive(Debug, Clone, PartialEq)]
pub struct Revolve {
    pub name: String,
    pub sketch: String,
    pub axis: String,
    pub angle: Option<Expr>,
    pub into: Option<(Combine, String)>,
}

/// A hole's size: a diameter, or the drill for a thread or a screw's
/// clearance by its name — `M3`, `G1/4`, `6-32`.
#[derive(Debug, Clone, PartialEq)]
pub enum HoleSize {
    Diameter(Expr),
    Thread(String),
    Clearance(String),
}

/// A hole's top widened: counterbored to a diameter and depth, or
/// countersunk to a diameter at an angle (90° if none is given).
#[derive(Debug, Clone, PartialEq)]
pub enum Counter {
    Bore { diameter: Expr, depth: Expr },
    Sink { diameter: Expr, angle: Option<Expr> },
}

/// `hole NAME on FACE at P[, P…] (diameter EXPR | thread SIZE | clearance
/// SIZE) (depth EXPR | through) [counterbore EXPR depth EXPR | countersink
/// EXPR [angle EXPR]]`: drilled square into a flat face at each point — a
/// sketch's point by name, or `(x, y)` on the face — and cut from the body
/// the face is on. A blind hole ends in a drill's 118° point.
#[derive(Debug, Clone, PartialEq)]
pub struct Hole {
    pub name: String,
    pub face: String,
    pub at: Vec<Spot>,
    pub size: HoleSize,
    /// None: through.
    pub depth: Option<Expr>,
    pub counter: Option<Counter>,
}

/// How a pattern repeats.
#[derive(Debug, Clone, PartialEq)]
pub enum Repeat {
    /// `linear (x, y, z)`: each copy that far on from the last.
    Linear(Expr, Expr, Expr),
    /// `circular about AXIS [angle EXPR]`: round an axis — `x`, `y`, `z`,
    /// or a sketch's line `SKETCH.LINE` — evenly all the way round, or
    /// spread over the angle.
    Circular { axis: String, angle: Option<Expr> },
}

/// `pattern NAME of FEATURE[, FEATURE…] (linear (x, y, z) | circular about
/// AXIS [angle EXPR]) count N`: the features made again, moved, into the
/// bodies they were made into — `count` in all, the first where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub name: String,
    pub of: Vec<String>,
    pub repeat: Repeat,
    pub count: Expr,
}

/// `mirror NAME of FEATURE[, FEATURE…] about PLANE`: the features made
/// again, mirrored in a plane — `XY`, `XZ`, `YZ`, or a flat face.
#[derive(Debug, Clone, PartialEq)]
pub struct Mirror {
    pub name: String,
    pub of: Vec<String>,
    pub plane: String,
}

/// `union BODY with OTHER`, `cut BODY with OTHER`, `intersect BODY with
/// OTHER`: two bodies made one, `OTHER` used up.
#[derive(Debug, Clone, PartialEq)]
pub struct Combined {
    pub how: Combine,
    pub body: String,
    pub with: String,
}

/// Edges a round or a chamfer takes.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgePick {
    /// `F & G`: where two faces meet.
    Between(String, String),
    /// `BODY.edges` or `FACE.edges`, every edge of it — those that are
    /// corners, not seams or smooth joins — or those `(parallel z)`,
    /// `(convex)`, `(concave)`, `(round)` or `(straight)`, filters all
    /// holding.
    Of { of: String, filters: Vec<EdgeFilter> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeFilter {
    /// Straight and along `x`, `y` or `z`.
    Parallel(String),
    Convex,
    Concave,
    Round,
    Straight,
}

impl EdgeFilter {
    pub fn word(&self) -> String {
        match self {
            EdgeFilter::Parallel(a) => format!("parallel {a}"),
            EdgeFilter::Convex => "convex".into(),
            EdgeFilter::Concave => "concave".into(),
            EdgeFilter::Round => "round".into(),
            EdgeFilter::Straight => "straight".into(),
        }
    }
}

/// `fillet NAME on EDGES radius EXPR`: the edges rounded — a ball of the
/// radius rolled along each, touching the faces either side.
#[derive(Debug, Clone, PartialEq)]
pub struct Fillet {
    pub name: String,
    pub edges: Vec<EdgePick>,
    pub radius: Expr,
}

/// `chamfer NAME on EDGES distance EXPR [and EXPR]`: the edges cut flat
/// that far along each face — the second distance along the second face
/// of `F & G`.
#[derive(Debug, Clone, PartialEq)]
pub struct Chamfer {
    pub name: String,
    pub edges: Vec<EdgePick>,
    pub distance: Expr,
    pub other: Option<Expr>,
}

/// `shell NAME of BODY thickness EXPR [open FACE, FACE…]`: the body
/// hollowed to walls that thick, the faces named taken away.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub name: String,
    pub body: String,
    pub thickness: Expr,
    pub open: Vec<String>,
}

/// `sweep NAME from SKETCH along PATH [join|cut|intersect BODY]`: the
/// profile carried along the lines and arcs of the sketch `PATH`, standing
/// square to it.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepAlong {
    pub name: String,
    pub sketch: String,
    pub path: String,
    pub into: Option<(Combine, String)>,
}

/// `loft NAME from SKETCH to SKETCH [join|cut|intersect BODY]`: a solid
/// between two profiles of as many edges each.
#[derive(Debug, Clone, PartialEq)]
pub struct Loft {
    pub name: String,
    pub from: String,
    pub to: String,
    pub into: Option<(Combine, String)>,
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
