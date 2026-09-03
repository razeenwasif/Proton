//! Grammars a plugin brought, as wasm.
//!
//! A tree-sitter grammar is C, and shipping one normally means shipping a
//! native build per platform. Compiled to wasm it is one file that works
//! everywhere, which is the whole reason plugin grammars are worth having:
//! installing a language should not mean having a C toolchain.
//!
//! **This is not the plugin sandbox.** A grammar is not a component and does
//! not speak the component ABI — tree-sitter loads and runs it through its
//! own store, and the engine here is separate from `proton-plugin`'s for a
//! reason worth stating plainly: that one has epoch interruption on and every
//! store carries a deadline, and tree-sitter creates its stores internally
//! where no deadline can be set. Sharing the engine would mean the first tick
//! traps a parse. So a grammar's external scanner is *not* on a clock, and
//! `docs/ARCHITECTURE.md` says so where the guarantee is described. The
//! deadline covers plugin commands, which is what it was built for.

use std::collections::HashMap;

use tree_sitter::wasmtime::Engine;
use tree_sitter::{Language, WasmStore};

/// A grammar loaded from a plugin: its bytes and its query.
///
/// The bytes are kept rather than the `Language`, because a wasm language
/// belongs to the store that built it and a store belongs to one parser. So
/// every `Syntax` that wants this grammar loads it into a store of its own,
/// and this is what it loads from.
#[derive(Debug, Clone)]
pub struct Added {
    pub id: String,
    pub extensions: Vec<String>,
    pub wasm: Vec<u8>,
    pub query: String,
}

/// Every grammar the plugins brought.
#[derive(Default)]
pub struct Grammars {
    engine: Option<Engine>,
    added: Vec<Added>,
}

impl std::fmt::Debug for Grammars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grammars")
            .field("added", &self.added.len())
            .finish()
    }
}

impl Grammars {
    /// Take a grammar, checking it loads before saying it did.
    ///
    /// Loaded once here and then thrown away, so a grammar that is not a
    /// grammar is reported when the plugin is installed rather than the first
    /// time somebody opens a file — which would be a file that mysteriously
    /// has no colours.
    pub fn add(&mut self, added: Added) -> Result<(), String> {
        let engine = self.engine.get_or_insert_with(Engine::default);
        let mut store = WasmStore::new(engine).map_err(|err| format!("{}: {err}", added.id))?;
        let language = store
            .load_language(&added.id, &added.wasm)
            .map_err(|err| format!("{}: {err}", added.id))?;

        // The query too. A grammar with a query that will not compile colours
        // nothing, which looks exactly like a grammar that did not load.
        tree_sitter::Query::new(&language, &added.query)
            .map_err(|err| format!("{}: highlights.scm: {err}", added.id))?;

        self.added.retain(|g| g.id != added.id);
        self.added.push(added);
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
    }

    /// The grammar for an extension, if a plugin brought one.
    pub fn for_extension(&self, extension: &str) -> Option<&Added> {
        self.added
            .iter()
            .find(|g| g.extensions.iter().any(|e| e == extension))
    }

    /// A parser holding this grammar, and the language to query with.
    ///
    /// A fresh store per parser, because that is what tree-sitter's ownership
    /// requires: `set_wasm_store` moves one in, and a language built by one
    /// store cannot be parsed by a parser holding another.
    pub fn parser(&self, grammar: &Added) -> Result<(tree_sitter::Parser, Language), String> {
        let engine = self.engine.clone().unwrap_or_default();
        let mut store = WasmStore::new(&engine).map_err(|err| format!("{}: {err}", grammar.id))?;
        let language = store
            .load_language(&grammar.id, &grammar.wasm)
            .map_err(|err| format!("{}: {err}", grammar.id))?;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_wasm_store(store)
            .map_err(|err| format!("{}: {err}", grammar.id))?;
        parser
            .set_language(&language)
            .map_err(|err| format!("{}: {err}", grammar.id))?;
        Ok((parser, language))
    }

    /// What is loaded, by language id.
    pub fn languages(&self) -> HashMap<&str, &[String]> {
        self.added
            .iter()
            .map(|g| (g.id.as_str(), g.extensions.as_slice()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_loaded_until_a_grammar_is_added() {
        // And no engine is built either: most people install no grammars, and
        // an editor that starts a second wasm engine to hold none of them is
        // paying for an extension point nobody used.
        let grammars = Grammars::default();
        assert!(grammars.is_empty());
        assert!(grammars.for_extension("zig").is_none());
    }

    #[test]
    fn something_that_is_not_a_grammar_is_refused_when_it_is_added() {
        // Rather than when a file is opened, which would look like a file
        // that mysteriously has no colours.
        let mut grammars = Grammars::default();
        let why = grammars
            .add(Added {
                id: "nonsense".into(),
                extensions: vec!["non".into()],
                wasm: b"this is not wasm".to_vec(),
                query: String::new(),
            })
            .expect_err("it should refuse");
        assert!(why.starts_with("nonsense:"), "and name it: {why}");
        assert!(grammars.is_empty());
    }
}
