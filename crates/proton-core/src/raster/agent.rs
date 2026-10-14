//! The agent's canvas tools: the open canvas read — its layers, palette and
//! selection, and its pixels as a grid of letters, so pixel art can be seen —
//! a canvas made, drawn on, made from seeds, and written out.
//!
//! An edit is the person's kind of edit: a call's marks are one step of
//! their undo, held until they save, on their screen at once — the window is
//! sent the layers and the tiles that changed, as undo sends them — and kept
//! to what is selected, as theirs are.

use std::fmt::Write as _;
use std::path::Path;

use proton_raster::filter::Filter;
use proton_raster::gen::{color, Generator};
use proton_raster::{blend, select, Blend, Canvas, Op, Preset, ShapeKind, Tip, Tool, SELECTION};
use serde_json::{json, Value};

use super::{by_layer, find, Raster, RasterFormat};
use crate::bus::{Command, Event, NoticeLevel};

/// The tools answered here, by name.
pub const TOOLS: [&str; 5] = ["canvas_info", "canvas_new", "canvas_draw", "canvas_generate", "canvas_export"];

/// The symbols a grid of pixels is written in, a colour each.
const SYMBOLS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// The most pixels a grid shows, a side, by preset.
const GRID_PIXEL: u32 = 128;
const GRID_PAINT: u32 = 64;
/// The most marks one call draws.
const MARKS: usize = 20_000;

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
fn hex(c: [u8; 4]) -> String {
    proton_raster::file::hex(c)
}

/// A colour as the agent writes one — `#rrggbb`, `#rrggbbaa`, `#rgb`, [r, g, b],
/// [r, g, b, a], or a palette's swatch by its number.
fn colour(value: &Value, palette: &[[u8; 4]]) -> Result<[u8; 4], String> {
    match value {
        Value::String(s) => {
            // CSS's short form too, #rgb and #rgba, as a model often writes it.
            let digits = s.trim().trim_start_matches('#');
            let long = if (digits.len() == 3 || digits.len() == 4) && digits.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", digits.chars().flat_map(|c| [c, c]).collect::<String>()) } else { s.clone() };
            proton_raster::file::unhex(&long).map_err(|_| format!("{s:?} is not a colour: #rrggbb or #rrggbbaa"))
        }
        Value::Number(n) => {
            let i = n.as_u64().ok_or("a swatch is a whole number")? as usize;
            palette.get(i).copied().ok_or_else(|| format!("the palette has no swatch {i}; it has {}", palette.len()))
        }
        Value::Array(parts) if parts.len() == 3 || parts.len() == 4 => {
            let mut out = [255u8; 4];
            for (i, p) in parts.iter().enumerate() {
                out[i] = p.as_u64().filter(|&v| v <= 255).ok_or("a colour's channels are 0 to 255")? as u8;
            }
            Ok(out)
        }
        _ => Err("a colour is \"#rrggbb\", [r, g, b, a] or a swatch's number".into()),
    }
}

/// A point as [x, y].
fn point(value: &Value) -> Result<[i32; 2], String> {
    match value.as_array().map(Vec::as_slice) {
        Some([x, y]) => Ok([x.as_i64().ok_or("a point's x is a whole number")? as i32, y.as_i64().ok_or("a point's y is a whole number")? as i32]),
        _ => Err("a point is [x, y]".into()),
    }
}

/// A rectangle as [x, y, width, height].
fn rect(value: &Value) -> Result<[i32; 4], String> {
    match value.as_array().map(Vec::as_slice) {
        Some([x, y, w, h]) => {
            let n = |v: &Value| v.as_i64().map(|n| n as i32).ok_or("a rectangle is [x, y, width, height] of whole numbers");
            Ok([n(x)?, n(y)?, n(w)?, n(h)?])
        }
        _ => Err("a rectangle is [x, y, width, height]".into()),
    }
}

/// A seed when none was asked for: the clock's.
fn any_seed() -> u32 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(7);
    proton_raster::gen::rng::hash(nanos as u32, (nanos >> 32) as i64, 1)
}

