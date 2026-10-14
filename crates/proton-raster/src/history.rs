//! Undo and redo, by what each change touched.
//!
//! A step keeps the tiles as they were before it — shared with the canvas
//! until the change copied them, so remembering costs what was changed and no
//! more — or, for a change to the layers themselves, the stack as it was. The
//! history is held to a number of bytes and of steps, the oldest let go first,
//! and the newest always kept. Undoing to the state last saved leaves a canvas
//! that is not unsaved, as the state number says.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::canvas::{Canvas, Layer, Preset};
use crate::op::{Applied, Op};
use crate::tile::{Grid, Tile, TILE_BYTES};

/// What a change replaced, to put back.
#[derive(Debug, Clone, Default)]
pub struct Patch {
    /// The layer stack and the next layer id, when the layers themselves changed.
    pub layers: Option<(Vec<Layer>, u32)>,
    pub palette: Option<Vec<[u8; 4]>>,
    /// Tiles as they were: layer, index, tile.
    pub tiles: Vec<(u32, u32, Option<Arc<Tile>>)>,
    /// The selection as it was, when it changed.
    pub selection: Option<Grid>,
    /// The canvas's size as it was, when it changed.
    pub size: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
struct Step {
    label: String,
    /// The canvas's state before the change and after it.
    before: u64,
    after: u64,
    patch: Patch,
    bytes: u64,
}

#[derive(Debug, Clone)]
pub struct History {
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    bytes: u64,
    most_bytes: u64,
    most_steps: u32,
}

impl History {
    pub fn new(preset: Preset) -> History {
        let (most_bytes, most_steps) = preset.history();
        History::with_limits(most_bytes, most_steps)
    }

    pub fn with_limits(most_bytes: u64, most_steps: u32) -> History {
        History { undo: VecDeque::new(), redo: Vec::new(), bytes: 0, most_bytes, most_steps }
    }

    /// Apply an op and remember it. A change that changed nothing is not a
    /// step.
    pub fn apply(&mut self, canvas: &mut Canvas, op: &Op, label: &str) -> Result<Applied, String> {
        let before = canvas.state;
        let (applied, patch) = canvas.apply(op)?;
        if applied.changed {
            self.redo.clear();
            let bytes = cost(&patch, canvas);
            self.bytes += bytes;
            self.undo.push_back(Step { label: label.to_string(), before, after: canvas.state, patch, bytes });
            self.trim();
        }
        Ok(applied)
    }

    /// A change that is not an op — layers brought in from a file — made
    /// and remembered as one step, as an op's would be.
    pub fn record(&mut self, canvas: &mut Canvas, label: &str, change: impl FnOnce(&mut Canvas) -> Result<(Applied, Patch), String>) -> Result<Applied, String> {
        let before = canvas.state;
        let (applied, patch) = change(canvas)?;
        if applied.changed {
            self.redo.clear();
            let bytes = cost(&patch, canvas);
            self.bytes += bytes;
            self.undo.push_back(Step { label: label.to_string(), before, after: canvas.state, patch, bytes });
            self.trim();
        }
        Ok(applied)
    }

    /// Take back the newest step; what it changed, and its name.
    pub fn undo(&mut self, canvas: &mut Canvas) -> Option<(Applied, String)> {
        let step = self.undo.pop_back()?;
        self.bytes -= step.bytes;
        let (inverse, applied) = canvas.restore(step.patch);
        canvas.state = step.before;
        let bytes = cost(&inverse, canvas);
        self.redo.push(Step { label: step.label.clone(), before: step.before, after: step.after, patch: inverse, bytes });
        Some((applied, step.label))
    }

    /// Make again the step last taken back.
    pub fn redo(&mut self, canvas: &mut Canvas) -> Option<(Applied, String)> {
        let step = self.redo.pop()?;
        let (inverse, applied) = canvas.restore(step.patch);
        canvas.state = step.after;
        let bytes = cost(&inverse, canvas);
        self.bytes += bytes;
        self.undo.push_back(Step { label: step.label.clone(), before: step.before, after: step.after, patch: inverse, bytes });
        self.trim();
        Some((applied, step.label))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The names of what undo and redo would do next.
    pub fn labels(&self) -> (Option<&str>, Option<&str>) {
        (self.undo.back().map(|s| s.label.as_str()), self.redo.last().map(|s| s.label.as_str()))
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn steps(&self) -> u32 {
        self.undo.len() as u32
    }

    /// The oldest let go while there is too much, the newest always kept.
    fn trim(&mut self) {
        while self.undo.len() > 1 && (self.bytes > self.most_bytes || self.undo.len() as u32 > self.most_steps) {
            if let Some(old) = self.undo.pop_front() {
                self.bytes -= old.bytes;
            }
        }
    }
}

/// The bytes a patch holds that the canvas does not share: tiles as they were
/// before a change, and a stack of layers' tiles no longer on the canvas.
fn cost(patch: &Patch, canvas: &Canvas) -> u64 {
    let mut bytes = 0;
    for (layer, index, tile) in &patch.tiles {
        if let Some(tile) = tile {
            let shared = canvas.layer(*layer).and_then(|l| l.grid.tile(*index)).is_some_and(|now| Arc::ptr_eq(now, tile));
            if !shared {
                bytes += TILE_BYTES as u64;
            }
        }
    }
    if let Some(sel) = &patch.selection {
        for index in 0..sel.cols * sel.rows {
            if let Some(tile) = sel.tile(index) {
                let shared = canvas.selection.tile(index).is_some_and(|now| Arc::ptr_eq(now, tile)) || Arc::ptr_eq(tile, crate::select::full());
                if !shared {
                    bytes += TILE_BYTES as u64;
                }
            }
        }
    }
    if let Some((layers, _)) = &patch.layers {
        for layer in layers {
            let now = canvas.layer(layer.info.id);
            for index in 0..layer.grid.cols * layer.grid.rows {
                if let Some(tile) = layer.grid.tile(index) {
                    let shared = now.and_then(|l| l.grid.tile(index)).is_some_and(|t| Arc::ptr_eq(t, tile));
                    if !shared {
                        bytes += TILE_BYTES as u64;
                    }
                }
            }
        }
    }
    bytes
}
