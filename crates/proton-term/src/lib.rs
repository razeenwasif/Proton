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
    /// How far back through the history this view is, in rows. `0` is the
    /// live screen. Sent so the panel can say so: a terminal scrolled back
    /// looks exactly like a terminal where the shell has stopped answering.
    pub scrollback: usize,
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

    let mut cmd = CommandBuilder::new(&program);
    // A login shell on macOS, as Terminal, iTerm and Ghostty start theirs. An
    // app opened from the Dock inherits launchd's bare PATH, and it is the
    // login profile — path_helper in /etc/zprofile, Homebrew's shellenv in
    // ~/.zprofile — that adds the rest. Without it, `brew` and everything it
    // installed was "command not found" in this terminal and nowhere else.
    #[cfg(target_os = "macos")]
    if takes_login_flag(&program) {
        cmd.arg("-l");
    }
    // Programs ask `TERM` what they may draw with. Claiming colour we do
    // render, and nothing we do not.
    cmd.env("TERM", "xterm-256color");
    cmd
}

/// Whether a shell is one known to take `-l` for "login". Anything else is
/// started as it always was, rather than handed a flag it may refuse.
#[cfg(any(target_os = "macos", test))]
fn takes_login_flag(program: &str) -> bool {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    matches!(
        name,
        "sh" | "bash" | "zsh" | "fish" | "ksh" | "mksh" | "dash" | "tcsh" | "csh" | "nu"
    )
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
        // Typing returns to the live screen, the way every terminal does:
        // what you are about to type will appear at the bottom, and being
        // left looking at history while the shell echoes somewhere unseen is
        // the more confusing of the two behaviours.
        self.parser.screen_mut().set_scrollback(0);
        let _ = self.writer.write_all(text.as_bytes());
        let _ = self.writer.flush();
    }

    /// How far back through the history the view is, in rows. `0` is live.
    pub fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Look back through what has scrolled off the top.
    ///
    /// `back` is in rows: positive goes further into the history, negative
    /// comes forward toward the live screen. Answers whether the view
    /// actually moved, so a wheel at either end costs no redraw.
    ///
    /// The bound is neither asked for nor stored. `vt100` clamps to however
    /// much history it really kept — which is not the configured maximum
    /// until the maximum has been reached — so the position is read back
    /// from it afterwards rather than predicted here.
    pub fn scroll(&mut self, back: i32) -> bool {
        let at = self.scrollback();
        let want = (at as i64 + i64::from(back)).max(0);
        let screen = self.parser.screen_mut();
        screen.set_scrollback(usize::try_from(want).unwrap_or(usize::MAX));
        self.scrollback() != at
    }

    /// Back to the live screen.
    pub fn scroll_to_bottom(&mut self) -> bool {
        let moved = self.scrollback() != 0;
        self.parser.screen_mut().set_scrollback(0);
        moved
    }

    /// How much history there is, in rows.
    ///
    /// Asked of `vt100` rather than counted here, and asked the only way it
    /// answers: `set_scrollback` clamps to what it actually kept, so setting
    /// it past the end and reading it back is what tells you the bound. The
    /// position is put back before returning, so this reads as a question.
    fn history(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        let at = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let len = screen.scrollback();
        screen.set_scrollback(at);
        len
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
        // A resize reflows what is on the screen, so a row count taken before
        // it no longer points at the same text. Returning to the bottom is
        // the honest answer; keeping the number would land somewhere arbitrary
        // while looking deliberate.
        self.parser.screen_mut().set_scrollback(0);
    }

    /// Take whatever the shell has said. `true` when the screen changed.
    ///
    /// If the view is scrolled back, it stays on the same *text* rather than
    /// the same offset. The offset counts rows from the bottom, so leaving it
    /// alone while new lines arrive would slide the history downward under
    /// the reader — which is precisely what happens when you scroll back to
    /// read an error from a build that is still printing.
    pub fn poll(&mut self) -> bool {
        let at = self.scrollback();
        let before = if at > 0 { self.history() } else { 0 };
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
        if at > 0 && changed {
            let grew = self.history().saturating_sub(before);
            if grew > 0 {
                self.parser.screen_mut().set_scrollback(at + grew);
            }
        }
        changed
    }

    /// The screen, as runs.
    ///
    /// Scrolled back or not, this is the same code: `vt100`'s `cell` already
    /// answers from the scrollback when there is an offset, so history is
    /// painted by the path that paints the live screen rather than a second
    /// one that could disagree with it.
    pub fn screen(&self) -> Screen {
        let screen = self.parser.screen();
        let (rows, _) = screen.size();
        let scrollback = screen.scrollback();
        Screen {
            rows: (0..rows).map(|row| row_of(screen, row)).collect(),
            cursor: screen.cursor_position(),
            // The cursor belongs to the live screen. Scrolled back, its row
            // means nothing here, and drawing it would put a caret on an
            // arbitrary line of history.
            cursor_visible: !screen.hide_cursor() && scrollback == 0,
            scrollback,
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

    /// A parser with history, filled past the top of its screen.
    ///
    /// Four rows and ten lines, so six have scrolled off — enough that being
    /// off by one in either direction shows up as the wrong number.
    fn filled() -> vt100::Parser {
        let mut parser = vt100::Parser::new(4, 20, 100);
        for n in 1..=10 {
            parser.process(format!("line{n}\r\n").as_bytes());
        }
        parser
    }

    fn rows_of(parser: &vt100::Parser) -> Vec<String> {
        let screen = parser.screen();
        (0..screen.size().0)
            .map(|row| line(&row_of(screen, row)))
            .collect()
    }

    #[test]
    fn history_is_painted_by_the_same_path_as_the_live_screen() {
        // The reason `screen()` needed no second code path: `vt100`'s `cell`
        // answers from the scrollback once there is an offset, so `row_of`
        // paints history without knowing there is any.
        let mut parser = filled();
        let live = rows_of(&parser);
        assert!(live.contains(&"line10".to_string()), "{live:?}");
        assert!(!live.contains(&"line4".to_string()), "{live:?}");

        parser.screen_mut().set_scrollback(3);
        let back = rows_of(&parser);
        assert!(
            back.contains(&"line7".to_string()),
            "three rows back should reach line7: {back:?}"
        );
        assert!(
            !back.contains(&"line10".to_string()),
            "and should have left the newest behind: {back:?}"
        );
    }

    #[test]
    fn asking_past_the_end_of_the_history_stops_at_the_oldest_line() {
        // What `scroll` relies on instead of tracking a bound itself: vt100
        // clamps, so the position read back afterwards is the real one.
        let mut parser = filled();
        parser.screen_mut().set_scrollback(usize::MAX);
        let kept = parser.screen().scrollback();
        assert!(kept > 0 && kept < usize::MAX, "clamped to {kept}");

        let oldest = rows_of(&parser);
        assert!(
            oldest.contains(&"line1".to_string()),
            "the very top of the history: {oldest:?}"
        );
    }

    #[test]
    fn only_shells_known_to_take_it_are_started_as_login_shells() {
        // `-l` is what lets a terminal opened from the Dock find Homebrew, and
        // it is a flag some shells would refuse rather than ignore.
        assert!(takes_login_flag("/bin/zsh"));
        assert!(takes_login_flag("/opt/homebrew/bin/fish"));
        assert!(takes_login_flag("bash"));
        assert!(!takes_login_flag("/usr/local/bin/elvish"));
        assert!(!takes_login_flag(""));
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
