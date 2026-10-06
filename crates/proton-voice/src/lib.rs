//! Voices: speech made by Fish Audio, for Notebook's audio overviews.
//!
//! [`fish`] is the client — speech asked for and streamed to a file, the
//! voices listed and their samples fetched — over a [`Transport`] that is the
//! network in the app and a script in a test, so nothing here is ever asked
//! of Fish by a test. The key is a [`Key`], which cannot be printed: it goes
//! only into the header of a request to Fish, never into a log, an error or
//! the page.

pub mod fish;

pub use fish::{Fish, FishError, Key, Speech, Transport, Voice, VoiceQuery};