/// Pixels of `canvas` within `area` — of one layer, or everything laid
/// together — as rows of symbols and a legend of what each is.
fn grid(canvas: &Canvas, layer: Option<u32>, area: [u32; 4]) -> Result<String, String> {
    let most = if canvas.preset == Preset::Pixel { GRID_PIXEL } else { GRID_PAINT };
    let [x0, y0, w, h] = area;
    if w > most || h > most {
        return Err(format!("canvas_info shows at most {most} × {most} pixels at once; ask for a smaller rect"));
    }
    let flat;
    let px: Box<dyn Fn(u32, u32) -> [u8; 4]> = match layer {
        Some(id) => {
            let g = canvas.grid(id).ok_or_else(|| format!("there is no layer {id}"))?;
            Box::new(move |x, y| g.pixel(x, y))
        }
        None => {
            flat = blend::flatten(canvas);
            let width = canvas.width;
            Box::new(move |x, y| {
                let at = ((y * width + x) * 4) as usize;
                [flat[at], flat[at + 1], flat[at + 2], flat[at + 3]]
            })
        }
    };
    // Palette colours keep their swatch's symbol; others follow, in the order met.
    let mut legend: Vec<[u8; 4]> = canvas.palette.iter().copied().take(SYMBOLS.len()).collect();
    let mut rows = Vec::new();
    for y in y0..y0 + h {
        let mut row = String::new();
        for x in x0..x0 + w {
            let p = px(x, y);
            if p[3] == 0 {
                row.push('.');
                continue;
            }
            let i = match legend.iter().position(|c| *c == p) {
                Some(i) => i,
                None => {
                    if legend.len() == SYMBOLS.len() {
                        return Err(format!("that part has more than {} colours, too many to write as letters; ask for a smaller rect", SYMBOLS.len()));
                    }
                    legend.push(p);
                    legend.len() - 1
                }
            };
            row.push(SYMBOLS[i] as char);
        }
        rows.push(row);
    }
    let used: Vec<String> = legend.iter().enumerate().filter(|(i, _)| rows.iter().any(|r| r.contains(SYMBOLS[*i] as char))).map(|(i, c)| format!("{}={}", SYMBOLS[i] as char, hex(*c))).collect();
    let mut out = format!("Pixels x {x0}–{} by y {y0}–{} ({}), '.' clear; {}:\n", x0 + w - 1, y0 + h - 1, layer.map_or("everything laid together".to_string(), |id| format!("layer {id}")), used.join(" "));
    out.push_str(&rows.join("\n"));
    Ok(out)
}

impl Raster {
    /// What the agent's last call changed on screen, for the window.
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// Events of a call the workspace answered for Canvas, to send with the
    /// rest.
    pub fn add_pending(&mut self, events: Vec<Event>) {
        self.pending.extend(events);
    }

    /// The canvas open, in words — or the canvases there are.
    fn summary(&self, root: &Path) -> String {
        let Some(open) = &self.open else {
            let (canvases, images) = find(root);
            let mut out = String::from("No canvas is open in Canvas mode.");
            if canvases.is_empty() {
                out.push_str(" There is none in the project; canvas_new makes one.");
            } else {
                let _ = write!(out, " The project's canvases: {}. The person opens one from Canvas's home page (⌃=), or canvas_new makes another.", canvases.iter().map(|c| format!("{} ({:?} {}×{})", c.path, c.preset, c.size[0], c.size[1]).to_lowercase()).collect::<Vec<_>>().join(", "));
            }
            if !images.is_empty() {
                let _ = write!(out, " Pictures it could bring in: {}.", images.join(", "));
            }
            return out;
        };
        let c = &open.canvas;
        let (undo, _) = open.history.labels();
        let mut out = format!(
            "Canvas {} ({}): {} {} × {} px{}. x runs right and y down from 0 at the top left.",
            open.name,
            open.shown,
            if c.preset == Preset::Pixel { "pixel art" } else { "painting" },
            c.width,
            c.height,
            if c.state != open.saved { ", with changes not saved" } else { "" }
        );
        if let Some(label) = undo {
            let _ = write!(out, " ⌘Z would undo: {label}.");
        }
        if c.palette.is_empty() {
            out.push_str("\nNo palette.");
        } else {
            let _ = write!(out, "\nPalette, by swatch: {}.", c.palette.iter().enumerate().map(|(i, p)| format!("{i} {}", hex(*p))).collect::<Vec<_>>().join(", "));
        }
        out.push_str("\nLayers, top first:");
        for layer in c.layers.iter().rev() {
            let i = &layer.info;
            let mut flags = Vec::new();
            if i.opacity < 255 {
                flags.push(format!("{}% opacity", (i.opacity as u32 * 100 + 127) / 255));
            }
            if i.blend != Blend::Normal {
                flags.push(format!("{:?}", i.blend).to_lowercase());
            }
            if i.hidden {
                flags.push("hidden".into());
            }
            if i.locked {
                flags.push("locked".into());
            }
            if i.alpha_lock {
                flags.push("draws only where it has pixels".into());
            }
            let pixels = select::bounds(&layer.grid).map_or("empty".to_string(), |b| format!("pixels within [{}, {}, {}, {}]", b[0], b[1], b[2], b[3]));
            let _ = write!(out, "\n- layer {} \"{}\": {}{}", i.id, i.name, pixels, if flags.is_empty() { String::new() } else { format!("; {}", flags.join(", ")) });
        }
        match c.selecting().then(|| select::bounds(&c.selection)).flatten() {
            Some(b) => {
                let _ = write!(out, "\nSelected: within [{}, {}, {}, {}] — every mark keeps to it.", b[0], b[1], b[2], b[3]);
            }
            None => out.push_str("\nNothing is selected."),
        }
        out
    }

