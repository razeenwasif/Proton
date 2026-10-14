//! A canvas out to other files and in from them: written beside its folder
//! as a PNG or a PSD of a frame, or a sprite sheet of every frame; sent to a
//! game; and pictures — a project's PNGs, PSDs and Aseprite files, or what
//! the window was handed by a paste or a drop — brought in as a canvas of
//! their own or as layers, one step of undo.

use std::path::{Path, PathBuf};

use proton_raster::{file, port, Preset};

use super::{by_layer, inside, slug, Raster, RasterFormat, EXTENSION};
use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The most bytes one picture handed over may be.
const BLOB_MOST: usize = 256 << 20;
/// How many handed-over pictures are kept, the newest.
const BLOBS_KEPT: usize = 4;

/// `dir/stem.ext`, or `dir/stem-2.ext`… when that is taken.
fn free(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}

/// A path as the project names it.
fn shown(root: &Path, path: &Path) -> String {
    let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    path.strip_prefix(root).or_else(|_| path.strip_prefix(&real)).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string())
}

/// A file in the project, held to it — any kind, unlike `inside`'s canvas
/// folders.
fn in_project(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not a file in the project"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let real = std::fs::canonicalize(&path).map_err(|e| format!("{rel}: {e}"))?;
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(real)
}

/// The colours of a small picture, when it has few: a pixel canvas's
/// palette, in the order first met.
fn palette_of(canvas: &proton_raster::Canvas) -> Vec<[u8; 4]> {
    let mut colours: Vec<[u8; 4]> = Vec::new();
    for grid in canvas.layers.iter().flat_map(|l| l.cels.iter()) {
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let p = grid.pixel(x, y);
                if p[3] == 255 && !colours.contains(&p) {
                    if colours.len() == 32 {
                        return Vec::new();
                    }
                    colours.push(p);
                }
            }
        }
    }
    colours
}

