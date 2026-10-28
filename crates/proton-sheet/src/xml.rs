//! XML for `.xlsx`, through quick-xml: a stream of starts, ends and text —
//! entities resolved, names without their prefixes — for a sheet's cells, and
//! a small tree for the parts that are small.

use quick_xml::events::Event;

#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    /// An element opened: its name without a prefix, and its attributes the
    /// same. An empty element is a start and then an end.
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

/// The events of a document, or why it could not be read.
pub fn events(xml: &str, mut f: impl FnMut(Ev) -> Result<(), String>) -> Result<(), String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let local = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    loop {
        let ev = reader.read_event().map_err(|e| format!("at {}: {e}", reader.buffer_position()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = local(e.local_name().as_ref());
                let mut attrs = Vec::new();
                for a in e.attributes().flatten() {
                    let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?.into_owned();
                    attrs.push((local(a.key.local_name().as_ref()), value));
                }
                let empty = matches!(ev, Event::Empty(_));
                f(Ev::Start(name.clone(), attrs))?;
                if empty {
                    f(Ev::End(name))?;
                }
            }
            Event::End(e) => f(Ev::End(local(e.local_name().as_ref())))?,
            Event::Text(t) => f(Ev::Text(t.decode().map_err(|e| e.to_string())?.into_owned()))?,
            Event::CData(c) => f(Ev::Text(String::from_utf8_lossy(&c).into_owned()))?,
            Event::GeneralRef(r) => {
                let s = if r.is_char_ref() {
                    r.resolve_char_ref().ok().flatten().map(String::from).unwrap_or_default()
                } else {
                    match r.decode().map_err(|e| e.to_string())?.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    }
                    .to_string()
                };
                f(Ev::Text(s))?;
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

/// An element and everything in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// The text directly inside it.
    pub text: String,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// A document as a tree: its root element.
pub fn tree(xml: &str) -> Result<Node, String> {
    let mut stack: Vec<Node> = vec![Node::default()];
    events(xml, |ev| {
        match ev {
            Ev::Start(name, attrs) => stack.push(Node { name, attrs, ..Default::default() }),
            Ev::End(_) => {
                let done = stack.pop().ok_or("an end with no start")?;
                stack.last_mut().ok_or("an end with no start")?.children.push(done);
            }
            Ev::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t);
                }
            }
        }
        Ok(())
    })?;
    let root = stack.pop().ok_or("no document")?;
    root.children.into_iter().next().ok_or_else(|| "an empty document".into())
}

/// Text for an element or an attribute, escaped; the control characters
/// XML cannot hold written as Excel writes them, `_x000B_`.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' | '\t' => out.push(c),
            '\r' => out.push_str("&#13;"),
            c if (c as u32) < 0x20 => out.push_str(&format!("_x{:04X}_", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `_x000B_` read back as the character it stands for.
pub fn unesc_x(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("_x") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let code = tail.get(2..6).filter(|h| h.chars().all(|c| c.is_ascii_hexdigit())).filter(|_| tail.get(6..7) == Some("_"));
        match code.and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
            Some(c) => {
                out.push(c);
                rest = &tail[7..];
            }
            None => {
                out.push_str("_x");
                rest = &tail[2..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_has_its_text_and_attributes_unescaped() {
        let t = tree(r#"<?xml version="1.0"?><x:root xmlns:x="u" a="1 &amp; 2"><b>fish &amp; chips &#233;</b><c/><b><![CDATA[<raw>]]></b></x:root>"#).unwrap();
        assert_eq!(t.name, "root");
        assert_eq!(t.attr("a"), Some("1 & 2"));
        assert_eq!(t.all("b").map(|b| b.text.clone()).collect::<Vec<_>>(), vec!["fish & chips é", "<raw>"]);
        assert!(t.child("c").is_some());
        assert_eq!(esc("a\u{b}b<"), "a_x000B_b&lt;");
        assert_eq!(unesc_x("a_x000B_b"), "a\u{b}b");
        assert_eq!(unesc_x("_x000B_ and _xZZ_"), "\u{b} and _xZZ_");
    }
}
