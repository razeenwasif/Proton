//! K1: the `.part` language read and printed back unchanged, and its
//! mistakes named where they are.

mod common;

use common::Rng;
use proton_cad::part::ast::*;
use proton_cad::part::{self, expr, parse, print};

const BRACKET: &str = r#"# A bracket for the pump, under the radiator.
part "Pump bracket \"B\""
units mm
material aluminium-6061
param t = 2
param hole = 3.4  # an M3 clearance hole
param w = 80
param inset = (w - 60) / 2

sketch base on XY
  rect r corner origin size = (w, 40)
  circle h1 center (10, 10) diameter = hole
  circle h2 center (70, 10) radius 1.7

  # the slot
  arc left center (20, 30) from (20, 35) to (20, 25) radius = 5
  line bottom left.end (60, 25)
  arc right center (60, 30) from bottom.end to (60, 35)
  line top right.end left.start length = 40
  construction line mid (40, -5) (40, 45)
  point tab (40, 0)
  spline edge (0, 50) (20, 55) (60, 45) (80, 50)
  equal h1 h2
  horizontal h1.center h2.center
  hdistance r.bl h1.center = inset
  symmetric h1.center h2.center about mid
  tangent bottom left  # both arcs
  tangent bottom right
  angle top r.bottom = 0
  distance tab r.bottom = 0
  fix r
end

sketch lid on base.top offset -t * 2
  circle boss center origin radius = 6
  diameter boss = sqrt(144) + min(1, 2) - 1
end

extrude plate from base distance t
revolve boss from lid about xaxis angle 90 / 2
extrude rib from lid distance -t symmetric
hole m3 on plate.top at h1.center, h2.center thread M3 through
fillet plate.edges(parallel z) radius 2
"#;

#[test]
fn a_part_prints_back_as_it_was_written() {
    let p = part::read(BRACKET).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(part::write(&p), BRACKET);
    // And what it read is what it printed.
    assert_eq!(part::read(&part::write(&p)).unwrap(), p);
    // What it says, by item.
    let sketches: Vec<&Sketch> = p.items.iter().filter_map(|n| if let Item::Sketch(s) = &n.item { Some(s) } else { None }).collect();
    assert_eq!(sketches.len(), 2);
    assert_eq!(sketches[1].on.reference, "base.top");
    assert_eq!(print::expr(sketches[1].on.offset.as_ref().unwrap()), "-t * 2");
    let features: Vec<&str> = p.items.iter().filter_map(|n| if let Item::Feature { keyword, .. } = &n.item { Some(keyword.as_str()) } else { None }).collect();
    assert_eq!(features, ["hole", "fillet"]);
    let extrudes: Vec<&Extrude> = p.items.iter().filter_map(|n| if let Item::Extrude(e) = &n.item { Some(e) } else { None }).collect();
    assert_eq!(
        extrudes,
        [&Extrude { name: "plate".into(), sketch: "base".into(), distance: Expr::Name("t".into()), symmetric: false }, &Extrude { name: "rib".into(), sketch: "lid".into(), distance: Expr::Neg(Box::new(Expr::Name("t".into()))), symmetric: true }]
    );
    let revolves: Vec<&Revolve> = p.items.iter().filter_map(|n| if let Item::Revolve(r) = &n.item { Some(r) } else { None }).collect();
    assert_eq!(revolves.len(), 1);
    assert_eq!((revolves[0].axis.as_str(), print::expr(revolves[0].angle.as_ref().unwrap()).as_str()), ("xaxis", "90 / 2"));
    let notes: Vec<&str> = p.items.iter().filter_map(|n| n.note.as_deref()).collect();
    assert_eq!(notes, [" an M3 clearance hole"]);
    // Parameters, worked out in order.
    let (env, wrong) = expr::params(&p);
    assert!(wrong.is_empty(), "{wrong:?}");
    assert_eq!(env["inset"], 10.0);
}

