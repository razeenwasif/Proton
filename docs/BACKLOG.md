# Backlog

What is planned and not built. Each item says what it is, what is still
open about it, and what it waits on. Items leave this file when they land.

## Notebook

Gemini Notebook in Proton (S0–S10 and the S3 extras, on master). What is
left of the plan, and what came of building it.

### Waiting on the person

- **The live runs (S11).** Each of these spends a little of a plan or of Fish
  credit, so each is run by hand: `tutor_live` (all four engines answering
  from the sources through `proton-mcp --notebook`), `cards_live`,
  `quiz_live` (a quiz, a mark by another family, an exam, a past paper,
  objectives, a diagnostic, a learning overview, slides, an infographic,
  and coding problems checked by building and running them), `audio_live` (a brief spoken by Fish, with
  `FISH_API_KEY` and `PROTON_FISH_VOICES`; and a recording transcribed),
  `sources_live` (pages of the web, Discover and Deep Research, a guide,
  labels, and a slide read from its picture), and `study_live` over the
  real folders. Gemini needs its sign-in first.
- **The pilot.** A course and its exam to build a notebook for end to end:
  its past papers, a practice exam sat against the clock, cards reviewed
  over days.
- **Rendering the window's HTML natively (S9).** PDFs of an exam, a report
  or a deck, and Video Overviews — slides drawn as pictures, each with its
  narration, joined by ffmpeg — wait on a hidden webview's
  `createPDF`/`takeSnapshot` (through objc2), which can only be tried in a
  build of the app run alongside the person's own. The alternative is a
  managed Typst. Until then an exam prints from its HTML, and a deck goes
  out as a PowerPoint.

### Later builds

- **Joining an audio overview.** Asking the hosts a question as they talk —
  Gemini Notebook's interactive mode: speech in, a grounded answer, spoken
  in a host's voice, then back to the script. Open: Fish's streaming, and
  hearing the question.
- **Transcribing on the machine.** whisper.cpp and a model (from about
  150 MB), installed with two say-sos as the Lab's Python is, for
  recordings that are not to leave it; Fish transcribes today, paid.
- **Digests of sources too long to read whole.** A book of a thousand pages
  is read through the notebook's server today; a digest per section, made
  once with an estimate shown first, would let a call see all of it.
- **A PowerPoint's pictures.** Slides read from a .pptx are its words alone;
  reading its pictures needs its slides drawn, which needs LibreOffice or
  Keynote.
- **The web through every engine.** Discover and Deep Research run on Claude
  Code, whose web tools can be given alone; Codex's and Gemini's web search
  are untried unattended.

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
