//! Where an [`Action`] becomes an edit.
//!
//! The modal engine says *what* — `Delete · InnerWord · 2` — and this decides
//! what that means for a buffer. Keeping the two apart is what lets Video mode
//! answer the same action with a ripple delete without either side knowing
//! about the other.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_text::edit::Range;
use proton_text::{motion, Buffer, Cursor, Edit};

use crate::action::{Action, Object, Verb};
use crate::block::{Block, PendingInsert};
use crate::vim::VimMode;

/// How an object resolves against a buffer.
enum Resolved {
    /// A place to put the cursor.
    Motion(usize),
    /// A span of characters.
    Span(Range),
    /// A span of whole lines — deleted with their newline, and yanked so that
    /// putting them back makes new lines rather than splicing into one.
    Linewise(Range),
}

#[derive(Debug, Default)]
pub struct Editor {
    buffers: Vec<Buffer>,
    active: usize,
    registers: HashMap<String, Register>,
    /// A block insert waiting for insert mode to end. See `block.rs`.
    block_insert: Option<PendingInsert>,
}

/// What shape the text in a register is, which decides how it comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shape {
    #[default]
    Charwise,
    /// Whole lines: put makes new lines rather than splicing into one.
    Linewise,
    /// A rectangle: put lays it out at the same column on successive lines.
    Blockwise,
}