#[test]
fn an_expression_prints_with_only_the_brackets_it_needs() {
    for (text, want) in [
        ("1 + 2 * 3", "1 + 2 * 3"),
        ("(1 + 2) * 3", "(1 + 2) * 3"),
        ("a - (b - c)", "a - (b - c)"),
        ("(a - b) - c", "a - b - c"),
        ("a / (b * c)", "a / (b * c)"),
        ("-(a + b)", "-(a + b)"),
        ("-a * b", "-a * b"),
        ("--a", "--a"),
        ("max(a, b + 1, 2)", "max(a, b + 1, 2)"),
        ("((w))", "w"),
    ] {
        let e = parse::expr(text).unwrap();
        assert_eq!(print::expr(&e), want, "{text}");
        assert_eq!(parse::expr(want).unwrap(), e, "{text}");
    }
}

fn random_expr(rng: &mut Rng, depth: u32) -> Expr {
    let leaf = depth == 0 || rng.unit() < 0.3;
    if leaf {
        return if rng.unit() < 0.5 { Expr::Num((rng.range(0.0, 1000.0) * 1000.0).round() / 1000.0) } else { Expr::Name(["t", "w", "hole", "h_2"][(rng.next_u64() % 4) as usize].into()) };
    }
    match rng.next_u64() % 6 {
        0 => Expr::Neg(Box::new(random_expr(rng, depth - 1))),
        1 => Expr::Call(["sqrt", "abs", "min", "max"][(rng.next_u64() % 4) as usize].into(), (0..1 + rng.next_u64() % 3).map(|_| random_expr(rng, depth - 1)).collect()),
        k => {
            let op = [Op::Add, Op::Sub, Op::Mul, Op::Div][(k - 2) as usize];
            Expr::Bin(op, Box::new(random_expr(rng, depth - 1)), Box::new(random_expr(rng, depth - 1)))
        }
    }
}

#[test]
fn five_thousand_random_expressions_read_back_unchanged() {
    let mut rng = Rng(41);
    for _ in 0..5000 {
        let e = random_expr(&mut rng, 5);
        let text = print::expr(&e);
        let back = parse::expr(&text).unwrap_or_else(|d| panic!("{text}: {d}"));
        assert_eq!(back, e, "{text}");
    }
}

fn random_spot(rng: &mut Rng, known: &[String]) -> Spot {
    if !known.is_empty() && rng.unit() < 0.3 {
        Spot::Ref(known[(rng.next_u64() as usize) % known.len()].clone())
    } else {
        let c = |rng: &mut Rng| ((rng.range(-500.0, 500.0)) * 1e6).round() / 1e6;
        Spot::At(c(rng), c(rng))
    }
}

