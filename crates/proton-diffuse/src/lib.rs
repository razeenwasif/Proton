//! Diffuse mode: image generation by node graph, a small ComfyUI.
//!
//! A person builds a graph of nodes — load a checkpoint, encode a prompt,
//! sample, decode, save — and runs it. The models run on forge, the RTX 4090
//! desktop, behind Nexus's gateway; Proton holds the graph, draws it, checks
//! it and sends it whole. `docs/DIFFUSE.md` is the design and the contract
//! with Nexus.
//!
//! ## The catalogue is the server's
//!
//! What nodes there are, what they take and what they make comes from the
//! server's `GET /nodes`, not from this crate: a node Nexus adds appears in
//! Proton without a release. So nothing here knows what a KSampler is. A
//! flow is checked against whichever [`Catalogue`] it is given, and the v1
//! catalogue in the design is this crate's test fixture — the same JSON
//! Nexus's tests hold, so a change on one side breaks the other's.
//!
//! ## The pieces
//!
//! - [`catalogue`]: the node catalogue and the model lists, as the server
//!   describes them.
//! - [`flow`]: a `.flow.json` — nodes with their places and values, links
//!   between their ports — and the graph a run sends.
//! - [`op`]: every edit, as a value, applied to a flow whole or not at all.
//! - [`check`]: what stops a flow running, node by node.
//! - [`doc`]: a flow open for editing — undo, redo, held until saved.
//! - [`text`]: the file's text, written so diffs show only what changed.
//! - [`client`]: the gateway's `/img/v1` — requests, the run's event
//!   stream, and how the gateway and forge are.
//! - [`cache`]: what forge served last, kept so graphs edit while it is off.
//! - [`templates`]: starter graphs, as batches of ops.
//! - [`png`]: an output's size and the graph that made it, from its PNG's head.

pub mod cache;
pub mod catalogue;
pub mod check;
pub mod client;
pub mod doc;
pub mod flow;
pub mod op;
pub mod png;
pub mod templates;
pub mod text;

pub use cache::Known;
pub use catalogue::{Catalogue, Input, Kind, Model, Models, NodeType, Output};
pub use check::{check, Problem};
pub use client::{Client, Error, Event, Health, HealthState, Key};
pub use doc::FlowDoc;
pub use flow::{Flag, Flow, Graph, GraphNode, Link, Node, Port, SeedMode, FORMAT};
pub use op::{Applied, Op};