#[derive(Debug, Clone, Default)]
struct Register {
    text: String,
    shape: Shape,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buffers: vec![Buffer::default()],
            active: 0,
            registers: HashMap::new(),
            block_insert: None,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffers[self.active]
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active]
    }

    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn focus(&mut self, index: usize) -> bool {
        if index < self.buffers.len() {
            self.active = index;
            return true;
        }
        false
    }

    /// Open a file, or focus it if it is already open — the same path twice
    /// must not become two buffers with diverging histories.
    pub fn open(&mut self, path: impl AsRef<Path>) -> std::io::Result<usize> {
        let path = path.as_ref();
        // Canonicalised so that two spellings of one file cannot become two
        // buffers, then un-prefixed, because on Windows canonicalising hands
        // back `\\?\C:\...` — which is the same file, but is neither what a
        // statusline should show nor a spelling any language server accepts.
        // The display path and the LSP path want the same normalisation, so
        // they get it in the same place.
        let canonical = path
            .canonicalize()
            .map(proton_lsp::uri::plain)
            .unwrap_or_else(|_| PathBuf::from(path));
        if let Some(i) = self
            .buffers
            .iter()
            .position(|b| b.path().map(|p| p == canonical.as_path()).unwrap_or(false))
        {
            self.active = i;
            return Ok(i);
        }
        let buffer = Buffer::open(&canonical)?;
        self.buffers.push(buffer);
        self.active = self.buffers.len() - 1;
        Ok(self.active)
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        self.buffer_mut().save()
    }

    /// Close a buffer, and say whether there was one to close.
    ///
    /// The editor always has at least one buffer — `buffer()` indexes into the
    /// list and every caller assumes it succeeds — so closing the last one
    /// leaves a fresh empty buffer rather than nothing to draw. That is also
    /// what a user means by it: an editor showing no editor is not a state
    /// anybody asked for.
    ///
    /// Which one you land on afterwards is the tab-bar convention rather than
    /// vim's: the neighbour to the left, or the new last one if you closed off
    /// the end.
    pub fn close(&mut self, index: usize) -> bool {
        if index >= self.buffers.len() {
            return false;
        }
        self.buffers.remove(index);

        if self.buffers.is_empty() {
            self.buffers.push(Buffer::default());
            self.active = 0;
        } else if index < self.active {
            // Everything after it shifted down; the same buffer is still open.
            self.active -= 1;
        } else if index == self.active {
            self.active = index.min(self.buffers.len() - 1);
        }

        // A block insert is a promise to type into a rectangle of the buffer
        // that was active when it started, and that buffer may have just gone.
        self.block_insert = None;
        true
    }

    /* ── executing actions ────────────────────────────────────────── */

    /// Run one action. Returns whether the buffer changed, so the caller knows
    /// whether to send a delta or just a cursor move.
    pub fn execute(&mut self, action: &Action, mode: VimMode) -> bool {
        let count = action.count.max(1) as usize;
        let visual = matches!(
            mode,
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock
        );

        match action.verb {
            Verb::Move => {
                let buf = self.buffer_mut();
                let Some(resolved) = resolve(buf, action.object, count) else {
                    return false;
                };
                let target = match resolved {
                    Resolved::Motion(at) => at,
                    Resolved::Span(r) | Resolved::Linewise(r) => r.start,
                };
                // Vertical motions keep the goal column; everything else sets it.
                let vertical = matches!(action.object, Object::LineDown | Object::LineUp);
                let goal = if vertical { Some(buf.goal()) } else { None };
                let cursor = if visual {
                    buf.cursor.extended_to(target)
                } else {
                    buf.cursor.moved_to(target)
                };
                buf.cursor = buf.clamp(Cursor {
                    goal_column: goal,
                    ..cursor
                });
                false
            }

            // A block is several ranges rather than one, so it turns off here
            // rather than being squeezed through machinery built for a run.
            Verb::Delete | Verb::Change | Verb::Yank
                if mode == VimMode::VisualBlock && action.object == Object::Selection =>
            {
                self.block_operator(action)
            }

            Verb::Delete | Verb::Change | Verb::Yank => {
                let Some((range, linewise)) = self.span_for(action, count, visual) else {
                    return false;
                };
                if range.is_empty() {
                    return false;
                }
                let text = self.buffer().slice(range);
                self.write_register(
                    action.register.as_deref(),
                    text,
                    if linewise {
                        Shape::Linewise
                    } else {
                        Shape::Charwise
                    },
                );

                if action.verb == Verb::Yank {
                    let buf = self.buffer_mut();
                    buf.cursor = buf.clamp(buf.cursor.moved_to(range.start));
                    return false;
                }

                let buf = self.buffer_mut();
                buf.apply(vec![Edit::delete(range)], Cursor::at(range.start));
                true
            }

            Verb::Insert => {
                let buf = self.buffer_mut();
                match action.object {
                    Object::After => {
                        let at = (buf.cursor.offset + 1).min(buf.line_end());
                        buf.cursor = Cursor::at(at);
                    }
                    Object::LineStart | Object::FirstNonBlank => {
                        let at = buf.first_non_blank();
                        buf.cursor = Cursor::at(at);
                    }
                    Object::LineEnd => {
                        let at = buf.line_end();
                        buf.cursor = Cursor::at(at);
                    }
                    Object::LineBelow => {
                        let at = buf.line_end();
                        buf.apply(vec![Edit::insert(at, "\n")], Cursor::at(at + 1));
                        return true;
                    }
                    Object::LineAbove => {
                        let at = buf.line_start();
                        buf.apply(vec![Edit::insert(at, "\n")], Cursor::at(at));
                        return true;
                    }
                    Object::BlockStart | Object::BlockEnd => {
                        self.begin_block_insert(action.object == Object::BlockEnd);
                        return false;
                    }
                    _ => {}
                }
                false
            }

            Verb::Paste => {
                let reg = self.read_register(action.register.as_deref());
                if reg.text.is_empty() {
                    return false;
                }
                if reg.shape == Shape::Blockwise {
                    return self.paste_block(&reg.text, action.object == Object::Here, count);
                }
                let buf = self.buffer_mut();
                let (at, cursor) = if reg.shape == Shape::Linewise {
                    // Linewise put goes to a new line, above for `P`.
                    let at = if action.object == Object::Here {
                        buf.line_start()
                    } else {
                        let end = buf.line_end();
                        (end + 1).min(buf.len_chars())
                    };
                    (at, at)
                } else {
                    let at = if action.object == Object::Here {
                        buf.cursor.offset
                    } else {
                        (buf.cursor.offset + 1).min(buf.line_end().max(buf.cursor.offset))
                    };
                    (at, at + reg.text.chars().count().saturating_sub(1))
                };
                let text = reg.text.repeat(count);
                buf.apply(vec![Edit::insert(at, text)], Cursor::at(cursor));
                true
            }

            Verb::Undo => {
                let mut changed = false;
                for _ in 0..count {
                    changed |= self.buffer_mut().undo();
                }
                changed
            }

            Verb::Redo => {
                let mut changed = false;
                for _ in 0..count {
                    changed |= self.buffer_mut().redo();
                }
                changed
            }

            // `.` never reaches here: the engine expands it into the steps it
            // stands for, so the editor only ever sees real verbs.
            Verb::Repeat => false,

            Verb::SetMark => {
                if let Object::Mark { name } = action.object {
                    let buf = self.buffer_mut();
                    let at = buf.cursor.offset;
                    buf.set_mark(name, at);
                }
                false
            }

            // `gd` and `gr` are answered by a language server and land in a
            // file this editor may not have open yet; `K` is answered beside
            // the cursor and touches nothing at all. All three belong to the
            // workspace, so reaching here means nobody had an answer — a
            // no-op rather than a failure.
            Verb::Goto | Verb::Explain => false,
        }
    }

    /// A character typed in insert mode.
    pub fn insert_char(&mut self, c: char) {
        let buf = self.buffer_mut();
        let at = buf.cursor.offset;
        buf.apply(vec![Edit::insert(at, c.to_string())], Cursor::at(at + 1));
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn backspace(&mut self) -> bool {
        let buf = self.buffer_mut();
        let at = buf.cursor.offset;
        if at == 0 {
            return false;
        }
        buf.apply(
            vec![Edit::delete(Range::new(at - 1, at))],
            Cursor::at(at - 1),
        );
        true
    }

    /* ── helpers ──────────────────────────────────────────────────── */

    /// The range an operator applies to, and whether it is linewise.
    fn span_for(&mut self, action: &Action, count: usize, visual: bool) -> Option<(Range, bool)> {
        let buf = self.buffer_mut();
        if visual && action.object == Object::Selection {
            let sel = buf.cursor.selection();
            // Visual selections are inclusive of the character under the cursor.
            return Some((
                Range::new(sel.start, (sel.end + 1).min(buf.len_chars())),
                false,
            ));
        }
        match resolve(buf, action.object, count)? {
            Resolved::Linewise(r) => Some((r, true)),
            Resolved::Span(r) => Some((r, false)),
            // Some motions are linewise even though they resolve to a point:
            // `dj` takes two whole lines, not a rectangle of characters, and
            // `d'a` takes every line between here and the mark. Expanding the
            // span here rather than at each object keeps `resolve` answering
            // one question — where — and this one answering the other.
            Resolved::Motion(at) if linewise(action.object) => {
                let from = buf.cursor.offset.min(at);
                let to = buf.cursor.offset.max(at);
                let lines = buf.position(to).line - buf.position(from).line + 1;
                let start = buf.position(from).line;
                Some((buf.line_span(start, lines), true))
            }
            Resolved::Motion(at) => Some((Range::new(buf.cursor.offset, at), false)),
        }
    }

    fn write_register(&mut self, name: Option<&str>, text: String, shape: Shape) {
        let reg = Register { text, shape };
        // The unnamed register always gets a copy, exactly as in vim.
        self.registers.insert("\"".into(), reg.clone());
        if let Some(name) = name {
            self.registers.insert(name.to_string(), reg);
        }
    }

    fn read_register(&self, name: Option<&str>) -> Register {
        self.registers
            .get(name.unwrap_or("\""))
            .cloned()
            .unwrap_or_default()
    }
}

