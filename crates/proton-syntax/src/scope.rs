//! Twelve colours, from a few hundred capture names.
//!
//! Grammars name captures at whatever granularity their authors found useful:
//! `function.method.builtin`, `variable.parameter`, `punctuation.delimiter`.
//! A reader looking at code can tell about a dozen colours apart, so this
//! collapses all of it onto the set the theme actually defines. Anything
//! unrecognised is left uncoloured rather than given a thirteenth colour that
//! means "we did not know".

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Constant,
    Variable,
    Property,
    Operator,
    Punctuation,
    Attribute,
}

impl Scope {
    /// Map a capture name onto a colour.
    ///
    /// Matched on the leading segment, because the tail is a refinement we do
    /// not draw: `function.method` and `function.macro` are both a function as
    /// far as the eye is concerned.
    pub fn from_capture(name: &str) -> Option<Scope> {
        // One exception to matching on the head, and it earns its place.
        // Grammars disagree about literals: some capture `1` as `@number`,
        // and tree-sitter-rust captures every numeric literal as
        // `@constant.builtin`. Folding that into `Constant` would leave the
        // number colour unused in the language Proton is written in. Booleans
        // and `None` come along with it, which is what themes do anyway.
        if name.starts_with("constant.builtin") {
            return Some(Scope::Number);
        }

        let head = name.split('.').next().unwrap_or(name);
        Some(match head {
            "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass" => {
                Scope::Keyword
            }
            "function" | "method" | "constructor" => Scope::Function,
            "type" | "class" | "struct" | "interface" | "namespace" | "module" | "tag" => {
                Scope::Type
            }
            "string" | "character" | "char" | "escape" | "regex" => Scope::String,
            "number" | "float" | "integer" | "boolean" => Scope::Number,
            "comment" => Scope::Comment,
            "constant" => Scope::Constant,
            "property" | "field" => Scope::Property,
            "operator" => Scope::Operator,
            "punctuation" | "delimiter" | "bracket" => Scope::Punctuation,
            "attribute" | "annotation" | "decorator" | "label" => Scope::Attribute,
            "variable" | "parameter" => Scope::Variable,
            // `@spell`, `@none`, `@text`, `@embedded` and whatever a grammar
            // invents next. Uncoloured is the right answer for all of them.
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinements_collapse_onto_the_colour_they_refine() {
        assert_eq!(
            Scope::from_capture("function.method.builtin"),
            Some(Scope::Function)
        );
        assert_eq!(
            Scope::from_capture("variable.parameter"),
            Some(Scope::Variable)
        );
        assert_eq!(
            Scope::from_capture("punctuation.bracket"),
            Some(Scope::Punctuation)
        );
    }

    #[test]
    fn an_unknown_capture_is_left_alone() {
        // Not an error, and not a fallback colour: a grammar naming something
        // we have no colour for should render as ordinary text.
        assert_eq!(Scope::from_capture("spell"), None);
        assert_eq!(Scope::from_capture("something.new"), None);
    }
}
