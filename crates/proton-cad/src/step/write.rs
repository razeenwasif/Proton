//! A part's bodies written as STEP AP214: exact, in millimetres.
//!
//! One product, named for the part; its shape a set of solids, one a body,
//! each a `MANIFOLD_SOLID_BREP` — or a `BREP_WITH_VOIDS` where it has voids
//! — named for the body. Every face is an `ADVANCED_FACE` on its own
//! surface, named for what made it, so Proton reads its names back; every
//! edge an `EDGE_CURVE` on its own curve, run its way along it.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::p21::{quote, real};
use crate::brep::{Coedge, Solid};
use crate::curve::Curve;
use crate::frame::Frame;
use crate::surface::Surface;
use crate::vec::Vec3;

struct Out {
    text: String,
    next: u64,
}

impl Out {
    fn add(&mut self, entity: String) -> u64 {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.text, "#{id}={entity};");
        id
    }

    fn point(&mut self, p: Vec3) -> u64 {
        self.add(format!("CARTESIAN_POINT('',({},{},{}))", real(p.x), real(p.y), real(p.z)))
    }

    fn direction(&mut self, d: Vec3) -> u64 {
        self.add(format!("DIRECTION('',({},{},{}))", real(d.x), real(d.y), real(d.z)))
    }

    fn placement(&mut self, f: &Frame) -> u64 {
        let (o, z, x) = (self.point(f.origin), self.direction(f.z), self.direction(f.x));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    fn curve(&mut self, c: &Curve) -> u64 {
        match c {
            Curve::Line(l) => {
                let p = self.point(l.origin);
                let d = self.direction(l.dir);
                let v = self.add(format!("VECTOR('',#{d},1.)"));
                self.add(format!("LINE('',#{p},#{v})"))
            }
            Curve::Circle(k) => {
                let a = self.placement(&k.frame);
                self.add(format!("CIRCLE('',#{a},{})", real(k.radius)))
            }
            Curve::Ellipse(e) => {
                let a = self.placement(&e.frame);
                self.add(format!("ELLIPSE('',#{a},{},{})", real(e.major), real(e.minor)))
            }
            Curve::BSpline(b) => {
                let poles: Vec<String> = b.poles.iter().map(|&p| format!("#{}", self.point(p))).collect();
                // Distinct knots and how often each comes.
                let mut knots: Vec<(f64, usize)> = Vec::new();
                for &k in &b.knots {
                    match knots.last_mut() {
                        Some((x, n)) if *x == k => *n += 1,
                        _ => knots.push((k, 1)),
                    }
                }
                let mults: Vec<String> = knots.iter().map(|k| k.1.to_string()).collect();
                let values: Vec<String> = knots.iter().map(|k| real(k.0)).collect();
                self.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',{},({}),.UNSPECIFIED.,.F.,.F.,({}),({}),.UNSPECIFIED.)", b.degree, poles.join(","), mults.join(","), values.join(",")))
            }
        }
    }

    fn surface(&mut self, s: &Surface) -> u64 {
        if let Surface::Free(f) = s {
            return self.patch(f);
        }
        let a = self.placement(s.frame());
        match s {
            Surface::Plane(_) => self.add(format!("PLANE('',#{a})")),
            Surface::Cylinder(c) => self.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(c.radius))),
            Surface::Cone(c) => self.add(format!("CONICAL_SURFACE('',#{a},{},{})", real(c.radius), real(c.half_angle))),
            Surface::Sphere(sp) => self.add(format!("SPHERICAL_SURFACE('',#{a},{})", real(sp.radius))),
            Surface::Torus(t) => self.add(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(t.major), real(t.minor))),
            Surface::Free(_) => unreachable!("written above"),
        }
    }

    /// A free-form patch, its poles where its frame puts them: a
    /// `B_SPLINE_SURFACE_WITH_KNOTS`, or the complex rational one.
    fn patch(&mut self, f: &crate::surface::Free) -> u64 {
        let p = f.patch();
        let rows: Vec<String> = (0..p.nu).map(|i| {
            let ids: Vec<String> = (0..p.nv).map(|j| format!("#{}", self.point(f.frame.to_world(p.poles[i * p.nv + j])))).collect();
            format!("({})", ids.join(","))
        }).collect();
        let knots = |k: &[f64]| {
            let mut out: Vec<(f64, usize)> = Vec::new();
            for &x in k {
                match out.last_mut() {
                    Some((y, n)) if *y == x => *n += 1,
                    _ => out.push((x, 1)),
                }
            }
            (out.iter().map(|k| k.1.to_string()).collect::<Vec<_>>().join(","), out.iter().map(|k| real(k.0)).collect::<Vec<_>>().join(","))
        };
        let ((mu, ku), (mv, kv)) = (knots(&p.ku), knots(&p.kv));
        match &p.weights {
            None => self.add(format!("B_SPLINE_SURFACE_WITH_KNOTS('',{},{},({}),.UNSPECIFIED.,.F.,.F.,.F.,({mu}),({mv}),({ku}),({kv}),.UNSPECIFIED.)", p.du, p.dv, rows.join(","))),
            Some(w) => {
                let wrows: Vec<String> = (0..p.nu).map(|i| format!("({})", (0..p.nv).map(|j| real(w[i * p.nv + j])).collect::<Vec<_>>().join(","))).collect();
                self.add(format!(
                    "(BOUNDED_SURFACE() B_SPLINE_SURFACE({},{},({}),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS(({mu}),({mv}),({ku}),({kv}),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE())",
                    p.du, p.dv, rows.join(","), wrows.join(",")
                ))
            }
        }
    }
}

