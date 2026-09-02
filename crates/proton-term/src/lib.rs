//! A terminal: a pseudo-terminal, and the screen it paints.
//!
//! The same arrangement as every other foreign process here — the shell runs
//! out of process, a reader thread pumps its bytes into a channel, and the
//! core drains that on its heartbeat. Nothing blocks the keystroke path.
//!
//! ## ConPTY asks a question, and stalls until it is answered
//!
//! The first thing a Windows pseudo-console sends is `ESC[6n` — "where is the
//! cursor?" — and it will not pump the child's output until something answers
//! with `ESC[<row>;<col>R`. A terminal that ignores it gets four bytes and then
//! silence, which looks exactly like the shell failing to start.
//!
//! That is measured rather than read: the probe that found it saw a live child
//! process, no output at all, and those four bytes sitting in the buffer.
//! `vt100` parses the screen but does not answer queries, so the reply is sent
//! from here.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// One run of characters sharing a look.
///
/// Runs rather than cells: a terminal row is mostly one colour, and a cell per
/// character would put eighty objects on the bridge for a line of prose. The
/// editor's viewport merges its spans for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    /// An ANSI palette index, when the colour came from one.
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    /// A true-colour value, when it did not.
    pub rgb: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// What the screen looks like now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Screen {
    pub rows: Vec<Vec<Run>>,
    pub cursor: (u16, u16),
    /// Whether the cursor should be drawn at all — a program can hide it.
    pub cursor_visible: bool,
}

pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    incoming: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
    /// Whether the shell is still there. A dead terminal keeps its last
    /// screen: what a command printed before it exited is the useful part.
    alive: bool,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("alive", &self.alive)
            .finish()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The shell to run, and what to tell programs about the terminal.
fn shell() -> CommandBuilder {
    #[cfg(windows)]
    let program = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
    #[cfg(not(windows))]
    let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());

    let mut cmd = CommandBuilder::new(program);
    // Programs ask `TERM` what they may draw with. Claiming colour we do
    // render, and nothing we do not.
    cmd.env("TERM", "xterm-256color");
    cmd
}

impl Terminal {
    /// Start a shell in `cwd`.
    pub fn open(cwd: &std::path::Path, rows: u16, cols: u16) -> std::io::Result<Terminal> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(other)?;

        let mut cmd = shell();
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd).map_err(other)?;
        // The slave has to go, or nothing ever reaches end-of-file and the
        // reader thread outlives the shell.
        drop(pair.slave);

        let writer = pair.master.take_writer().map_err(other)?;
        let mut reader = pair.master.try_clone_reader().map_err(other)?;
        let (found, incoming) = mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name("proton-term-read".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if found.send(buf[..n].to_vec()).is_err() {
                                return;
                            }
                        }
                    }
                }
            })?;

        Ok(Terminal {
            master: pair.master,
            writer,
            child,
            incoming,
            // Scrollback, so what went past the top is still there.
            parser: vt100::Parser::new(rows, cols, 2000),
            rows,
            cols,
            alive: true,
        })
    }

    pub fn is_alive(&mut self) -> bool {
        if self.alive && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.alive = false;
        }
        self.alive
    }

    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Type at it.
    pub fn write(&mut self, text: &str) {
        let _ = self.writer.write_all(text.as_bytes());
        let _ = self.writer.flush();
    }

    /// Tell the shell the window changed. Programs redraw from this.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Take whatever the shell has said. `true` when the screen changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.incoming.try_recv() {
                Ok(chunk) => {
                    // ConPTY will not pump the child's output until its cursor
                    // query is answered. See the note at the top of this file.
                    if find(&chunk, b"\x1b[6n") {
                        let (row, col) = self.parser.screen().cursor_position();
                        let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                        let _ = self.writer.write_all(reply.as_bytes());
                        let _ = self.writer.flush();
                    }
                    self.parser.process(&chunk);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.alive {
                        self.alive = false;
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }

    /// The screen, as runs.
    pub fn screen(&self) -> Screen {
        let screen = self.parser.screen();
        let (rows, _) = screen.size();
        Screen {
            rows: (0..rows).map(|row| row_of(screen, row)).collect(),
            cursor: screen.cursor_position(),
            cursor_visible: !screen.hide_cursor(),
        }
    }
}

/// One row of a parsed screen, as runs.
///
/// A free function because it has nothing to do with the pseudo-terminal: it
/// turns cells into runs, and a test can hand it a screen it built itself
/// rather than starting a shell to check a colour.
pub fn row_of(screen: &vt100::Screen, row: u16) -> Vec<Run> {
    let (_, cols) = screen.size();
    let mut runs: Vec<Run> = Vec::new();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        let text = cell.contents();
        // An empty cell is a space. Dropping it would close up the gaps that
        // alignment is made of.
        let text: String = if text.is_empty() {
            " ".into()
        } else {
            text.into()
        };
        let look = look(cell);
        match runs.last_mut() {
            Some(last) if same(last, &look) => last.text.push_str(&text),
            _ => runs.push(Run { text, ..look }),
        }
    }
    trim(&mut runs);
    runs
}

