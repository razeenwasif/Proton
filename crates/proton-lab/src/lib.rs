//! Research's Lab: the Python an investigation's code runs in, made with
//! the scientist's say-so, and — next — the runs themselves.
//!
//! This crate starts processes and talks to no model: conda to make the
//! environment, Python to run in it. The core decides when; the scientist
//! decides whether.

pub mod conda;
pub mod env;
pub mod job;
pub mod run;
pub mod sandbox;

pub use env::{EnvState, Lab};
pub use run::{Run, Spec, State as RunState};
pub use sandbox::Kind as Sandbox;
