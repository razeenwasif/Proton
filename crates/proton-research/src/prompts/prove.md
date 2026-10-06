# Your task: state a hypothesis in Lean, and prove it — or its negation

{{context}}

## The hypothesis

{{hypothesis}}

## What its verification found

{{verification}}

{{previous}}

State the hypothesis as a theorem of Lean 4 with Mathlib — Lean {{lean}}, Mathlib {{mathlib}} — and prove it. If it is false, state and prove its negation instead. Lean checks your file exactly as you give it, with nothing added but two questions at its end: what `{{theorem}}` is, and which axioms it rests on.

- Name the theorem `{{theorem}}`, at the top level of the file, outside every namespace: `theorem {{theorem}} … := by …`. Lemmas of your own may come before it.
- Import Mathlib — `import Mathlib`, or parts of it — and nothing else.
- State it faithfully. The statement Lean prints is what is shown as proved, beside the hypothesis: a weaker or different one proves nothing about it. Say in `notes` exactly how your statement is the hypothesis's, and what it leaves out.
- Use Mathlib's definitions and lemmas, by their real names{{sources}}. You cannot run Lean yourself: what it says of your file comes back to you, and you may be asked to mend it — {{calls}} in all.
- Use `sorry` only for a step you cannot finish: a statement whose proof has `sorry` in it is kept as stated, with the proof open.
- Proton refuses a file that could run code or take anything on trust: `#eval`, `axiom`, `native_decide`, new syntax, macros or elaborators, `set_option debug.…`, and anything from Lean's own namespaces or IO. A proof rests on `propext`, `Classical.choice` and `Quot.sound` alone.
- If the hypothesis has no faithful statement in Lean — it is about the world rather than mathematics, or too vague to state — answer with the kind `none`, an empty `lean`, and why in `notes`.

Answer with its `label`, the `kind` (`proof` of the hypothesis, `disproof` for a proof of its negation, or `none`), the whole `lean` file, and your `notes`.
