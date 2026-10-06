# Backlog

What is planned and not built. Each item says what it is, what is still
open about it, and what it waits on. Items leave this file when they land.

## Research

Research's milestones after Lean (P0–P7, `6c103a9`).

### Waiting on the scientist

- **Lean, live (P8).** Make Lean from the Lab pane's Lean card (about 3 GB
  to download, about 15 GB kept). Then
  `PROTON_LEAN_RECORD=1 cargo test -p proton-core --test live_lean -- --ignored --nocapture`
  records `crates/proton-research/fixtures/lean/*.jsonl` again from a real
  Lean — the offline tests' expected statements may need adjusting to its
  real output — and a live mathematics round follows, with the scientist's
  go-ahead. Unverified until then: whether `lake exe cache get` builds its
  tool without Xcode, Mathlib's cache host, the real `lean --json` message
  shape, time and memory per check, and whether 30 turns cover three
  attempts.
- **A live round with the literature on.** `tests/live_round.rs` defaults to
  it; set `PROTON_MCP=target/release/proton-mcp` so Claude Code and Codex
  read the papers through Proton's server. It spends the scientist's limits.
- **Semantic Scholar's fixtures.** Its pool for callers without a key
  answered 429 throughout 2026-10-06, so `tests/fixtures/s2_*.json` were never
  recorded: `PROTON_LITERATURE_RECORD=1 cargo test -p proton-literature --test live semantic -- --ignored`
  once it answers.

### Later builds

- **Molecule and protein viewers.** Molecules from SMILES and XYZ — impostor
  spheres and cylinders, element colours and radii, bond detection, picking —
  building on the Lab's 3D scenes, which draw spheres and bonds already; and
  protein structures from PDB and mmCIF, with cartoons. Open: vendoring Mol*
  or NGL (a download, to be asked for with its size) or drawing them in
  three.js; viewers that need workers or wasm go in a sandboxed frame under
  the window's CSP.
- **Manuscripts.** The paper drafted inside the investigation, every figure
  and number traced to the run, code and environment that produced it, with
  inline annotation to refine it. Open: LaTeX or Typst, and where it compiles.
- **Forking an investigation.** A copy to take another direction from a
  point in it. Open: what is copied — hypotheses, tournament, runs, the
  literature and its PDFs — and how the copy says where it came from.
- **Remote compute.** The Lab's code run elsewhere. Open: SSH, Slurm, or
  both, and what provenance and sandboxing mean off the machine.
- **Lean's statements compared.** Lean's `comparator` (sandboxed on Linux)
  to check a proved statement against an agreed one, rather than leaving
  faithfulness to the prover's notes, the judges and the scientist.
- **The Lab on Linux and Windows.** bubblewrap has never been run for real,
  and the Lab is Unix-only (no `Scripts\python.exe`).
