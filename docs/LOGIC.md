# Logic — Proton's logic mode

A calculator and a solver for logic, in a tab of its own:
- the calculator after [logic-calculator.com](https://logic-calculator.com/):
  truth tables, properties, normal forms, Karnaugh maps, equivalence and
  arguments for any formula;
- the solver after John Slaney's
  [Logic for Fun](https://logic4fun.comp.anu.edu.au/): a puzzle written as
  sorts, functions and constraints in first-order logic, and every model of
  it found;
- proofs: semantic tableaux, natural deduction, sequent calculus and
  resolution, built by Proton or written by you and checked line by line.

An agent can read, solve and prove too. Asked for on 2026-10-30.

## Decided (2026-10-30)

- **The label is "Logic"**, on the rail and the title bar, on **⌃8**, after
  Circuit's ⌃7. In the code it is `logic`: `ModeId::Logic`, the crate
  `proton-logic`, the core module `logic`, `ui/logic*.js`, commands `Logic*`
  and events `logic*`.
- **Two languages, one meaning.** Proton's own (below) is what Proton writes
  and the agent is taught. Logic4Fun's is read and written too, since every
  construct of one has a construct of the other: a Logic4Fun puzzle opens as
  itself, and "Save as Logic4Fun" gives the text of its three boxes to paste
  into the site. The person chose both, after seeing the two side by side.
- **Nothing is taken from FINDER or the site's code.** The solver is
  Proton's own, written from what the language means, as the site's guide and
  syntax page describe it. The site's puzzles are its own work: none is
  shipped or copied into the tests; the tests use puzzles written here.
- **Documents are text files that diff,** Proton's rule: `.logic` for
  theories and worksheets, `.proof` for proofs, `.l4f` for Logic4Fun's text.
- **The engine is Rust, in the core,** a new crate with no new external
  crates. Circuit's Quine–McCluskey and synthesis are reused, not copied:
  `proton-logic` depends on `proton-circuit`.
- **Built next,** ahead of the Notebook and Research backlogs, at the
  person's word on 2026-10-30.

## Proton's language

One language for a calculator line, a puzzle and a proof's formulas. A file
is a list of statements, one to a line; `#` starts a comment.

```
# Four spies
spy   = American | Belgian | Cuban | Dane      # a sort, its elements named
place = Egypt | France | Greece | Haiti
level = retired < on_run < new < full          # named, and ordered

location: spy -> place  one-to-one             # a function, and its kind
status:   spy -> level  one-to-one

status(Cuban) > status(Belgian)                # a constraint
location(Dane) = France
all s: status(s) = retired <-> location(s) = Greece
status(American) = on_run
all s: location(s) = Haiti <-> status(s) = new
```

```
# Eight queens
file = 1..8
rank = 1..8
queen: file -> rank  one-to-one
all a, b: a != b -> abs(queen(a) - queen(b)) != abs(a - b)
```

- **Sorts:** `name = A | B | C` (elements named), `name = a < b < c`
  (named, ordered), `name = 1..8` (numbers), `name = size 5` (five elements,
  `name1` to `name5`), `name = size 3..6` (a size the solver may choose).
  `bool` and `int` are built in.
- **Vocabulary:** `f: s1, s2 -> s3` a function, `p: s1, s2 -> bool` a
  predicate, `c: s` a constant, `p, q, r: bool` propositions. After the
  type: `one-to-one`, `onto`, `partial` (some arguments have no value;
  `defined(f(x))` says which), `symmetric` (for two arguments, either order).
- **Formulas:**
  - connectives `not`, `and`, `or`, `xor`, `->`, `<->`, and `true`,
    `false`; each also as a symbol (`¬ ∧ ∨ ⊕ → ↔ ⊤ ⊥`) and as the
    calculators' ASCII (`! ~ & && | || ^ => <=> ⊃`), so anything pasted
    from a textbook or another calculator reads;
  - `=`, `!=`, `<`, `<=`, `>`, `>=`; `+`, `-`, `*`, `abs( )`, `min( )`,
    `max( )`, `succ( )`, `pred( )` on numbers and ordered sorts;
  - quantifiers `all x: F`, `some x: F`, the sort said when it cannot be
    inferred (`all x in spy: F`), several at once (`all a, b: F`), and
    counting ones: `exactly 2 x: F`, `at least 1 x: F`, `at most 3 x: F`;
  - `and` binds tighter than `or`, which binds tighter than `->`, then
    `<->`; `->` groups to the right. Nothing depends on a setting.
- **Questions, for the calculator:** a line beginning `?` is asked, not
  assumed. `? p -> q` (what is this formula), `? p, p -> q |= q` (does the
  argument hold), `? p -> q == !q -> !p` (are these the same), `? some …`
  (is there a model). A worksheet is a file of questions.
- **Logic4Fun's language** is read as the site's syntax page gives it:
  `Sorts:` / `Vocabulary:` / `Constraints:` sections, `enum:`,
  `cardinality`, `function { … }`, `{all_different}`, `ALL`, `SOME`, `DIF`,
  `EST`, `IMP`, clause form with `->` and `;`, `%` comments. Written back,
  the same: a `.l4f` file is the three boxes under their headings.

## The calculator

For the formula or question on the line at the cursor, or typed into the
calculator's box:
- **The truth table**, columns for each subformula if asked, up to 16
  propositions shown; past that, the questions below are answered by search
  instead of by table.
- **Properties:** tautology, contradiction or contingent; satisfiable, with a
  model; the number of models.
- **Equivalent forms:** negation normal form, CNF and DNF, the full (canonical)
  CNF and DNF, the least sum of products and product of sums (Circuit's
  Quine–McCluskey), and the formula in NAND only and NOR only.
- **A Karnaugh map** for two to six propositions, groups ringed, as Circuit
  draws them.
- **Equivalence** of two formulas, with a row where they differ if they do.
- **Arguments:** valid, or a countermodel; and a proof of it, in the system
  chosen (below).
- **A truth table to a formula:** a column of 0s and 1s, or a table typed in,
  read back as a formula.
- **Build in Circuit:** the formula drawn as a circuit in Circuit, AND/OR,
  NAND or NOR, its truth table its test.
- Formulas with quantifiers go to the solver and the provers instead.

## The solver

- **A theory's models found:** each sort given its elements, each function a
  table, every constraint true. Shown as tables — a function of one argument
  as a row, of two as a grid — the first model at once, and the next ones on
  asking; or "no model", or "no model within the time".
- **How:** each function's value at each argument is a cell; constraints are
  instantiated over the sorts, a cell at a time as the search reaches them;
  a cell with one value left is set; `one-to-one` is kept as a whole; the
  search takes the cell with fewest values left. Symmetry between elements
  nothing names is broken, so a model is not found twice in another order.
  Counting is exact; enumeration can be stopped.
- **When there is no model:** the smallest set of constraints that already
  has none is found and those lines are marked, as the site's Diagnose does.
- **Sizes it is measured on:** eight queens (all 92 models), a five-house
  Einstein puzzle, and a 9×9 Sudoku, each in under a second.

## Proofs

A `.proof` file names its system, its premises and its goal, then a line per
step: a number, a formula (or sequent, or clause), the rule and the lines it
uses. Proton checks each line and says why one is wrong; and it can find a
proof, which it writes in the same form.

```
system fitch
premises p -> q, q -> r
goal p -> r

1  p -> q          premise
2  q -> r          premise
3  | p             assume
4  | q             ->E 1, 3
5  | r             ->E 2, 4
6  p -> r          ->I 3-5
```

- **Semantic tableaux:** propositional, and first-order with free variables
  and unification. Built by Proton and drawn as a tree, each branch closed or
  open; an open branch read as a countermodel.
- **Natural deduction, Fitch style:** subproofs as bars, introduction and
  elimination for each connective, the quantifiers with their conditions on
  new names, and `=`. Checked line by line; found by search — always for
  propositional logic, within a bound for first-order.
- **Sequent calculus:** Gentzen's LK, its rules as named; a proof a tree,
  written as numbered lines; checked, and found by search.
- **Resolution:** the formula to clauses with each step shown — negation
  normal form, prenex, Skolem functions, CNF — then resolvents, with
  unification for first-order, to the empty clause.
- **When no proof is found,** a countermodel is looked for with the solver,
  so "not valid" comes with a reason where one is small enough to find.

## The window

- **The home page:** the project's `.logic`, `.proof` and `.l4f` files, and
  New theory, New worksheet and New proof.
- **An editor and a panel.** The editor is a text area coloured as it is
  typed, as Notebook's coding practice has. The panel on the right shows,
  for the line at the cursor: the calculator's results, the solver's models,
  or a proof's check, each line marked.
- **Keys:**
  - `⌘↩` asks the line at the cursor; `⌘⇧↩` solves the theory or checks the
    proof.
  - `⌘S` saves; `⎋` stops a search.
  - The calculator's symbol buttons type `¬ ∧ ∨ → ↔ ⊨`; typed ASCII is read
    as the same.
- **Edits are held until ⌘S,** and the quit prompt names an unsaved file.

## The agent

- **Tools:**
  - `logic_ask` answers a calculator question: a table, properties, forms,
    validity.
  - `logic_solve` gives a theory's models, or the lines that conflict.
  - `logic_prove` finds or checks a proof in a named system.
  - `logic_read` and `logic_write` read and change a file, its changes held
    until saved.
- **`@logic`** sends the file on screen, the line at the cursor and the last
  result.
- **With Notebook:** "Logic practice", made from the sources and checked by
  the engine before it is kept — formulas to classify, arguments to prove,
  puzzles to encode — each opened in Logic.

## Not built, and why

These are the backlog's "Later builds" under Logic: other proof systems and
other logics, for a complete logic workbench.
- Hilbert-style axiomatic proofs; tree-shaped natural deduction.
- Step-by-step Boolean simplification, each law named.
- Herbrand models; equality by paramodulation.
- DPLL and CDCL with the search shown, for large propositional problems.
- Modal logics (K, T, S4, S5) with tableaux and Kripke countermodels;
  intuitionistic logic, with Kripke countermodels; three-valued and fuzzy
  logics; relevant logics.
- LTL and CTL model checking.
- Syllogisms with Venn diagrams.
- English to logic and back, by the agent.
- A statement handed to Lean in Research.

## Milestones

Each lands with its tests passing.

**Built so far** (2026-10-30): L0 — `crates/proton-logic`: Proton's language
read statement by statement (a statement runs on past a line ending in an
operator or with a bracket open), each mistake named at its line and column;
printed back with only the brackets its meaning needs, which 5,000 random
formulas read back unchanged; typed, each variable's sort found from what it
is given to and undeclared names in a formula's place taken as propositions;
Logic4Fun's three sections read — `enum:` as ordered sorts, `cardinality = n`
as `0..n-1`, clause form with its free variables meant for every value,
`(A) = (B)` between formulas as `A <-> B`, `DIF` as `abs(a - b)`, `EST` as
`defined`, FINDER's settings left out and named — and written back with every
connective bracketed, counting quantifiers spelt out in `SOME`, and a range
not starting at 0 written from 0 with the numbers said of it moved too
(refused where that would change the meaning); and formulas worked out in a
model, partial functions' missing values included, a model's tables checked
against their kinds (one-to-one, onto, symmetric, total). Checked on a
puzzle written here in both languages, and on eight queens. Logic4Fun's text
is checked by reading it back, not by the site's solver.

| | What | Proved by |
|---|---|---|
| **L0** | `proton-logic`: the language — parsed, typed, printed back the same; Logic4Fun's read and written; formulas evaluated | round trips; each construct against its meaning; Logic4Fun and Proton texts of one puzzle reading as one theory |
| **L1** | The calculator's engine: truth tables, properties, forms, equivalence, arguments with countermodels, table to formula; a SAT search past 16 propositions | each against hand-worked cases; forms checked equivalent to their formula by table |
| **L2** | The mode: `ModeId::Logic` on ⌃8, its home page, the editor, the calculator panel, Karnaugh maps, Build in Circuit, ⌘S and the quit prompt | core tests of the events; the browser suite |
| **L3** | The solver: sorts, functions, predicates, constants, one-to-one, numbers, counting quantifiers; models shown, counted and enumerated | puzzles written here with known answers; eight queens' 92 |
| **L4** | The rest of Logic4Fun's language (partial functions, `EST`, cardinality ranges, clause form); "no model" diagnosed; measured | the same puzzle in both languages, the same models; timing budgets |
| **L5** | Semantic tableaux, propositional and first-order, drawn; countermodels from open branches | valid and invalid arguments with known answers |
| **L6** | Natural deduction (Fitch): `.proof` files checked line by line, and found | proofs written here, right and wrong, each error named |
| **L7** | Sequent calculus and resolution: checked and found; clause form shown step by step | the same arguments as L5, by each system |
| **L8** | The agent's tools, `@logic`, Notebook's logic practice | through `run_tool`, as the other tools are tested |
| **L9** | Measured; docs; the status page | timing budgets |

## Open

- **Which rules Fitch uses.** Courses name the rules differently (`->E`,
  `MP`, `⊃E`; `RAA` or `¬I`). Proton will read the common names for each and
  write `forall x`'s (the open textbook) unless the person's course uses
  others — the person to say which, before L6.
