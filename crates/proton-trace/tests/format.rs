//! Against real traces.
//!
//! Every fixture in `tests/traces/` came out of the actual Code Visualizer
//! running on this machine — not hand-written, because a hand-written fixture
//! only tests what I already believed the format was. The linked list carries
//! 287 pointers and is the one that matters: it is a cyclic structure, which
//! is the case the whole edge rule exists for.

use proton_trace::format::{Event, Status, Value};
use proton_trace::{edges, EdgeKind, Graph, Resolved, Trace};

fn load(name: &str) -> Trace {
    let path = format!("tests/traces/{name}.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn every_real_trace_parses() {
    // The contract check. If upstream changes the format, this is what says so
    // — and it says so against real output rather than against my reading of
    // the document.
    for name in [
        "dangling",
        "linked_list",
        "aliasing",
        "crash",
        "compile_error",
    ] {
        let trace = load(name);
        assert!(!trace.language.is_empty(), "{name} has no language");
    }
}

#[test]
fn a_clean_run_carries_steps_a_stack_and_a_heap() {
    let trace = load("linked_list");
    assert_eq!(trace.status, Status::Ok);
    assert_eq!(trace.steps.len(), 63);

    let step = &trace.steps[30];
    assert!(!step.stack.is_empty(), "a step should have a stack");
    assert!(!step.heap.is_empty(), "this program allocates");

    // Innermost first — the format is explicit about it, and a renderer that
    // assumes the other order draws the call stack upside down.
    let depths: Vec<u32> = step.stack.iter().map(|f| f.depth).collect();
    let mut sorted = depths.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(depths, sorted, "innermost frame should come first");
}

#[test]
fn a_compile_error_has_no_steps_but_says_why() {
    // The one status that cannot carry steps. The C backend leaves
    // `diagnostics` empty and puts the compiler's raw output in `error`, so
    // that is the only text there is to show.
    let trace = load("compile_error");
    assert_eq!(trace.status, Status::CompileError);
    assert!(trace.steps.is_empty());
    assert!(!trace.status.has_steps());

    let error = trace.error.expect("a compile error explains itself");
    assert!(error.contains("error:"), "{error}");
}

#[test]
fn a_crash_still_carries_everything_up_to_it() {
    // The property the pane depends on: a failed run is not an empty one.
    // Rendering nothing because `status != ok` throws away the steps that show
    // how it got there.
    let trace = load("crash");
    assert_eq!(trace.status, Status::RuntimeError);
    assert!(trace.status.has_steps());
    assert!(!trace.steps.is_empty(), "a crash keeps its steps");

    let last = trace.steps.last().unwrap();
    assert_eq!(last.event, Event::Signal, "the last step marks the crash");
    assert!(trace
        .error
        .as_deref()
        .is_some_and(|e| e.contains("SIGSEGV")));
}

/* ── the edge rule ────────────────────────────────────────────────── */

#[test]
fn a_pointer_resolves_to_the_object_it_names() {
    let trace = load("linked_list");
    let step = &trace.steps[30];
    let graph = Graph::of(step);

    assert!(!graph.is_empty(), "the step should index something");

    // Every heap block is reachable by its own address.
    for block in &step.heap {
        assert!(
            matches!(graph.resolve(&block.addr), Resolved::At { .. }),
            "a heap block should resolve by its own address: {}",
            block.addr
        );
    }
}

#[test]
fn a_null_pointer_is_null_and_not_a_failure() {
    // Most lists end in one. Reporting it as unresolvable would put a broken
    // arrow at the end of every correct linked list.
    let trace = load("linked_list");
    let graph = Graph::of(&trace.steps[30]);
    assert_eq!(graph.resolve("0x0"), Resolved::Null);
}

#[test]
fn an_address_inside_an_object_lands_on_the_field_not_the_container() {
    // The half of the rule that is easy to miss. `&node->next` is a pointer to
    // a field, and resolving it to the block would point every arrow at the
    // same box.
    let trace = load("linked_list");
    let step = &trace.steps[30];
    let graph = Graph::of(step);

    // Find a struct field that is not at offset zero, and aim at it.
    let block = &step.heap[0];
    let Value::Struct { fields, .. } = &block.value else {
        panic!("expected the block to hold a struct");
    };
    let inner = fields
        .iter()
        .find(|f| {
            f.value
                .at()
                .address()
                .zip(proton_trace::format::address(&block.addr))
                .is_some_and(|(a, b)| a > b)
        })
        .expect("a struct with a field past its first byte");

    let addr = inner.value.at().addr.clone().unwrap();
    let Resolved::At { target, offset } = graph.resolve(&addr) else {
        panic!(
            "a field address should resolve, got {:?}",
            graph.resolve(&addr)
        );
    };
    assert!(
        target.id.ends_with(&format!(".{}", inner.name)),
        "should land on the field `{}`, landed on `{}`",
        inner.name,
        target.id
    );
    assert_eq!(offset, 0, "an exact field address has no offset");
}

#[test]
fn every_pointer_in_a_real_program_resolves_to_something_sensible() {
    // The whole-graph check. 287 pointers across the run; every one should be
    // null, resolved, or honestly reported as pointing outside what is drawn.
    // An arrow that silently goes nowhere is the failure this catches.
    let trace = load("linked_list");
    let mut counts = (0, 0, 0, 0);

    for step in &trace.steps {
        let graph = Graph::of(step);
        for edge in edges(step, &graph) {
            match edge.kind {
                EdgeKind::Points => counts.0 += 1,
                EdgeKind::Null => counts.1 += 1,
                EdgeKind::Dangling => counts.2 += 1,
                EdgeKind::Unknown => counts.3 += 1,
            }
            if edge.kind == EdgeKind::Points {
                assert!(!edge.to.is_empty(), "a resolved edge needs a destination");
            }
        }
    }

    let (points, null, dangling, unknown) = counts;
    println!("resolved {points}, null {null}, dangling {dangling}, unknown {unknown}");
    assert!(points > 50, "most pointers in a linked list should resolve");
    assert!(null > 0, "a linked list ends in one");
    assert!(
        unknown * 4 < points,
        "too many pointers went nowhere: {unknown} unknown against {points} resolved"
    );
}

#[test]
fn a_pointer_into_freed_memory_is_reported_as_dangling() {
    // The thing the tool exists to show. It has to be distinguishable from
    // "points at nothing I drew" — one is a bug in the traced program and the
    // other is a limit of the renderer.
    let trace = load("dangling");
    let found = trace.steps.iter().any(|step| {
        let graph = Graph::of(step);
        edges(step, &graph)
            .iter()
            .any(|e| e.kind == EdgeKind::Dangling)
    });
    assert!(
        found,
        "the freed block is read after free; some step should show a dangling edge"
    );
}

#[test]
fn an_edge_knows_where_it_starts() {
    // The arrow is drawn between the pointer's own slot and its target, so the
    // slot's address has to survive resolution — without it the line starts
    // from the whole frame instead of from the field.
    let trace = load("linked_list");
    let step = &trace.steps[30];
    let graph = Graph::of(step);

    let resolved: Vec<_> = edges(step, &graph)
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Points)
        .collect();
    assert!(!resolved.is_empty());
    assert!(
        resolved.iter().all(|e| e.from_addr.is_some()),
        "every pointer slot should report its own address"
    );
}

#[test]
fn python_traces_the_same_way_c_does() {
    // The point of the format: the pane has no language-specific code. If a
    // Python trace needed different handling, the seam would be leaking.
    let trace = load("aliasing");
    assert_eq!(trace.language, "python");
    assert_eq!(trace.status, Status::Ok);

    let step = trace.steps.last().unwrap();
    let graph = Graph::of(step);
    let found = edges(step, &graph);
    assert!(
        found.iter().any(|e| e.kind == EdgeKind::Points),
        "aliasing draws two names at one object, which is an edge"
    );
}
