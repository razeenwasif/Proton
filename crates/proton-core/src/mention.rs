//! `@selection`, `@dataframe`, `@game`: naming what the model may look at.
//!
//! The design's phrasing is the specification: *what the model can see is a
//! chip you can point at*. A question about "this" is a question the model
//! answers by guessing, and the usual fix — letting it grep until it finds
//! something that looks right — is slower, costs more, and is wrong often
//! enough to matter. A mention says exactly what "this" is.
//!
//! ## The words stay; the context is appended
//!
//! `@selection` is not replaced by the selected text. The transcript keeps
//! what was actually typed, and the context arrives after it under a heading.
//! Substituting inline would make a short question unreadable in the pane it
//! is drawn in, and would leave the reader unable to tell which parts of their
//! own sentence they wrote.
//!
//! ## An unresolved mention says so
//!
//! `@selection` with nothing selected appends a line saying nothing is
//! selected, rather than nothing at all. Silence would leave the model
//! answering about whatever it could find, and the user believing it had been
//! shown something.

/// Something the composer can point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mention {
    /// The text selected in the editor.
    Selection,
    /// The table the selected notebook cell produced.
    Dataframe,
    /// The game open in Game mode, and the room on screen.
    Game,
    /// The canvas open in Canvas mode — and small pixel art's pixels.
    Canvas,
    /// The flow on screen in Diffuse: its nodes, values and links, and what
    /// stops it running.
    Flow,
}

impl Mention {
    /// As it is written.
    pub fn word(self) -> &'static str {
        match self {
            Mention::Selection => "@selection",
            Mention::Dataframe => "@dataframe",
            Mention::Game => "@game",
            Mention::Canvas => "@canvas",
            Mention::Flow => "@flow",
        }
    }

    /// Every mention this build understands, for the composer's chips.
    pub const ALL: [Mention; 5] = [Mention::Selection, Mention::Dataframe, Mention::Game, Mention::Canvas, Mention::Flow];
}

/// The mentions in a question, in the order they were written, without repeats.
///
/// Repeats are dropped because two `@selection`s mean one selection, and
/// sending it twice would double the cost of asking about it emphatically.
pub fn found(text: &str) -> Vec<Mention> {
    let mut out = Vec::new();
    for mention in Mention::ALL {
        if mentions(text, mention.word()) && !out.contains(&mention) {
            out.push(mention);
        }
    }
    // In the order they appear, so the context reads in the order it was asked
    // for rather than in the order this file happens to list them.
    out.sort_by_key(|m| text.find(m.word()).unwrap_or(usize::MAX));
    out
}

/// Whether `word` appears as a mention rather than inside something else.
///
/// The `@` has to start a word and the mention has to end one. Without the
/// first, `foo@selection.com` is an address that quietly attaches a file to a
/// question about email. Without the second, `@selections` would match
/// `@selection` and then leave a stray `s`.
fn mentions(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(word) {
        let start = from + at;
        let end = start + word.len();

        let before_ok = start == 0
            || text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric() && c != '.' && c != '_' && c != '@');
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');

        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_is_found() {
        assert_eq!(found("explain @selection"), [Mention::Selection]);
        assert_eq!(found("what is in @dataframe?"), [Mention::Dataframe]);
    }

    #[test]
    fn the_modes_own_are_ones_too() {
        assert_eq!(found("shade @game, then @selection"), [Mention::Game, Mention::Selection]);
        assert_eq!(found("why can't I reach the ledge in @game?"), [Mention::Game]);
        assert!(found("@games are fun").is_empty());
        assert_eq!(found("shade the sword in @canvas"), [Mention::Canvas]);
        assert!(found("@canvases").is_empty());
        assert_eq!(found("run @flow at 1216×832"), [Mention::Flow]);
        assert!(found("@flows").is_empty());
        assert!(found("mail@game.org and @gamers").is_empty());
        assert!(found("@log and @pdf went with LaTeX and the Reader").is_empty());
    }

    #[test]
    fn both_can_be_asked_for_at_once() {
        let out = found("does @selection agree with @dataframe");
        assert_eq!(out, [Mention::Selection, Mention::Dataframe]);
    }

    #[test]
    fn they_come_back_in_the_order_they_were_written() {
        // The context reads in the order it was asked for, not in the order
        // this module happens to list them.
        let out = found("compare @dataframe against @selection");
        assert_eq!(out, [Mention::Dataframe, Mention::Selection]);
    }

    #[test]
    fn saying_it_twice_does_not_send_it_twice() {
        // Two `@selection`s mean one selection. Sending it again would double
        // the cost of asking emphatically.
        assert_eq!(found("@selection and @selection"), [Mention::Selection]);
    }

    #[test]
    fn an_email_address_is_not_a_mention() {
        // The failure this exists to prevent: a question about email quietly
        // attaching a file to itself.
        assert!(found("write to bob@selection.com").is_empty());
        assert!(found("ping me@dataframe.io").is_empty());
    }

    #[test]
    fn a_longer_word_is_not_a_mention() {
        assert!(found("the @selections list").is_empty());
        assert!(found("@dataframes").is_empty());
    }

    #[test]
    fn punctuation_after_it_is_fine() {
        assert_eq!(found("what is @selection?"), [Mention::Selection]);
        assert_eq!(found("(@dataframe)"), [Mention::Dataframe]);
        assert_eq!(found("@selection, please"), [Mention::Selection]);
    }

    #[test]
    fn a_mention_at_the_very_start_or_end_counts() {
        assert_eq!(found("@selection"), [Mention::Selection]);
        assert_eq!(found("look at @dataframe"), [Mention::Dataframe]);
    }

    #[test]
    fn a_question_with_no_mentions_has_none() {
        assert!(found("why is the grid janking?").is_empty());
        assert!(found("").is_empty());
    }

    #[test]
    fn an_at_sign_alone_is_not_one() {
        assert!(found("@ selection").is_empty());
        assert!(found("look @ this").is_empty());
    }
}