    /// What `@canvas` sends: the canvas, and for small pixel art its pixels.
    pub fn about(&self, root: &Path) -> String {
        let mut out = self.summary(root);
        if let Some(open) = &self.open {
            let c = &open.canvas;
            if c.preset == Preset::Pixel && c.width <= 64 && c.height <= 64 {
                if let Ok(g) = grid(c, None, [0, 0, c.width, c.height]) {
                    out.push('\n');
                    out.push_str(&g);
                }
            } else {
                out.push_str("\ncanvas_info with `pixels` shows a part of it as a grid.");
            }
        }
        out
    }

    pub fn tool(&mut self, root: &Path, name: &str, input: &Value) -> Result<String, String> {
        if name == "canvas_info" {
            return self.tool_info(root, input);
        }
        if name == "canvas_new" {
            return self.tool_new(root, input);
        }
        if self.open.is_none() {
            return Err("No canvas is open in Canvas mode. The person opens one from its home page (⌃=), or canvas_new makes one.".into());
        }
        match name {
            "canvas_draw" => self.tool_draw(input),
            "canvas_generate" => self.tool_generate(input),
            "canvas_export" => {
                let format = match input.get("format").and_then(Value::as_str) {
                    Some("png") | None => RasterFormat::Png,
                    Some("psd") => RasterFormat::Psd,
                    Some(other) => return Err(format!("canvas_export: {other:?} is not a format: png or psd")),
                };
                let scale = input.get("scale").and_then(Value::as_u64).unwrap_or(1) as u32;
                let events = self.export(format, scale);
                let said = events.iter().find_map(|e| match e {
                    Event::Notice { text, level } => Some((text.clone(), *level)),
                    _ => None,
                });
                self.pending.extend(events);
                self.pending.push(self.home_event(root));
                match said {
                    Some((text, NoticeLevel::Error | NoticeLevel::Warn)) => Err(text),
                    Some((text, _)) => Ok(format!("{}, beside the canvas's folder.", capital(&text))),
                    None => Ok("Written.".into()),
                }
            }
            _ => Err(format!("{name}: not a canvas tool")),
        }
    }

    fn tool_info(&self, root: &Path, input: &Value) -> Result<String, String> {
        let mut out = self.summary(root);
        if let (Some(ask), Some(open)) = (input.get("pixels"), &self.open) {
            let c = &open.canvas;
            let layer = ask.get("layer").and_then(Value::as_u64).map(|n| n as u32);
            let area = match ask.get("rect") {
                Some(r) => {
                    let [x, y, w, h] = rect(r)?;
                    let (x, y) = (x.max(0) as u32, y.max(0) as u32);
                    [x, y, (w.max(0) as u32).min(c.width.saturating_sub(x)), (h.max(0) as u32).min(c.height.saturating_sub(y))]
                }
                None => [0, 0, c.width, c.height],
            };
            if area[2] == 0 || area[3] == 0 {
                return Err("that rect is off the canvas".into());
            }
            out.push('\n');
            out.push_str(&grid(c, layer.filter(|&l| l != SELECTION), area)?);
        }
        Ok(out)
    }

