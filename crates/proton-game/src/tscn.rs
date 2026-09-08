//! Reading and writing Godot's `.tscn`.
//!
//! ## The shape of the file
//!
//! A sequence of sections, each a bracketed header and the lines under it:
//!
//! ```text
//! [gd_scene load_steps=3 format=3 uid="uid://c0abc"]
//!
//! [ext_resource type="Texture2D" path="res://tiles.png" id="1_x8k"]
//!
//! [node name="Player" type="CharacterBody2D" parent="."]
//! position = Vector2(196, 286)
//! speed = 400.0
//! ```
//!
//! Only `[node ...]` sections are turned into anything; everything else —
//! resources, sub-resources, connections, editable instances — is text this
//! does not touch, which is most of a real scene and all of the parts most
//! likely to change between Godot versions.
//!
//! ## The hierarchy is in the `parent` attribute, as a path
//!
//! The root node has no `parent`. A child of the root has `parent="."`. Deeper
//! ones name a path from the root: `parent="Player/Arm"`. So a node's own path
//! is its parent's path joined with its name, and that is what makes the tree
//! — the same flat-list-with-a-parent-reference shape `proton-scene` chose on
//! purpose, arrived at here because Godot got there first.
//!
//! ## Values are kept as written
//!
//! `Vector2(196, 286)`, `ExtResource("1_x8k")`, `&"walk"`, `null` — a `.tscn`
//! value is a small expression language, and parsing it into a typed model
//! would mean re-emitting it later and getting the spacing wrong. What a
//! property has is the text between the `=` and the end of the line, and what
//! an edit does is replace those bytes.

use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A Godot scene, and the text it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    text: String,
}

/// One node in the scene tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameNode {
    /// As Godot writes it: `Player`, `CollisionShape2D`.
    pub name: String,
    /// The Godot class: `CharacterBody2D`, `Sprite2D`. Empty for a node that
    /// instances a scene rather than naming a type, which is ordinary.
    pub kind: String,
    /// The path from the root, `Player/Arm`. Empty for the root itself, which
    /// is what makes it the root.
    pub path: String,
    /// The `parent` attribute as written: absent for the root, `.` for a child
    /// of it.
    pub parent: String,
    pub depth: usize,
    pub children: usize,
    pub properties: Vec<Property>,
    /// 1-based, for opening the scene at this node in Codex.
    pub line: usize,
}

/// One `key = value` line under a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub name: String,
    /// Exactly the text after the `=`, trimmed of the space either side.
    pub value: String,
    pub line: usize,
    /// The bytes the value occupies. Stale the moment anything edits the file,
    /// so it is not sent anywhere and is re-scanned after every write.
    #[serde(skip)]
    #[ts(skip)]
    span: Range<usize>,
}

impl Scene {
    /// Read a scene, refusing anything that is not one.
    ///
    /// The first section of a `.tscn` is always `[gd_scene`. A `.tres` — a
    /// resource, same syntax — opens with `[gd_resource`, and showing its
    /// contents in a scene tree would be a confident wrong answer.
    pub fn parse(text: &str) -> Result<Scene, String> {
        let first = text
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with('['))
            .unwrap_or_default();
        if !first.starts_with("[gd_scene") {
            return Err(match first.starts_with("[gd_resource") {
                true => "this is a Godot resource, not a scene".into(),
                false => "this is not a Godot scene".into(),
            });
        }
        Ok(Scene {
            text: text.to_string(),
        })
    }

    pub fn read(path: &Path) -> Result<Scene, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Scene::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// The file as it should be written. Byte for byte what was read, apart
    /// from whatever was edited.
    pub fn to_text(&self) -> &str {
        &self.text
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, &self.text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The node tree, in the order the file lists it.
    ///
    /// Which is already parents-before-children: Godot writes a node after the
    /// parent it names, because it could not otherwise be loaded. Sorting
    /// would be work to arrive back where we started, and would hide a file
    /// where that is *not* true — which is a file Godot itself would refuse.
    pub fn nodes(&self) -> Vec<GameNode> {
        let mut out: Vec<GameNode> = Vec::new();
        for section in sections(&self.text) {
            if section.kind != "node" {
                continue;
            }
            let name = attribute(&section.header, "name").unwrap_or_default();
            let parent = attribute(&section.header, "parent").unwrap_or_default();
            let path = match parent.as_str() {
                // The root: no parent attribute at all.
                "" => String::new(),
                "." => name.clone(),
                other => format!("{other}/{name}"),
            };
            let depth = match parent.as_str() {
                "" => 0,
                "." => 1,
                other => other.split('/').count() + 1,
            };
            out.push(GameNode {
                properties: properties(&self.text, &section),
                line: self.text[..section.at].lines().count() + 1,
                children: 0,
                kind: attribute(&section.header, "type").unwrap_or_default(),
                depth,
                parent,
                path,
                name,
            });
        }
        // Counted afterwards, because a parent is written before the children
        // that name it and cannot know about them yet.
        for i in 0..out.len() {
            let mine = out[i].path.clone();
            out[i].children = out
                .iter()
                .filter(|one| match one.parent.as_str() {
                    "." => mine.is_empty(),
                    other => other == mine && !other.is_empty(),
                })
                .count();
        }
        out
    }

    /// Change one property on one node.
    ///
    /// Answers whether anything changed, so setting a value to what it already
    /// is does not write the file.
    pub fn set_property(&mut self, path: &str, name: &str, value: &str) -> Result<bool, String> {
        if value.contains('\n') {
            return Err("a property value is one line".into());
        }
        let node = self
            .nodes()
            .into_iter()
            .find(|one| one.path == path)
            .ok_or_else(|| format!("there is no node {path:?}"))?;

        if let Some(existing) = node.properties.iter().find(|one| one.name == name) {
            if existing.value == value {
                return Ok(false);
            }
            self.text.replace_range(existing.span.clone(), value);
            return Ok(true);
        }

        // Not there yet. Written under the node's last property, or directly
        // under the header when it has none — which is where Godot puts one,
        // and putting it anywhere else would move lines that did not change.
        //
        // Matched on name *and* parent, not name alone. Godot allows the same
        // name under different parents and it is the norm — every character
        // has a `Sprite` — so name alone finds whichever comes first in the
        // file and writes the property onto the wrong node, silently.
        let section = sections(&self.text)
            .into_iter()
            .find(|s| {
                s.kind == "node"
                    && attribute(&s.header, "name").as_deref() == Some(&node.name)
                    && attribute(&s.header, "parent").unwrap_or_default() == node.parent
            })
            .ok_or_else(|| format!("there is no node {path:?}"))?;
        let at = node
            .properties
            .last()
            .map(|one| one.span.end)
            .unwrap_or(section.header_end);
        self.text.insert_str(at, &format!("\n{name} = {value}"));
        Ok(true)
    }
}

