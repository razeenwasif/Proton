//! Writing `scene.proton.json` so that a diff of it is worth reading.
//!
//! ## Why this is hand-written rather than `serde_json`'s pretty printer
//!
//! The pretty printer puts every array element on its own line. A position is
//! three numbers, so a scene of two hundred objects becomes a file of many
//! thousand lines in which a nudge shows up as
//!
//! ```text
//! -        0.0,
//! +        0.25,
//! ```
//!
//! — a hunk that says nothing about which object moved or along what axis. The
//! same change written as `"position": [0.25, 0, 0]` is one line that answers
//! all of it. Nothing in `serde_json`'s `Formatter` can express "compact for
//! arrays of numbers and expanded for everything else", because a formatter is
//! streaming and cannot see what an array holds before it starts writing it.
//!
//! Sixty lines of writer buys a file somebody can review, which is the entire
//! point of the load-bearing rule this format exists to obey.
//!
//! ## Numbers are rounded on the way out
//!
//! A viewport drag produces `0.30000000000000004`. Written verbatim it is
//! noise in the file, noise in the diff, and it changes again the next time
//! anybody touches it. Six decimal places is finer than any modelling tool
//! shows and far finer than a millimetre at architectural scale.

use serde_json::Value;

/// How many decimal places survive a save.
const PLACES: usize = 6;

/// Two spaces, which is what everything else in this project's JSON uses.
const INDENT: &str = "  ";

/// A scene as text.
pub fn to_text(value: &Value) -> String {
    let mut out = String::new();
    write(&mut out, value, 0);
    out.push('\n');
    out
}

fn write(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            out.push_str("{\n");
            for (i, (key, child)) in map.iter().enumerate() {
                pad(out, depth + 1);
                out.push_str(&string(key));
                out.push_str(": ");
                write(out, child, depth + 1);
                if i + 1 < map.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, depth);
            out.push('}');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        // The reason this file exists. A vector, a colour, a quaternion, a
        // list of vertex indices — all of them read as one thing and should
        // occupy one line.
        Value::Array(items) if items.iter().all(Value::is_number) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(out, item, depth);
            }
            out.push(']');
        }
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(out, depth + 1);
                write(out, item, depth + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, depth);
            out.push(']');
        }
        Value::String(text) => out.push_str(&string(text)),
        Value::Number(n) => out.push_str(&number(n)),
        Value::Bool(b) => out.push_str(match b {
            true => "true",
            false => "false",
        }),
        Value::Null => out.push_str("null"),
    }
}

fn pad(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

/// A number, short enough to read and stable enough to diff.
fn number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let Some(f) = n.as_f64() else {
        return n.to_string();
    };
    // Not finite: JSON has no way to write it, and `serde_json` turns it into
    // null on the way in, so this is only reachable through a value built in
    // memory. Zero is a lie, but a scene with a NaN position in it is already
    // broken and a file that cannot be parsed back is worse.
    if !f.is_finite() {
        return "0".into();
    }
    let mut text = format!("{f:.PLACES$}");
    // `1.500000` reads worse than `1.5`, and `0.000000` worse than `0`.
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    // `-0` is a real f64 and an unhelpful thing to see in a file.
    match text == "-0" {
        true => "0".into(),
        false => text,
    }
}

fn string(text: &str) -> String {
    // Through `serde_json`, so the escaping is exactly what a parser expects
    // rather than exactly what this author remembered.
    Value::String(text.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_vector_stays_on_one_line() {
        // The whole reason for a hand-written writer. Three lines per position
        // makes a scene unreadable and its diffs uninformative.
        let text = to_text(&json!({ "position": [1.0, 2.5, -3.0] }));
        assert_eq!(text, "{\n  \"position\": [1, 2.5, -3]\n}\n");
    }

    #[test]
    fn a_list_of_objects_still_gets_a_line_each() {
        // Compact is for numbers. Objects on one line would put a whole scene
        // on one line, which is the opposite failure.
        let text = to_text(&json!({ "objects": [{ "id": "a" }, { "id": "b" }] }));
        assert!(
            text.contains("\n    {\n      \"id\": \"a\"\n    },\n"),
            "{text}"
        );
    }

    #[test]
    fn floats_are_rounded_rather_than_written_as_computed() {
        // What a drag actually produces.
        let text = to_text(&json!({ "x": 0.1 + 0.2 }));
        assert_eq!(text, "{\n  \"x\": 0.3\n}\n");
    }

    #[test]
    fn a_whole_number_is_written_without_a_point() {
        // `1.0` and `1` both parse, and the second is what somebody typing a
        // scale of one would write.
        assert_eq!(to_text(&json!([1.0, 0.0, -0.0])), "[1, 0, 0]\n");
    }

    #[test]
    fn empty_things_do_not_get_two_lines() {
        assert_eq!(
            to_text(&json!({ "a": {}, "b": [] })),
            "{\n  \"a\": {},\n  \"b\": []\n}\n"
        );
    }

    #[test]
    fn what_is_written_parses_back_to_what_it_meant() {
        // A pretty printer that cannot be read back is a bug that hides until
        // somebody reopens their work.
        //
        // Compared as text rather than as `Value`, and not to dodge the
        // assertion: `0.0` is deliberately written `0`, which parses back as
        // an integer, and `serde_json` counts `Number(0.0)` and `Number(0)` as
        // different values though they are the same number. Text is the thing
        // this function is actually promising about.
        let value = json!({
            "objects": [
                { "id": "a", "transform": { "position": [1.5, 0.0, -2.25] }, "on": true },
                { "id": "b", "tags": ["one", "two"], "parent": null }
            ]
        });
        let text = to_text(&value);
        let back: Value = serde_json::from_str(&text).expect("it parses");
        assert_eq!(to_text(&back), text);
        // And nothing was quietly dropped on the way.
        assert!(text.contains("\"on\": true"), "{text}");
        assert!(text.contains("\"parent\": null"), "{text}");
        assert!(
            text.contains("[\"one\", \"two\"]") || text.contains("\"one\","),
            "{text}"
        );
    }

    #[test]
    fn a_string_with_awkward_characters_survives() {
        let value = json!({ "name": "a \"quoted\" name\nwith a newline\tand a tab" });
        let back: Value = serde_json::from_str(&to_text(&value)).expect("it parses");
        assert_eq!(back, value);
    }

    #[test]
    fn writing_twice_gives_the_same_bytes() {
        // The guarantee that matters: a save that changes a file nobody
        // edited would put noise in every review.
        let value = json!({ "objects": [{ "id": "a", "position": [0.1, 0.2, 0.3] }] });
        let once = to_text(&value);
        let back: Value = serde_json::from_str(&once).unwrap();
        assert_eq!(to_text(&back), once);
    }
}