/// Turn an object into something a verb can act on.
fn resolve(buf: &Buffer, object: Object, count: usize) -> Option<Resolved> {
    use Object::*;
    Some(match object {
        Line => Resolved::Linewise(buf.line_range(count)),
        LineDown => Resolved::Motion(buf.line_down(count)),
        LineUp => Resolved::Motion(buf.line_up(count)),
        CharLeft => Resolved::Motion(buf.cursor.offset.saturating_sub(count)),
        CharRight => Resolved::Motion((buf.cursor.offset + count).min(buf.line_end())),
        WordForward => Resolved::Motion(buf.word_forward(count, false)),
        WordBackward => Resolved::Motion(buf.word_backward(count, false)),
        WordEnd => Resolved::Motion(buf.word_end(count, false)),
        BigWordForward => Resolved::Motion(buf.word_forward(count, true)),
        BigWordBackward => Resolved::Motion(buf.word_backward(count, true)),
        BigWordEnd => Resolved::Motion(buf.word_end(count, true)),
        LineStart => Resolved::Motion(buf.line_start()),
        FirstNonBlank => Resolved::Motion(buf.first_non_blank()),
        LineEnd => Resolved::Motion(buf.line_end()),
        FileStart => Resolved::Motion(buf.file_start()),
        FileEnd => Resolved::Motion(buf.file_end()),
        Selection => Resolved::Span(buf.cursor.selection()),
        InnerWord => Resolved::Span(buf.inner_word(false)),
        AroundWord => Resolved::Span(buf.a_word(false)),
        InnerBigWord => Resolved::Span(buf.inner_word(true)),
        AroundBigWord => Resolved::Span(buf.a_word(true)),
        InnerPair { open } => Resolved::Span(buf.inside_pair(open, closing(open), false)?),
        AroundPair { open } => Resolved::Span(buf.inside_pair(open, closing(open), true)?),
        MarkExact { name } => Resolved::Motion(buf.mark(name)?),
        // `'a` lands on the first non-blank of the mark's line, the way `'`
        // always does — a mark remembers a place, and `'` asks for the line
        // it is on rather than the column.
        MarkLine { name } => {
            let at = buf.mark(name)?;
            Resolved::Motion(motion::first_non_blank(buf.text().slice(..), at))
        }
        // `Mark` is where to *put* one, which is the cursor — nothing to
        // resolve. `Definition` is a question for a language server, answered
        // in the workspace where one lives.
        // A block is not one range, so it never resolves — it goes down its
        // own path in `execute`. `Mark` is where to *put* a mark, which is
        // the cursor. `Definition` is a question for a language server,
        // answered in the workspace where one lives.
        BlockStart
        | BlockEnd
        | Mark { .. }
        | Definition
        | References
        | None
        | Here
        | After
        | LineBelow
        | LineAbove => return Option::None,
    })
}

