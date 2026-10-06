# Lean's answers, for the round's tests

Each `NAME.lean` is a file a prover might submit for hypothesis H004, and
`NAME.jsonl` is what `lean --json` printed for it once Proton appended its
two questions (`#check @H004` and `#print axioms H004`) — one message a line.

These were written by hand from Lean's message format, before Proton had a
Lean of its own; the live test (`crates/proton-core/tests/live_lean.rs`)
records them again from a real Lean and Mathlib.