    fn tool_new(&mut self, root: &Path, input: &Value) -> Result<String, String> {
        let name = input.get("name").and_then(Value::as_str).ok_or("canvas_new: `name` is required")?.to_string();
        let preset = match input.get("preset").and_then(Value::as_str) {
            Some("paint") => Preset::Paint,
            Some("pixel") | None => Preset::Pixel,
            Some(other) => return Err(format!("canvas_new: {other:?} is not a preset: pixel or paint")),
        };
        let fallback = if preset == Preset::Pixel { 32 } else { 1024 };
        let width = input.get("width").and_then(Value::as_u64).unwrap_or(fallback) as u32;
        let height = input.get("height").and_then(Value::as_u64).unwrap_or(width as u64) as u32;
        let events = self.apply(root, Command::RasterNew { name: name.clone(), preset, width, height, folder: None });
        if let Some(why) = events.iter().find_map(|e| match e {
            Event::Notice { text, level: NoticeLevel::Warn | NoticeLevel::Error } => Some(text.clone()),
            _ => None,
        }) {
            return Err(why);
        }
        self.pending.extend(events);
        let open = self.open.as_ref().ok_or("the canvas was not opened")?;
        Ok(format!(
            "Made {} ({}), {} × {} {}, open on the person's screen with one empty layer (layer {}){}.",
            open.name,
            open.shown,
            width,
            height,
            if preset == Preset::Pixel { "pixel art" } else { "painting" },
            open.canvas.layers[0].info.id,
            if preset == Preset::Pixel { " and a palette of sixteen" } else { "" }
        ))
    }

    /// The layer a call draws on: the one named (by id or name), a new one,
    /// or the top one. The ops to make a new one go first.
    fn target(&self, input: &Value, ops: &mut Vec<Op>) -> Result<u32, String> {
        let c = &self.open.as_ref().ok_or("no canvas is open")?.canvas;
        if let Some(name) = input.get("new_layer").and_then(Value::as_str) {
            ops.push(Op::AddLayer { name: name.to_string(), at: None });
            return Ok(c.next_id);
        }
        match input.get("layer") {
            Some(Value::Number(n)) => {
                let id = n.as_u64().unwrap_or(0) as u32;
                c.layer(id).map(|l| l.info.id).ok_or_else(|| format!("there is no layer {id}"))
            }
            Some(Value::String(name)) => c.layers.iter().find(|l| l.info.name == *name).map(|l| l.info.id).ok_or_else(|| format!("there is no layer named {name:?}")),
            _ => c.layers.last().map(|l| l.info.id).ok_or_else(|| "the canvas has no layer".to_string()),
        }
    }

    /// Ops made one step: applied, remembered, and sent to the window.
    fn edit_all(&mut self, label: &str, ops: &[Op]) -> Result<proton_raster::Applied, String> {
        let open = self.open.as_mut().ok_or("no canvas is open")?;
        let applied = open.history.record(&mut open.canvas, label, |c| c.apply_all(ops))?;
        if applied.changed {
            open.version += 1;
            let changed = Event::RasterChanged { generation: open.generation, version: open.version, tiles: by_layer(&applied.touched), meta: applied.meta };
            self.pending.push(self.canvas_event());
            self.pending.push(changed);
        }
        Ok(applied)
    }