fn closing(open: char) -> char {
    match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        other => other,
    }
}

/// Whether a motion takes whole lines when an operator is waiting on it.
///
/// `dj` deletes two lines rather than a rectangle of characters, and `d'a`
/// takes every line between here and the mark. The objects themselves resolve
/// to a point either way — this is only about what an operator does with it.
fn linewise(object: Object) -> bool {
    matches!(
        object,
        Object::LineDown | Object::LineUp | Object::MarkLine { .. }
    )
}

/* ── visual block ─────────────────────────────────────────────────── */

impl Editor {
    /// Delete, change or yank a rectangle.
    ///
    /// Separate from `span_for` because a block is several ranges and the
    /// operators are built around one. They are applied as a single change,
    /// so one `u` takes the whole rectangle back.
    fn block_operator(&mut self, action: &Action) -> bool {
        let block = Block::of(self.buffer());
        let ranges = block.ranges(self.buffer());

        // Every row, including the ones too short to reach the block. Dropping
        // the empty ones would lose the shape, and a block pasted back without
        // its blank rows comes out crooked.
        let rows: Vec<String> = ranges
            .iter()
            .map(|range| self.buffer().slice(*range))
            .collect();
        self.write_register(
            action.register.as_deref(),
            rows.join("\n"),
            Shape::Blockwise,
        );

        if action.verb == Verb::Yank {
            let buf = self.buffer_mut();
            let at = block.corner(buf);
            buf.cursor = buf.clamp(buf.cursor.moved_to(at));
            return false;
        }

        let edits: Vec<Edit> = ranges
            .iter()
            .filter(|range| !range.is_empty())
            .map(|range| Edit::delete(*range))
            .collect();
        if edits.is_empty() {
            return false;
        }

        let buf = self.buffer_mut();
        let at = block.corner(buf);
        buf.apply(edits, Cursor::at(at));
        true
    }

