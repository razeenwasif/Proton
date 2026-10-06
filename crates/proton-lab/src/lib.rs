//! Research's Lab: the Python an investigation's code runs in, made with
//! the scientist's say-so, and the runs themselves; and Lean, with Mathlib,
//! for its mathematics.
//!
//! This crate starts processes and talks to no model: conda to make the
//! environment, Python to run in it. The core decides when; the scientist
//! decides whether.

pub mod conda;
pub mod env;
pub mod job;
pub mod lean;
pub mod pip;
pub mod run;
pub mod sandbox;

pub use env::{EnvState, Lab, Maker};
pub use lean::{LeanEnv, LeanPlan, LeanState};
pub use run::{Run, Spec, State as RunState};
pub use sandbox::Kind as Sandbox;