impl Raster {
    /// A picture the window hands over, kept to bring in: its number.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<u32, String> {
        if bytes.len() > BLOB_MOST {
            return Err(format!("a picture of more than {} MB is too large to bring in", BLOB_MOST >> 20));
        }
        self.next_blob += 1;
        self.blobs.insert(self.next_blob, bytes);
        while self.blobs.len() > BLOBS_KEPT {
            if let Some(&oldest) = self.blobs.keys().next() {
                self.blobs.remove(&oldest);
            }
        }
        Ok(self.next_blob)
    }

    /// What was copied, as a PNG of the part of the canvas it covers — for
    /// the system's clipboard. None if nothing was.
    pub fn clip_png(&self) -> Option<Vec<u8>> {
        let open = self.open.as_ref()?;
        let clip = open.canvas.clip.as_ref()?;
        let [x, y, w, h] = proton_raster::select::bounds(clip)?;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for py in y..y + h {
            for px in x..x + w {
                rgba.extend_from_slice(&clip.pixel(px, py));
            }
        }
        file::encode_png(w, h, &rgba).ok()
    }

    /// Write the open canvas beside its folder: frame `frame` (the one
    /// shown if not said) as a PNG or a PSD, or every frame as a sheet.
    pub(super) fn export(&self, format: RasterFormat, scale: u32, frame: Option<u32>, columns: u32) -> Vec<Event> {
        let Some(open) = &self.open else { return vec![notice("no canvas is open".into(), NoticeLevel::Warn)] };
        let dir = open.dir.parent().unwrap_or(&open.dir).to_path_buf();
        let stem = slug(&open.name);
        let frames = open.canvas.frames.len() as u32;
        let frame = frame.unwrap_or(open.frame);
        if frame >= frames {
            return vec![notice(format!("there is no frame {}; the canvas has {frames}", frame + 1), NoticeLevel::Warn)];
        }
        let scale = scale.clamp(1, 16);
        let scaled = if scale > 1 { format!("{stem}@{scale}x") } else { stem.clone() };
        // A frame of an animation is named for its place in it.
        let framed = |s: &str| if frames > 1 { format!("{s}-{}", frame + 1) } else { s.to_string() };
        let (path, bytes) = match format {
            RasterFormat::Png => (dir.join(format!("{}.png", framed(&scaled))), port::png(&open.canvas, frame, scale)),
            RasterFormat::Psd => (dir.join(format!("{}.psd", framed(&stem))), Ok(port::psd(&open.canvas, frame))),
            RasterFormat::Sheet => {
                let image = format!("{scaled}-sheet.png");
                match port::sheet(&open.canvas, &image, columns, scale) {
                    Ok((png, json)) => {
                        let json_path = dir.join(format!("{scaled}-sheet.json"));
                        if let Err(why) = std::fs::write(&json_path, json) {
                            return vec![notice(format!("{} was not written: {why}", json_path.display()), NoticeLevel::Error)];
                        }
                        (dir.join(image), Ok(png))
                    }
                    Err(why) => (dir.join(image), Err(why)),
                }
            }
        };
        match bytes.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let said = match format {
                    RasterFormat::Sheet => format!("wrote {name} and its JSON, {frames} frame{}", if frames == 1 { "" } else { "s" }),
                    _ => format!("wrote {name}"),
                };
                vec![notice(said, NoticeLevel::Info)]
            }
            Err(why) => vec![notice(format!("{} was not written: {why}", path.display()), NoticeLevel::Error)],
        }
    }

    /// Write the open canvas into a game's `art/` folder: its folder, and
    /// the file as the game names it.
    pub fn send(&self, root: &Path, game: &str, layers: bool) -> Result<(PathBuf, String), String> {
        let open = self.open.as_ref().ok_or("no canvas is open")?;
        let dir = if game.is_empty() || game == "." { std::fs::canonicalize(root).map_err(|e| e.to_string())? } else { in_project(root, game)? };
        if !proton_game::project::Project::is_game(&dir) {
            return Err(format!("{game} is not a game"));
        }
        let art = dir.join("art");
        std::fs::create_dir_all(&art).map_err(|e| format!("{}: {e}", art.display()))?;
        let stem = slug(&open.name);
        let (name, bytes) = if layers { (format!("{stem}.psd"), port::psd(&open.canvas, open.frame)) } else { (format!("{stem}.png"), port::png(&open.canvas, open.frame, 1)?) };
        std::fs::write(art.join(&name), bytes).map_err(|e| format!("{name}: {e}"))?;
        // An animation's every frame, beside it, for the game to animate by.
        if open.canvas.frames.len() > 1 {
            let image = format!("{stem}-sheet.png");
            let (png, json) = port::sheet(&open.canvas, &image, 0, 1)?;
            std::fs::write(art.join(&image), png).map_err(|e| format!("{image}: {e}"))?;
            std::fs::write(art.join(format!("{stem}-sheet.json")), json).map_err(|e| format!("{stem}-sheet.json: {e}"))?;
        }
        Ok((dir, format!("art/{name}")))
    }

    /// Bring a picture in: as layers on the canvas open, or as a canvas.
    pub(super) fn import(&mut self, root: &Path, path: Option<&str>, blob: Option<u32>, layer: bool, above: Option<u32>, named: Option<String>) -> Vec<Event> {
        let warn = |why: String| vec![notice(why, NoticeLevel::Warn)];
        let (bytes, name, beside) = match (path, blob) {
            (Some(rel), _) => match in_project(root, rel).and_then(|p| std::fs::read(&p).map(|b| (b, p)).map_err(|e| format!("{rel}: {e}"))) {
                Ok((bytes, p)) => {
                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Picture".into());
                    (bytes, name, p.parent().map(Path::to_path_buf))
                }
                Err(why) => return warn(why),
            },
            (None, Some(id)) => match self.blobs.get(&id) {
                Some(bytes) => {
                    // A dropped file's name, without its kind.
                    let name = named.as_deref().map(|n| Path::new(n).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).filter(|n| !n.trim().is_empty());
                    (bytes.clone(), name.unwrap_or_else(|| "Pasted".to_string()), None)
                }
                None => return warn("the picture handed over is gone: paste or drop it again".into()),
            },
            (None, None) => return warn("bring in what? a file, or a picture handed over".into()),
        };
        let picture = match port::read(&bytes, &name) {
            Ok(p) => p,
            Err(why) => return warn(why),
        };
        let mut events = Vec::new();
        if layer {
            let Some(open) = &mut self.open else { return warn("open a canvas to bring a picture in as a layer".into()) };
            let (w, h) = (open.canvas.width, open.canvas.height);
            let frames = open.canvas.frames.len();
            if picture.frames.len() > frames {
                events.push(notice(format!("{name} has {} frames and the canvas {frames}; the frames past them were left out", picture.frames.len()), NoticeLevel::Warn));
            }
            // A still picture smaller than the canvas is put in its middle —
            // in the frame shown; a Photoshop or Aseprite file's layers where
            // its own canvas has them, frame by frame.
            let centre = |pw: u32, ph: u32| if pw <= w && ph <= h { [(w - pw) as i32 / 2, (h - ph) as i32 / 2] } else { [0, 0] };
            let still = picture.frames.len() <= 1;
            let lone = still && picture.parts.len() == 1 && picture.parts[0].cels[0].as_ref().is_some_and(|c| c.left == 0 && c.top == 0);
            let shown = open.frame as usize;
            let grids = picture
                .parts
                .iter()
                .map(|p| {
                    let cels = if still {
                        let mut cels = vec![proton_raster::Grid::new(w, h); shown + 1];
                        if let Some(image) = &p.cels[0] {
                            let at = if lone { centre(image.width, image.height) } else { [image.left, image.top] };
                            cels[shown] = port::grid_of(image, w, h, at);
                        }
                        cels
                    } else {
                        port::cels_of(p, w, h, picture.frames.len().min(frames) as u32)
                    };
                    (p.name.clone(), p.opacity, !p.visible, p.blend, cels)
                })
                .collect();
            let label = if picture.parts.len() == 1 { format!("bring in {name}") } else { format!("bring in {} layers", picture.parts.len()) };
            match open.history.record(&mut open.canvas, &label, |c| c.bring_in(grids, above)) {
                Ok(applied) => {
                    open.version += 1;
                    let changed = Event::RasterChanged { generation: open.generation, version: open.version, tiles: by_layer(&applied.touched), meta: true };
                    // The layers first, as undo sends them.
                    events.push(self.canvas_event());
                    events.push(changed);
                }
                Err(why) => return warn(why),
            }
        } else {
            if self.dirty() {
                return warn("save or discard the open canvas's changes before bringing in another".into());
            }
            let preset = port::preset_for(picture.width, picture.height);
            let mut canvas = match port::canvas_of(&picture, preset) {
                Ok(c) => c,
                Err(why) => return warn(why),
            };
            if preset == Preset::Pixel && canvas.palette.is_empty() {
                canvas.palette = palette_of(&canvas);
            }
            let base = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
            let dir = free(beside.as_deref().unwrap_or(&base), &slug(&name), EXTENSION);
            let folder = shown(root, &dir);
            if let Err(why) = inside(root, &folder).and_then(|_| file::save(&dir, &canvas)) {
                return warn(why);
            }
            self.show(root, dir, canvas);
            events.extend(self.events(root));
            events.push(notice(format!("made {folder} of {name}"), NoticeLevel::Info));
        }
        events.extend(picture.notes.into_iter().map(|n| notice(n, NoticeLevel::Warn)));
        events
    }
}