    /// Start a block insert: put the cursor on the first line's edge and
    /// remember the rest for when insert mode ends.
    fn begin_block_insert(&mut self, at_end: bool) {
        let block = Block::of(self.buffer());
        let buf = self.buffer_mut();

        let column = if at_end { block.right - 1 } else { block.left };
        let lines: Vec<usize> = block
            .lines()
            .skip(1)
            .filter(|line| *line < buf.len_lines())
            .collect();

        let first = buf.offset_of(proton_text::Position {
            line: block.first_line,
            column,
        });
        // `A` on a line shorter than the block's right edge appends at its
        // end; the padding that gets it to the column happens on commit, when
        // it is known how much is needed.
        buf.cursor = Cursor::at(first);

        self.block_insert = Some(PendingInsert {
            lines,
            column,
            from: first,
            pad: at_end,
        });
    }

    /// Insert mode is ending — copy what was typed onto the rest of the block.
    ///
    /// Read back out of the buffer rather than accumulated as it was typed,
    /// because backspace, a completion and anything else that edits during
    /// insert all have to be reflected, and only the buffer knows all of them.
    pub fn finish_block_insert(&mut self) -> bool {
        let Some(pending) = self.block_insert.take() else {
            return false;
        };
        let buf = self.buffer_mut();
        let to = buf.cursor.offset.max(pending.from).min(buf.len_chars());
        let typed = buf.slice(Range::new(pending.from, to));

        // Nothing typed, or the typing left the line it started on: either way
        // there is nothing sensible to replicate.
        if typed.is_empty() || typed.contains('\n') {
            return false;
        }

        let mut edits = Vec::new();
        for line in pending.lines {
            let width = buf.line(line).chars().count();
            let start = buf.offset_of(proton_text::Position { line, column: 0 });

            if pending.column <= width {
                edits.push(Edit::insert(start + pending.column, &typed));
            } else if pending.pad {
                // `A` reaches past the end of a short line by padding it out.
                // `I` does not — a column that line never had is not a place
                // to insert, so it is skipped.
                let padded = format!("{}{typed}", " ".repeat(pending.column - width));
                edits.push(Edit::insert(start + width, &padded));
            }
        }

        if edits.is_empty() {
            return false;
        }
        let cursor = buf.cursor;
        buf.apply(edits, cursor);
        true
    }
}

impl Editor {
    /// Put a rectangle back: each row at the same column on successive lines.
    ///
    /// Short lines are padded to reach the column, because the point of a
    /// block paste is that the result is a column — a row that quietly landed
    /// at the end of a short line instead would make it not one.
    fn paste_block(&mut self, text: &str, before: bool, count: usize) -> bool {
        let buf = self.buffer_mut();
        let start = buf.position(buf.cursor.offset);
        let column = if before {
            start.column
        } else {
            start.column + 1
        };

        let mut edits = Vec::new();
        let mut tail = String::new();

        for (offset, row) in text.split('\n').enumerate() {
            let row = row.repeat(count);
            let line = start.line + offset;
            if line >= buf.len_lines() {
                // Past the end of the buffer: collect the rest into new lines
                // rather than dropping them.
                tail.push('\n');
                tail.push_str(&" ".repeat(column));
                tail.push_str(&row);
                continue;
            }
            let width = buf.line(line).chars().count();
            let at = buf.offset_of(proton_text::Position { line, column: 0 });
            if column <= width {
                edits.push(Edit::insert(at + column, row));
            } else {
                edits.push(Edit::insert(
                    at + width,
                    format!("{}{row}", " ".repeat(column - width)),
                ));
            }
        }

        if !tail.is_empty() {
            let end = buf.len_chars();
            // A file ending in a newline has an empty last line that starts
            // exactly at the end, so a row landing on it and the overflow are
            // two inserts at one offset — which is not an order, it is a race.
            // Merging is what that order would have meant anyway.
            match edits.last_mut().filter(|edit| edit.start == end) {
                Some(edit) => edit.text.push_str(&tail),
                None => edits.push(Edit::insert(end, tail)),
            }
        }
        if edits.is_empty() {
            return false;
        }

        let landing = buf.offset_of(proton_text::Position {
            line: start.line,
            column,
        });
        buf.apply(edits, Cursor::at(landing));
        true
    }
}
