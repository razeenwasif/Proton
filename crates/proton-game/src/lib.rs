//! Game mode's document: a Godot scene.
//!
//! ## Why somebody else's format, when every other mode has its own
//!
//! Design has `theme.css`, Modeling has `scene.proton.json`, Data has
//! `.ipynb`. Game's document is listed in the architecture as *engine-native
//! scene files*, and that is a decision rather than an omission: a game's
//! scene is only meaningful to the engine that runs it. A Proton format here
//! would be a second copy of the truth that the engine cannot open, and
//! keeping the two in step would be the whole job forever.
//!
//! So this reads and writes what Godot already writes. `.tscn` is a text
//! format that diffs in Git on its own — the load-bearing rule of this project
//! is already satisfied by somebody else's decision, and the useful thing to
//! do is not to break it.
//!
//! Godot rather than Unity or Unreal, and the reason is the same one: Unity's
//! scenes are YAML with generated file ids that churn, and Unreal's are
//! binary. `.tscn` is the one where reading and writing honestly is possible.
//!
//! ## Nothing is re-printed
//!
//! Every edit is a splice, as everywhere else in this project — and here it
//! matters more, because the file belongs to a tool that will open it next.
//! Re-emitting a `.tscn` from a model would mean guessing at Godot's own
//! formatting, and being wrong in a way that shows up as a whole-file diff in
//! somebody's game repository.
//!
//! ## What this is not
//!
//! It reads a scene. It does not run one. The play transport, the profiler,
//! hot reload and replaying recorded input all need a Godot process to talk
//! to, and none of them is faked here.

mod tscn;

pub use tscn::{GameNode, Property, Scene};
