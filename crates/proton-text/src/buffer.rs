//! A buffer: text, a cursor, and the history that got it here.
//!
//! `ropey` does the heavy lifting. It is behind a thin surface on purpose —
//! nothing outside this file knows the store is a rope, so replacing it with a
//! summary tree later (folded height, diagnostic counts) is a change to one
//! module rather than to everything that reads text.

use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::edit::{Cursor, Edit, Position, Range};
use crate::history::History;
use crate::motion;

/// How the next edit joins the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    /// Each edit is its own undo step.
    None,
    /// The next edit starts a group; the ones after it join it.
    Pending,
    /// Edits fold into the current history node.
    Joined,
}

#[derive(Debug)]
pub struct Buffer {
    text: Rope,
    path: Option<PathBuf>,
    history: History,
    /// The history revision the file on disk matches.
    saved_at: usize,
    /// Whether edits are currently folding into one undoable group.
    group: Group,
    /// Bumped on every change so the front end can drop stale deltas.
    version: u64,
    pub cursor: Cursor,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        Self {
            text: Rope::from_str(text),
            path: None,
            history: History::new(),
            saved_at: 0,
            group: Group::None,
            version: 0,
            cursor: Cursor::default(),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        Ok(Self {
            path: Some(path.to_path_buf()),
            ..Self::from_text(&text)
        })
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "buffer has no path",
            ));
        };
        std::fs::write(path, self.text.to_string())?;
        self.saved_at = self.history.revision();
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[No Name]".into())
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_modified(&self) -> bool {
        self.history.revision() != self.saved_at
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn contents(&self) -> String {
        self.text.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }

    /// One line, without its newline.
    pub fn line(&self, index: usize) -> String {
        if index >= self.text.len_lines() {
            return String::new();
        }
        let line = self.text.line(index);
        let mut s = line.to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        s
    }

    /// A window of lines, for a virtualised view.
    pub fn lines(&self, from: usize, count: usize) -> Vec<String> {
        let last = (from + count).min(self.text.len_lines());
        (from..last).map(|i| self.line(i)).collect()
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len_chars());
        let line = self.text.char_to_line(offset);
        Position {
            line,
            column: offset - self.text.line_to_char(line),
        }
    }

    pub fn offset_of(&self, position: Position) -> usize {
        let line = position.line.min(self.text.len_lines().saturating_sub(1));
        let start = self.text.line_to_char(line);
        (start + position.column).min(motion::line_end(self.text.slice(..), start))
    }

    /// LSP speaks UTF-16 code units, so the conversion belongs here rather
    /// than being rediscovered by every caller.
    pub fn utf16_of(&self, offset: usize) -> usize {
        self.text
            .char_to_utf16_cu(offset.min(self.text.len_chars()))
    }

    /// The column at `offset`, counted in UTF-16 code units from the start of
    /// its line — which is what an LSP position's `character` actually is. Not
    /// bytes, not characters: an emoji before the cursor moves this by two.
    pub fn utf16_column(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len_chars());
        let start = self.text.line_to_char(self.text.char_to_line(offset));
        self.utf16_of(offset) - self.utf16_of(start)
    }

    /// The inverse, for a position a server sent us. Clamped to the line, so a
    /// diagnostic that arrives one edit late lands somewhere real instead of
    /// panicking.
    pub fn offset_at_utf16(&self, line: usize, column: usize) -> usize {
        let line = line.min(self.text.len_lines().saturating_sub(1));
        let start = self.text.line_to_char(line);
        let end = motion::line_end(self.text.slice(..), start);
        let target = self.utf16_of(start) + column;
        if target >= self.text.len_utf16_cu() {
            return end;
        }
        self.text.utf16_cu_to_char(target).min(end)
    }

    /* ── editing ──────────────────────────────────────────────────── */

    /// Apply edits and record them as one undoable change.
    ///
    /// Callers pass edits in natural document order and they must not overlap.
    /// Everything is applied back to front so earlier offsets stay valid.
    pub fn apply(&mut self, mut edits: Vec<Edit>, cursor_after: Cursor) {
        if edits.is_empty() {
            return;
        }
        edits.sort_by_key(|e| e.start);
        let before = self.cursor;

        // The inverses are computed against the text as it will be *after* the
        // whole change, not against the intermediate states — otherwise every
        // inverse but the first carries offsets that no longer exist, and a
        // multi-edit undo reads off the end. `delta` is how far each later
        // edit has shifted by the time the change is done.
        let mut inverse = Vec::with_capacity(edits.len());
        let mut delta: isize = 0;
        for edit in &edits {
            let removed = self.text.slice(edit.start..edit.end).to_string();
            let start = (edit.start as isize + delta) as usize;
            inverse.push(Edit {
                start,
                end: start + edit.inserted_len(),
                text: removed,
            });
            delta += edit.inserted_len() as isize - (edit.end - edit.start) as isize;
        }

        // Both directions are stored in the order they apply: highest
        // offset first within one change, and for a group, chronological
        // across changes. Replay then never has to sort — and must not,
        // because a group's edits are sequential and depend on each other.
        edits.sort_by_key(|e| std::cmp::Reverse(e.start));
        inverse.sort_by_key(|e| std::cmp::Reverse(e.start));

        self.splice(&edits);
        self.cursor = self.clamp(cursor_after);

        match self.group {
            Group::Joined => self.history.join(edits, inverse, self.cursor),
            _ => {
                self.history.commit(edits, inverse, before, self.cursor);
                if self.group == Group::Pending {
                    self.group = Group::Joined;
                }
            }
        }
    }

    /// Start folding edits into one undoable change.
    ///
    /// `join_current` is true when the action that opened the group already
    /// made an edit: `c` deletes before insert mode starts, and that deletion
    /// belongs to the same undo step as the typing that follows.
    pub fn begin_group(&mut self, join_current: bool) {
        self.group = if join_current {
            Group::Joined
        } else {
            Group::Pending
        };
    }

    pub fn end_group(&mut self) {
        self.group = Group::None;
    }

    /// Apply edits in the order given. Callers order them highest offset
    /// first so that earlier offsets stay valid.
    fn splice(&mut self, edits: &[Edit]) {
        for edit in edits {
            self.text.remove(edit.start..edit.end);
            if !edit.text.is_empty() {
                self.text.insert(edit.start, &edit.text);
            }
        }
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.history.undo() else {
            return false;
        };
        self.replay(step.edits);
        self.cursor = self.clamp(step.cursor);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.history.redo() else {
            return false;
        };
        self.replay(step.edits);
        self.cursor = self.clamp(step.cursor);
        true
    }

    /// More than one branch out of here means an undo went back past a fork,
    /// and the other branch is still reachable.
    pub fn undo_branches(&self) -> usize {
        self.history.branches()
    }

    /// History steps are replayed rather than re-committed — recording them
    /// would make undo itself undoable and the tree would never terminate.
    /// Applies the stored order exactly. A group's edits are sequential and
    /// dependent, so sorting them here would be wrong.
    fn replay(&mut self, edits: Vec<Edit>) {
        self.splice(&edits);
    }

    /// Keep the cursor inside the text, and off the newline — normal mode sits
    /// *on* a character, not past the end of the line.
    pub fn clamp(&self, cursor: Cursor) -> Cursor {
        let len = self.text.len_chars();
        let offset = cursor.offset.min(len);
        let limit = motion::line_end(self.text.slice(..), offset);
        let start = motion::line_start(self.text.slice(..), offset);
        let clamped = offset.min(limit.max(start));
        Cursor {
            offset: clamped,
            anchor: cursor.anchor.min(len),
            goal_column: cursor.goal_column,
        }
    }

    /* ── motions, bound to this buffer ────────────────────────────── */

    pub fn word_forward(&self, count: usize, big: bool) -> usize {
        let mut at = self.cursor.offset;
        for _ in 0..count.max(1) {
            at = motion::word_forward(self.text.slice(..), at, big);
        }
        at
    }

    pub fn word_backward(&self, count: usize, big: bool) -> usize {
        let mut at = self.cursor.offset;
        for _ in 0..count.max(1) {
            at = motion::word_backward(self.text.slice(..), at, big);
        }
        at
    }

    pub fn word_end(&self, count: usize, big: bool) -> usize {
        let mut at = self.cursor.offset;
        for _ in 0..count.max(1) {
            at = motion::word_end(self.text.slice(..), at, big);
        }
        at
    }

    pub fn line_start(&self) -> usize {
        motion::line_start(self.text.slice(..), self.cursor.offset)
    }

    pub fn line_end(&self) -> usize {
        motion::line_end(self.text.slice(..), self.cursor.offset)
    }

    pub fn first_non_blank(&self) -> usize {
        motion::first_non_blank(self.text.slice(..), self.cursor.offset)
    }

    pub fn line_range(&self, count: usize) -> Range {
        motion::line_range(self.text.slice(..), self.cursor.offset, count)
    }

    pub fn column(&self) -> usize {
        motion::column_of(self.text.slice(..), self.cursor.offset)
    }

    /// The goal column, remembered across vertical moves.
    pub fn goal(&self) -> usize {
        self.cursor.goal_column.unwrap_or_else(|| self.column())
    }

    pub fn line_down(&self, count: usize) -> usize {
        motion::line_down(self.text.slice(..), self.cursor.offset, count, self.goal())
    }

    pub fn line_up(&self, count: usize) -> usize {
        motion::line_up(self.text.slice(..), self.cursor.offset, count, self.goal())
    }

    pub fn file_start(&self) -> usize {
        0
    }

    pub fn file_end(&self) -> usize {
        motion::line_start(self.text.slice(..), self.text.len_chars())
    }

    pub fn inner_word(&self, big: bool) -> Range {
        motion::inner_word(self.text.slice(..), self.cursor.offset, big)
    }

    pub fn a_word(&self, big: bool) -> Range {
        motion::a_word(self.text.slice(..), self.cursor.offset, big)
    }

    pub fn inside_pair(&self, open: char, close: char, around: bool) -> Option<Range> {
        motion::inside_pair(self.text.slice(..), self.cursor.offset, open, close, around)
    }

    pub fn slice(&self, range: Range) -> String {
        let len = self.text.len_chars();
        self.text
            .slice(range.start.min(len)..range.end.min(len))
            .to_string()
    }
}