    fn tool_draw(&mut self, input: &Value) -> Result<String, String> {
        let marks = input.get("marks").and_then(Value::as_array).ok_or("canvas_draw: `marks` is required: a list of marks")?;
        if marks.is_empty() {
            return Err("canvas_draw: no marks to draw".into());
        }
        let mut ops = Vec::new();
        let layer = self.target(input, &mut ops)?;
        let palette = self.open.as_ref().map(|o| o.canvas.palette.clone()).unwrap_or_default();
        let ink = |m: &Value| -> Result<[u8; 4], String> { m.get("color").map(|c| colour(c, &palette)).unwrap_or(Ok([0, 0, 0, 255])) };
        let tip = |m: &Value| Tip { size: m.get("size").and_then(Value::as_u64).unwrap_or(1).clamp(1, 64) as u32, round: m.get("round").and_then(Value::as_bool).unwrap_or(false), mirror: [None, None] };
        let mut count = 0;
        for (i, m) in marks.iter().enumerate() {
            let fail = |why: String| format!("canvas_draw: mark {}: {why}", i + 1);
            if m.get("op").is_some() {
                // One of the engine's own ops, as the window sends them.
                ops.push(serde_json::from_value(m.clone()).map_err(|e| fail(format!("not an op: {e}")))?);
                count += 1;
                continue;
            }
            if let Some(points) = m.get("pixels").and_then(Value::as_array) {
                let c = ink(m).map_err(fail)?;
                for p in points {
                    let [x, y] = point(p).map_err(fail)?;
                    ops.push(Op::FillRect { layer, rect: [x, y, 1, 1], color: c });
                    count += 1;
                }
            } else if let Some(points) = m.get("line").and_then(Value::as_array) {
                let c = ink(m).map_err(fail)?;
                let mut q8 = Vec::new();
                for p in points {
                    let [x, y] = point(p).map_err(fail)?;
                    q8.extend([x * 256 + 128, y * 256 + 128, 65535]);
                }
                ops.push(Op::Stroke { layer, tool: Tool::Pencil, tip: tip(m), color: c, pixel_perfect: m.get("perfect").and_then(Value::as_bool).unwrap_or(true), points: q8 });
                count += 1;
            } else if let Some(r) = m.get("rect").or_else(|| m.get("ellipse")) {
                let [x, y, w, h] = rect(r).map_err(fail)?;
                if w <= 0 || h <= 0 {
                    return Err(fail("a shape needs a width and a height".into()));
                }
                let shape = if m.get("rect").is_some() { ShapeKind::Rect } else { ShapeKind::Ellipse };
                let filled = m.get("fill").and_then(Value::as_bool).unwrap_or(false);
                ops.push(Op::Shape { layer, shape, from: [x, y], to: [x + w - 1, y + h - 1], filled, tip: tip(m), color: ink(m).map_err(fail)? });
                count += 1;
            } else if let Some(p) = m.get("fill") {
                let [x, y] = point(p).map_err(fail)?;
                let tolerance = m.get("tolerance").and_then(Value::as_u64).unwrap_or(0).min(255) as u8;
                ops.push(Op::Fill { layer, x, y, color: ink(m).map_err(fail)?, tolerance, contiguous: m.get("everywhere").and_then(Value::as_bool) != Some(true), diagonal: false });
                count += 1;
            } else if let Some(r) = m.get("erase") {
                ops.push(Op::FillRect { layer, rect: rect(r).map_err(fail)?, color: [0, 0, 0, 0] });
                count += 1;
            } else {
                return Err(fail("a mark is pixels, line, rect, ellipse, fill, erase, or an engine op".into()));
            }
            if ops.len() > MARKS {
                return Err(format!("canvas_draw: at most {MARKS} marks a call"));
            }
        }
        let applied = self.edit_all("the agent's drawing", &ops).map_err(|why| format!("canvas_draw drew nothing: {why}"))?;
        let name = self.open.as_ref().and_then(|o| o.canvas.layer(applied.made.unwrap_or(layer)).map(|l| l.info.name.clone())).unwrap_or_default();
        if !applied.changed {
            return Ok("Drawn, but nothing changed: those pixels were already so.".into());
        }
        Ok(format!(
            "Drew {count} mark{} on layer {} \"{name}\". It is one step of the person's undo, on their screen now and held until they save — say what you drew rather than that a file was written.",
            if count == 1 { "" } else { "s" },
            applied.made.unwrap_or(layer)
        ))
    }

