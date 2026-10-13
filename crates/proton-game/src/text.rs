//! A game's files as text a diff can be read in.
//!
//! Modeling's writer (`proton-scene`'s `write.rs`) makes the case for writing
//! JSON by hand: a position is two numbers and should be one line, which no
//! streaming formatter can decide before it has started writing the array.
//! This is the same writer with one more rule — what is empty is left out —
//! because a room has a hundred entities and most of them have no `to`, no
//! `requires` and no `props`, and a file of `"to": null` is a file nobody
//! reads.
//!
//! ## Keys in the order the structs declare them
//!
//! `serde_json`'s `Value` sorts an object's keys — its `preserve_order` is off
//! in this workspace, and turning it on would reorder every other mode's
//! files — so a room written through one would start `ambience`, `camera`,
//! with its `id` somewhere in the middle. Serialising writes fields in the
//! order they are declared, though, so the struct is written as compact JSON
//! first and read back into [`Ordered`], which keeps the order it is given.

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Serialize;
use serde_json::Number;

/// Decimal places a number keeps: finer than any pixel, and stable — a drag
/// that leaves `0.30000000000000004` behind writes `0.3`.
const PLACES: usize = 6;

/// JSON as it was written: objects keep their keys in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Ordered {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Ordered>),
    Object(Vec<(String, Ordered)>),
}

impl Ordered {
    fn is_number(&self) -> bool {
        matches!(self, Ordered::Number(_))
    }

    fn is_string(&self) -> bool {
        matches!(self, Ordered::String(_))
    }

    fn is_boolean(&self) -> bool {
        matches!(self, Ordered::Bool(_))
    }
}

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Ordered, D::Error> {
        struct Reader;
        impl<'de> Visitor<'de> for Reader {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<Ordered, E> {
                Ok(Ordered::Null)
            }
            fn visit_none<E>(self) -> Result<Ordered, E> {
                Ok(Ordered::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Ordered, E> {
                Ok(Ordered::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Ordered, E> {
                Ok(Ordered::Number(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Ordered, E> {
                Ok(Ordered::Number(n.into()))
            }
            fn visit_f64<E>(self, n: f64) -> Result<Ordered, E> {
                Ok(Number::from_f64(n).map(Ordered::Number).unwrap_or(Ordered::Null))
            }
            fn visit_str<E>(self, s: &str) -> Result<Ordered, E> {
                Ok(Ordered::String(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<Ordered, E> {
                Ok(Ordered::String(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Ordered::Array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut members = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    members.push((key, value));
                }
                Ok(Ordered::Object(members))
            }
        }
        deserializer.deserialize_any(Reader)
    }
}

/// A value as the text of a file, ending in a newline.
pub fn to_text<T: Serialize>(value: &T) -> Result<String, String> {
    let compact = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let tree: Ordered = serde_json::from_str(&compact).map_err(|e| e.to_string())?;
    let mut out = String::new();
    write(&mut out, &tree, 0);
    out.push('\n');
    Ok(out)
}

/// What the writer leaves out of an object: nothing said is the same as the
/// default, which is empty.
fn empty(value: &Ordered) -> bool {
    match value {
        Ordered::Null => true,
        Ordered::Array(items) => items.is_empty(),
        // A camera with no bounds and no zones is no camera worth writing.
        Ordered::Object(map) => map.iter().all(|(_, v)| empty(v)),
        _ => false,
    }
}

fn write(out: &mut String, value: &Ordered, depth: usize) {
    match value {
        Ordered::Object(map) => {
            let kept: Vec<&(String, Ordered)> = map.iter().filter(|(_, v)| !empty(v)).collect();
            if kept.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (key, child)) in kept.iter().map(|m| (&m.0, &m.1)).enumerate() {
                pad(out, depth + 1);
                out.push_str(&string(key));
                out.push_str(": ");
                write(out, child, depth + 1);
                if i + 1 < kept.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, depth);
            out.push('}');
        }
        Ordered::Array(items) if items.is_empty() => out.push_str("[]"),
        // A point, a size, a rectangle, a list of keys: one thing, one line.
        Ordered::Array(items) if items.iter().all(|v| v.is_number() || v.is_string() || v.is_boolean()) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(out, item, depth);
            }
            out.push(']');
        }
        Ordered::Array(items) => {
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
        Ordered::String(text) => out.push_str(&string(text)),
        Ordered::Number(n) => out.push_str(&number(n)),
        Ordered::Bool(true) => out.push_str("true"),
        Ordered::Bool(false) => out.push_str("false"),
        Ordered::Null => out.push_str("null"),
    }
}

fn pad(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn number(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let Some(f) = n.as_f64() else {
        return n.to_string();
    };
    if !f.is_finite() {
        return "0".into();
    }
    let mut text = format!("{f:.PLACES$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    match text == "-0" {
        true => "0".into(),
        false => text,
    }
}

fn string(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_on_one_line_objects_open_and_empties_left_out() {
        #[derive(Serialize)]
        struct Thing {
            id: &'static str,
            at: [i32; 2],
            to: Option<i32>,
            requires: Vec<i32>,
            props: std::collections::BTreeMap<String, i32>,
            points: Vec<[i32; 2]>,
            scale: f64,
            rot: f64,
        }
        let value = Thing {
            id: "a",
            at: [10, -20],
            to: None,
            requires: vec![],
            props: Default::default(),
            points: vec![[0, 1], [2, 3]],
            scale: 1.0,
            rot: 0.1 + 0.2,
        };
        assert_eq!(
            to_text(&value).unwrap(),
            "{\n  \"id\": \"a\",\n  \"at\": [10, -20],\n  \"points\": [\n    [0, 1],\n    [2, 3]\n  ],\n  \"scale\": 1,\n  \"rot\": 0.3\n}\n"
        );
    }
}