/// One `[...]` section: its header, and where its body runs to.
struct Section {
    /// `node name="Player" type="CharacterBody2D" parent="."`
    header: String,
    /// The first word, which says what kind of section it is.
    kind: String,
    /// Where the `[` is.
    at: usize,
    /// Just past the `]`.
    header_end: usize,
    /// Where the next section starts, or the end of the file.
    body_end: usize,
}

fn sections(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for (at, _) in text.match_indices('[') {
        // Only a `[` that begins a line is a section header. One inside a
        // value — `points = PackedVector2Array([0, 0])` — is not, and treating
        // it as one would cut a node's properties in half.
        if at > 0 && text.as_bytes()[at - 1] != b'\n' {
            continue;
        }
        let Some(close) = text[at..].find(']').map(|end| at + end) else {
            continue;
        };
        let header = text[at + 1..close].to_string();
        let kind = header
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if let Some(last) = out.last_mut() {
            last.body_end = at;
        }
        out.push(Section {
            at,
            header_end: close + 1,
            body_end: text.len(),
            header,
            kind,
        });
    }
    out
}

/// One attribute out of a section header, by name.
fn attribute(header: &str, name: &str) -> Option<String> {
    let mut at = 0;
    while let Some(found) = header[at..].find(name) {
        let start = at + found;
        at = start + name.len();
        // A whole word: `parent` must not match inside `grandparent`.
        if start > 0 && !header.as_bytes()[start - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = header[at..].trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        let value = rest[1..].trim_start();
        return Some(match value.starts_with('"') {
            true => value[1..].split('"').next()?.to_string(),
            false => value.split_whitespace().next()?.to_string(),
        });
    }
    None
}

/// The `key = value` lines belonging to a section.
fn properties(text: &str, section: &Section) -> Vec<Property> {
    let body = &text[section.header_end..section.body_end];
    let mut out = Vec::new();
    let mut at = section.header_end;
    for line in body.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_end();
        // A continuation of a multi-line value — Godot wraps long arrays —
        // has no `=` at the top level and is left alone rather than guessed
        // at. Its first line is still found, which is what an inspector needs.
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let name = trimmed[..eq].trim();
        if name.is_empty() || !name.chars().all(is_key) {
            continue;
        }
        let raw = &trimmed[eq + 1..];
        let lead = raw.len() - raw.trim_start().len();
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        let from = start + eq + 1 + lead;
        out.push(Property {
            name: name.to_string(),
            value: value.to_string(),
            line: text[..start].lines().count() + 1,
            span: from..from + value.len(),
        });
    }
    out
}

/// What may appear in a property name. Godot allows paths — `shape/margin`,
/// `theme_override_colors/font_color` — and indices in `metadata/0`.
fn is_key(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '-')
}