    fn tool_generate(&mut self, input: &Value) -> Result<String, String> {
        let mut ops = Vec::new();
        let open = self.open.as_ref().ok_or("no canvas is open")?;
        let (w, h) = (open.canvas.width as i32, open.canvas.height as i32);
        let palette = open.canvas.palette.clone();
        let mut said = Vec::new();
        if let Some(ramp) = input.get("ramp") {
            let base = colour(ramp.get("base").unwrap_or(&json!("#5a7fbf")), &palette)?;
            let steps = ramp.get("steps").and_then(Value::as_u64).unwrap_or(5).clamp(2, 16) as u32;
            let shift = ramp.get("shift").and_then(Value::as_u64).unwrap_or(25) as u32;
            let made = color::ramp(base, steps, shift, 24000);
            let fresh: Vec<[u8; 4]> = made.iter().copied().filter(|c| !palette.contains(c)).collect();
            said.push(format!("a ramp of {} — {} — added to the palette as swatches {} to {}", hex(base), made.iter().map(|c| hex(*c)).collect::<Vec<_>>().join(", "), palette.len(), palette.len() + fresh.len().max(1) - 1));
            ops.push(Op::SetPalette { colors: palette.iter().copied().chain(fresh).collect() });
        }
        let wants_layer = input.get("generator").is_some() || input.get("filter").is_some();
        let layer = if wants_layer { Some(self.target(input, &mut ops)?) } else { None };
        if let (Some(g), Some(layer)) = (input.get("generator"), layer) {
            let mut g = g.clone();
            if g.get("seed").is_none() {
                g["seed"] = json!(any_seed());
            }
            if g.get("kind").and_then(Value::as_str) == Some("sprite") && g.get("at").is_none() {
                let scale = g.get("scale").and_then(Value::as_i64).unwrap_or(1) as i32;
                let size = match g.get("template").and_then(Value::as_str) {
                    Some("item") => [12, 12],
                    Some("creature") => [14, 12],
                    _ => [14, 14],
                };
                g["at"] = json!([(w - size[0] * scale) / 2, (h - size[1] * scale) / 2]);
            }
            for key in ["color", "floor"] {
                if let Some(c) = g.get(key).cloned().filter(|c| !c.is_array() && !c.is_null()) {
                    g[key] = json!(colour(&c, &palette)?);
                }
            }
            if let Some(list) = g.get("colors").and_then(Value::as_array).cloned() {
                g["colors"] = json!(list.iter().map(|c| colour(c, &palette)).collect::<Result<Vec<_>, _>>()?);
            } else if g.get("kind").and_then(Value::as_str) == Some("noise") {
                g["colors"] = json!(if palette.len() >= 2 { palette.clone() } else { color::ramp([90, 120, 170, 255], 5, 25, 26000) });
            }
            let generator: Generator = serde_json::from_value(g.clone()).map_err(|e| format!("canvas_generate: not a generator: {e}"))?;
            said.push(format!("{} with seed {}", g.get("kind").and_then(Value::as_str).unwrap_or("a picture"), g["seed"]));
            ops.push(Op::Generate { layer, generator });
        }
        if let (Some(f), Some(layer)) = (input.get("filter"), layer) {
            let mut f = f.clone();
            if let Some(c) = f.get("color").cloned().filter(|c| !c.is_array()) {
                f["color"] = json!(colour(&c, &palette)?);
            }
            let filter: Filter = serde_json::from_value(f.clone()).map_err(|e| format!("canvas_generate: not a filter: {e}"))?;
            said.push(format!("the {} filter", f.get("kind").and_then(Value::as_str).unwrap_or("")));
            ops.push(Op::Filter { layer, filter });
        }
        if ops.is_empty() {
            return Err("canvas_generate: give a generator, a filter or a ramp".into());
        }
        let applied = self.edit_all("the agent's making", &ops).map_err(|why| format!("canvas_generate made nothing: {why}"))?;
        let on = layer.map(|l| applied.made.unwrap_or(l)).and_then(|l| self.open.as_ref().and_then(|o| o.canvas.layer(l)).map(|x| format!(" on layer {} \"{}\"", x.info.id, x.info.name))).unwrap_or_default();
        Ok(format!(
            "Made {}{on}. It is one step of the person's undo, on their screen now and held until they save; the same seed makes the same pixels again.",
            said.join(", then ")
        ))
    }
}

/// A sentence's first letter made a capital.
fn capital(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