#[test]
fn random_sketches_read_back_unchanged() {
    let mut rng = Rng(42);
    for _ in 0..300 {
        let mut items = Vec::new();
        let mut points: Vec<String> = vec!["origin".into()];
        let mut names: Vec<String> = vec!["xaxis".into(), "yaxis".into()];
        for k in 0..rng.next_u64() % 25 {
            let name = format!("e{k}");
            let construction = rng.unit() < 0.15;
            let shape = match rng.next_u64() % 6 {
                0 => Shape::Point { at: (1.5, -2.25) },
                1 => Shape::Line { start: random_spot(&mut rng, &points), end: random_spot(&mut rng, &points), length: (rng.unit() < 0.3).then(|| random_expr(&mut rng, 2)) },
                2 => Shape::Circle { center: random_spot(&mut rng, &points), radius: 3.5, size: (rng.unit() < 0.5).then(|| if rng.unit() < 0.5 { Size::Radius(random_expr(&mut rng, 2)) } else { Size::Diameter(random_expr(&mut rng, 2)) }) },
                3 => Shape::Arc { center: random_spot(&mut rng, &points), start: random_spot(&mut rng, &points), end: random_spot(&mut rng, &points), size: None },
                4 => Shape::Rect { corner: random_spot(&mut rng, &points), size: (10.0, -4.5), dims: (rng.unit() < 0.5).then(|| (random_expr(&mut rng, 1), random_expr(&mut rng, 1))) },
                _ => Shape::Spline { points: (0..2 + rng.next_u64() % 4).map(|_| random_spot(&mut rng, &points)).collect() },
            };
            let shape = match shape {
                // A circle drawn with a dimension has no radius of its own in the text.
                Shape::Circle { center, size: Some(s), .. } => Shape::Circle { center, radius: 0.0, size: Some(s) },
                Shape::Rect { corner, dims: Some(d), .. } => Shape::Rect { corner, size: (0.0, 0.0), dims: Some(d) },
                s => s,
            };
            match &shape {
                Shape::Line { .. } => points.extend([format!("{name}.start"), format!("{name}.end")]),
                Shape::Circle { .. } => points.push(format!("{name}.center")),
                _ => {}
            }
            names.push(name.clone());
            items.push(Noted { item: SketchItem::Entity(Entity { construction, name, shape }), note: (rng.unit() < 0.2).then(|| " a note".to_string()), line: 0 });
            if rng.unit() < 0.5 {
                let kind = Kind::ALL[(rng.next_u64() as usize) % Kind::ALL.len()];
                let (lo, hi) = kind.arity();
                let n = lo + (rng.next_u64() as usize) % (hi - lo + 1);
                let refs = (0..n).map(|_| names[(rng.next_u64() as usize) % names.len()].clone()).collect();
                let value = kind.valued().then(|| random_expr(&mut rng, 2));
                items.push(Noted::new(SketchItem::Constraint(Constraint { kind, refs, value })));
            }
            if rng.unit() < 0.1 {
                items.push(Noted::new(if rng.unit() < 0.5 { SketchItem::Blank } else { SketchItem::Comment(" between".into()) }));
            }
        }
        let p = Part { items: vec![Noted::new(Item::Sketch(Sketch { name: "s".into(), on: Placement { reference: "XZ".into(), offset: None }, items, end_note: None }))] };
        let text = part::write(&p);
        let back = part::read(&text).unwrap_or_else(|d| panic!("{d}\n{text}"));
        assert_eq!(back, p, "{text}");
        assert_eq!(part::write(&back), text);
    }
}

#[test]
fn each_mistake_is_named_at_its_line_and_column() {
    let cases: [(&str, usize, usize, &str); 15] = [
        ("extrude plate base distance 2\n", 1, 15, "expected `from`"),
        ("extrude plate from base 2\n", 1, 25, "expected `distance`"),
        ("revolve spin from base angle 90\n", 1, 24, "expected `about`"),
        ("units furlongs\n", 1, 7, "not units"),
        ("param = 3\n", 1, 7, "parameter's name"),
        ("param t = 2 +\n", 1, 14, "number, a name"),
        ("sketch s on XY\n  line a (0, 0)\nend\n", 2, 16, "a point"),
        ("sketch s on XY\n  line a (0, 0) (1, 1)\n", 1, 1, "no `end`"),
        ("sketch s on XY\n  squiggle q\nend\n", 2, 3, "neither a shape"),
        ("sketch s on XY\n  tangent a\nend\n", 2, 12, "`tangent` names 2"),
        ("sketch s on XY\n  distance a b\nend\n", 2, 15, "expected `=`"),
        ("sketch s on XY\n  circle c center (0, 0) diameter 4\nend\n", 2, 26, "its `radius`"),
        ("frobnicate the part\n", 1, 1, "not a statement"),
        ("part \"unclosed\n", 1, 6, "no closing"),
        ("sketch s on XY\n  point p (1, 2) extra\nend\n", 2, 18, "more than"),
    ];
    for (text, line, col, says) in cases {
        let d = part::read(text).expect_err(text);
        assert_eq!((d.line, d.col), (line, col), "{text:?}: {d}");
        assert!(d.message.contains(says), "{text:?}: {d}");
    }
}

#[test]
fn a_parameter_that_cannot_be_worked_out_says_why() {
    let p = part::read("param a = 1 / 0\nparam b = c + 1\nparam d = sqrt(-1)\nparam e = 2\nparam e = 3\n").unwrap();
    let (env, wrong) = expr::params(&p);
    assert_eq!(env.len(), 1);
    assert_eq!(env["e"], 2.0);
    let says: Vec<String> = wrong.iter().map(|(l, w)| format!("{l}: {w}")).collect();
    assert_eq!(says, ["1: `a`: a division by nothing", "2: `b`: no parameter is called `c`", "3: `d`: the square root of a negative", "5: `e` is a parameter already"]);
}