/// The volume a shell's faces bound: more than none for a solid's outside,
/// less for a void.
fn shell_volume(s: &Solid, faces: &[usize]) -> f64 {
    let sub = Solid { vertices: s.vertices.clone(), edges: s.edges.clone(), faces: faces.iter().map(|&f| s.faces[f].clone()).collect(), shells: Vec::new() };
    crate::mass::mass(&sub).map_or(0.0, |m| m.volume)
}

/// The bodies, `(name, solid)`, as a STEP file for a part named `part`;
/// `stamp` the file's time, as ISO 8601.
pub fn write(part: &str, bodies: &[(&str, &Solid)], stamp: &str) -> String {
    let mut o = Out { text: String::new(), next: 1 };
    let app = o.add("APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
    o.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let pctx = o.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let product = o.add(format!("PRODUCT({},{},'',(#{pctx}))", quote(part), quote(part)));
    o.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}))"));
    let formation = o.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let dctx = o.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let definition = o.add(format!("PRODUCT_DEFINITION('design','',#{formation},#{dctx})"));
    let shape = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    let mm = o.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = o.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sr = o.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = o.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{mm},'distance_accuracy_value','confusion accuracy')"));
    let ctx = o.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('',''))"));
    let origin = o.placement(&Frame::world());

    let mut items = vec![origin];
    for &(name, s) in bodies {
        // Geometry first, shared where the solid shares it.
        let vertices: Vec<u64> = s.vertices.iter().map(|v| {
            let p = o.point(v.point);
            o.add(format!("VERTEX_POINT('',#{p})"))
        }).collect();
        let edges: Vec<u64> = s.edges.iter().map(|e| {
            let c = o.curve(&e.curve);
            o.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", vertices[e.start], vertices[e.end]))
        }).collect();
        // A loop as oriented edges, run as it is or turned round.
        let mut oriented: HashMap<Coedge, u64> = HashMap::new();
        let mut face = |o: &mut Out, f: usize, flip: bool| -> u64 {
            let fc = &s.faces[f];
            let surf = o.surface(&fc.surface);
            let mut bounds = Vec::new();
            for (k, lp) in fc.loops.iter().enumerate() {
                let run: Vec<Coedge> = if flip { lp.iter().rev().map(|c| Coedge { forward: !c.forward, ..*c }).collect() } else { lp.clone() };
                let refs: Vec<String> = run
                    .iter()
                    .map(|c| {
                        let id = *oriented.entry(*c).or_insert_with(|| o.add(format!("ORIENTED_EDGE('',*,*,#{},{})", edges[c.edge], if c.forward { ".T." } else { ".F." })));
                        format!("#{id}")
                    })
                    .collect();
                let el = o.add(format!("EDGE_LOOP('',({}))", refs.join(",")));
                bounds.push(o.add(format!("{}('',#{el},.T.)", if k == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" })));
            }
            let b: Vec<String> = bounds.iter().map(|x| format!("#{x}")).collect();
            o.add(format!("ADVANCED_FACE({},({}),#{surf},{})", quote(&fc.name), b.join(","), if fc.reversed != flip { ".F." } else { ".T." }))
        };
        // The shells: each outside a solid of its own, each void in the
        // outside it is in.
        let mut outs: Vec<(usize, crate::vec::Aabb)> = Vec::new();
        let mut voids: Vec<usize> = Vec::new();
        for (k, sh) in s.shells.iter().enumerate() {
            if shell_volume(s, &sh.faces) >= 0.0 {
                let mut b = crate::vec::Aabb::empty();
                for &f in &sh.faces {
                    for c in s.faces[f].loops.iter().flatten() {
                        for p in s.edges[c.edge].sample(4) {
                            b.include(p);
                        }
                    }
                }
                outs.push((k, b));
            } else {
                voids.push(k);
            }
        }
        for (i, &(k, ref bx)) in outs.iter().enumerate() {
            let faces: Vec<String> = s.shells[k].faces.iter().map(|&f| format!("#{}", face(&mut o, f, false))).collect();
            let shell = o.add(format!("CLOSED_SHELL('',({}))", faces.join(",")));
            let mine: Vec<usize> = voids
                .iter()
                .copied()
                .filter(|&v| {
                    let p = s.vertices.get(s.edges[s.faces[s.shells[v].faces[0]].loops[0][0].edge].start).map(|x| x.point);
                    p.is_some_and(|p| bx.contains(p, 1e-6)) && outs.iter().position(|o2| o2.1.contains(p.unwrap(), 1e-6)) == Some(i)
                })
                .collect();
            let label = if outs.len() > 1 { format!("{name}.{}", i + 1) } else { name.to_string() };
            let solid = if mine.is_empty() {
                o.add(format!("MANIFOLD_SOLID_BREP({},#{shell})", quote(&label)))
            } else {
                // A void is written as a closed shell facing out of it, and
                // turned over.
                let vs: Vec<String> = mine
                    .iter()
                    .map(|&v| {
                        let faces: Vec<String> = s.shells[v].faces.iter().map(|&f| format!("#{}", face(&mut o, f, true))).collect();
                        let inner = o.add(format!("CLOSED_SHELL('',({}))", faces.join(",")));
                        format!("#{}", o.add(format!("ORIENTED_CLOSED_SHELL('',*,#{inner},.F.)")))
                    })
                    .collect();
                o.add(format!("BREP_WITH_VOIDS({},#{shell},({}))", quote(&label), vs.join(",")))
            };
            items.push(solid);
        }
    }
    let list: Vec<String> = items.iter().map(|x| format!("#{x}")).collect();
    let rep = o.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION({},({}),#{ctx})", quote(part), list.join(",")));
    o.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"));

    let mut f = String::new();
    f.push_str("ISO-10303-21;\nHEADER;\n");
    let _ = writeln!(f, "FILE_DESCRIPTION(({}),'2;1');", quote(&format!("{part}, from Proton CAD")));
    let _ = writeln!(f, "FILE_NAME({},{},(''),(''),'Proton CAD','Proton CAD','');", quote(part), quote(stamp));
    f.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    f.push_str(&o.text);
    f.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    f
}

/// Seconds since 1970 as the file's time: `2026-11-09T06:40:00`, UTC.
pub fn stamp(unix: u64) -> String {
    // Days to a date: Howard Hinnant's civil-from-days.
    let (days, secs) = ((unix / 86_400) as i64, unix % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
