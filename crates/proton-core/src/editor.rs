//! Where an [`Action`] becomes an edit.
//!
//! The modal engine says *what* — `Delete · InnerWord · 2` — and this decides
//! what that means for a buffer. Keeping the two apart is what lets Video mode
//! answer the same action with a ripple delete without either side knowing
//! about the other.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_text::edit::Range;
use proton_text::{Buffer, Cursor, Edit};

use crate::action::{Action, Object, Verb};
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
}

#[derive(Debug, Clone, Default)]
struct Register {
    text: String,
    linewise: bool,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buffers: vec![Buffer::default()],
            active: 0,
            registers: HashMap::new(),
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
        let canonical = path.canonicalize().unwrap_or_else(|_| PathBuf::from(path));
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

    /* ── executing actions ────────────────────────────────────────── */

    /// Run one action. Returns whether the buffer changed, so the caller knows
    /// whether to send a delta or just a cursor move.
    pub fn execute(&mut self, action: &Action, mode: VimMode) -> bool {
        let count = action.count.max(1) as usize;
        let visual = matches!(mode, VimMode::Visual | VimMode::VisualLine);

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

            Verb::Delete | Verb::Change | Verb::Yank => {
                let Some((range, linewise)) = self.span_for(action, count, visual) else {
                    return false;
                };
                if range.is_empty() {
                    return false;
                }
                let text = self.buffer().slice(range);
                self.write_register(action.register.as_deref(), text, linewise);

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
                    _ => {}
                }
                false
            }

            Verb::Paste => {
                let reg = self.read_register(action.register.as_deref());
                if reg.text.is_empty() {
                    return false;
                }
                let buf = self.buffer_mut();
                let (at, cursor) = if reg.linewise {
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
            Resolved::Motion(at) => Some((Range::new(buf.cursor.offset, at), false)),
        }
    }

    fn write_register(&mut self, name: Option<&str>, text: String, linewise: bool) {
        let reg = Register { text, linewise };
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
        None | Here | After | LineBelow | LineAbove => return Option::None,
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