/// Drop the terminal's right-hand padding.
///
/// A blank cell is still a cell, so every row arrives eighty columns wide
/// whatever is on it. The trailing blanks are padding rather than content, and
/// sending them makes every line the full width for no reason.
///
/// The last run usually *ends* in spaces rather than being made of them —
/// merging joins the text and the padding into one — so the trailing spaces
/// have to be trimmed off it, not just whole runs popped. Popping alone left
/// every row padded, which is what the first live run showed.
/// Only ever *unpainted* blanks. A run with a background colour is drawing
/// those spaces on purpose — a status bar, a selection — and trimming them
/// cuts the bar short.
fn trim(runs: &mut Vec<Run>) {
    while runs
        .last()
        .is_some_and(|run| run.text.trim().is_empty() && plain(run))
    {
        runs.pop();
    }
    if let Some(last) = runs.last_mut() {
        if plain(last) {
            let end = last.text.trim_end_matches(' ').len();
            last.text.truncate(end);
        }
    }
}

fn look(cell: &vt100::Cell) -> Run {
    let (fg, rgb) = colour(cell.fgcolor());
    let (bg, bg_rgb) = colour(cell.bgcolor());
    Run {
        text: String::new(),
        fg,
        bg,
        // One true colour per run is enough: a cell with both a custom
        // foreground and background is vanishingly rare, and the foreground is
        // the one you read.
        rgb: rgb.or(bg_rgb),
        bold: cell.bold(),
        italic: cell.italic(),
        underline: cell.underline(),
        inverse: cell.inverse(),
    }
}

fn colour(from: vt100::Color) -> (Option<u8>, Option<(u8, u8, u8)>) {
    match from {
        vt100::Color::Default => (None, None),
        vt100::Color::Idx(i) => (Some(i), None),
        vt100::Color::Rgb(r, g, b) => (None, Some((r, g, b))),
    }
}

fn same(a: &Run, b: &Run) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.rgb == b.rgb
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.inverse == b.inverse
}

fn plain(run: &Run) -> bool {
    run.fg.is_none() && run.bg.is_none() && run.rgb.is_none() && !run.inverse && !run.underline
}

fn find(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn other(err: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen built by feeding bytes to the parser, with no shell involved.
    ///
    /// The conversion has nothing to do with the pseudo-terminal, so it does
    /// not need one — and a test that starts a shell to check a colour is a
    /// test that fails when the shell is not the one you expected.
    fn painted(bytes: &str) -> Vec<Vec<Run>> {
        let mut parser = vt100::Parser::new(4, 20, 0);
        parser.process(bytes.as_bytes());
        let screen = parser.screen();
        (0..screen.size().0)
            .map(|row| row_of(screen, row))
            .collect()
    }

    fn line(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn a_plain_line_is_one_run_with_no_padding() {
        // Every cell on the row exists, so without trimming this arrives
        // twenty columns wide however short the text is.
        let rows = painted("hello");
        assert_eq!(rows[0].len(), 1);
        assert_eq!(line(&rows[0]), "hello");
    }

    #[test]
    fn an_empty_row_has_no_runs_at_all() {
        let rows = painted("hello");
        assert!(rows[1].is_empty(), "{:?}", rows[1]);
    }

    #[test]
    fn colour_survives_and_splits_the_run() {
        let rows = painted("plain \x1b[31mred\x1b[0m done");
        assert_eq!(line(&rows[0]), "plain red done");

        let red = rows[0]
            .iter()
            .find(|r| r.text.contains("red"))
            .expect("a red run");
        assert_eq!(red.fg, Some(1), "ANSI 1 is red");
        assert!(rows[0][0].fg.is_none(), "the text before it is not");
    }

    #[test]
    fn true_colour_comes_through_as_itself() {
        let rows = painted("\x1b[38;2;120;170;255mblue\x1b[0m");
        let run = &rows[0][0];
        assert_eq!(run.rgb, Some((120, 170, 255)));
        assert_eq!(run.fg, None, "not squeezed into a palette index");
    }

    #[test]
    fn attributes_are_kept() {
        let rows = painted("\x1b[1mbold\x1b[0m \x1b[4munder\x1b[0m");
        assert!(rows[0].iter().any(|r| r.bold && r.text.contains("bold")));
        assert!(rows[0]
            .iter()
            .any(|r| r.underline && r.text.contains("under")));
    }

    #[test]
    fn adjacent_cells_that_look_alike_become_one_run() {
        // A row of prose should not cross the bridge as eighty objects.
        let rows = painted("aaaaaaaaaa");
        assert_eq!(rows[0].len(), 1);
    }

    #[test]
    fn a_painted_background_keeps_its_spaces() {
        // Those spaces are being drawn on purpose — a selection bar, a status
        // line — and trimming them would cut the bar short.
        let rows = painted("\x1b[41m   \x1b[0m");
        assert_eq!(line(&rows[0]), "   ", "{:?}", rows[0]);
        assert_eq!(rows[0][0].bg, Some(1));
    }
}
