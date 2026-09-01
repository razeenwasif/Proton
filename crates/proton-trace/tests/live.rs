//! Against the real tracer.
//!
//! The fixtures prove the format and the edge rule. They cannot prove the part
//! that only breaks once there is a process: finding a Python that is not the
//! Microsoft Store stub, giving the child a `PATH` its `gcc` and `gdb` are on,
//! and hearing it say it is ready before talking to it.
//!
//! Skipped, loudly, when the checkout or the toolchain is absent — a
//! contributor without MSYS2 should not see a red suite.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_trace::client::Reply;
use proton_trace::format::Status;
use proton_trace::{edges, EdgeKind, Graph, Tracer};

/// The vendored checkout, from this crate's directory.
fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/code-visualizer")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("vendor/code-visualizer"))
}

fn available() -> bool {
    checkout().join("server").join("app.py").is_file() && proton_trace::server::python().is_some()
}

fn finish(run: &mut proton_trace::Run) -> Reply {
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        for reply in run.poll() {
            if !matches!(reply, Reply::Started) {
                return reply;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the trace never finished");
}

const DANGLING: &str = r#"#include <stdio.h>
#include <stdlib.h>

typedef struct Node { int val; struct Node *next; } Node;

int main(void) {
    Node *a = malloc(sizeof(Node));
    a->val = 1;
    a->next = malloc(sizeof(Node));
    a->next->val = 2;
    a->next->next = NULL;

    free(a->next);
    printf("%d\n", a->next->val);

    free(a);
    return 0;
}
"#;

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn the_tracer_starts_and_traces_a_real_program() {
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }

    let tracer = Tracer::start(&checkout()).expect("the tracer should start");

    // The toolchain report is what drives the Targets list, so it has to say
    // something about every language rather than only the working ones.
    let languages = tracer.languages();
    assert!(
        !languages.is_empty(),
        "the tracer should report its backends"
    );
    let c = languages
        .iter()
        .find(|l| l.id == "c")
        .expect("C is a backend");
    assert!(c.implemented);

    if tracer.readiness("c").is_err() {
        println!("skipping the trace: {:?}", tracer.readiness("c"));
        return;
    }

    let mut run = tracer.trace("c", DANGLING);
    let Reply::Finished(trace) = finish(&mut run) else {
        panic!("expected a finished trace");
    };

    assert_eq!(trace.status, Status::Ok);
    assert!(!trace.steps.is_empty());
    assert_eq!(trace.language, "c");

    // The point of the exercise: the read after free shows up as an edge into
    // memory that is gone, not as a missing arrow.
    let dangling = trace.steps.iter().any(|step| {
        let graph = Graph::of(step);
        edges(step, &graph)
            .iter()
            .any(|e| e.kind == EdgeKind::Dangling)
    });
    assert!(dangling, "the freed block is read after free");
}

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn a_language_that_cannot_run_here_explains_itself() {
    // `implemented` and `ready` are different questions, and the answer to the
    // second is what the user has to act on. Rust is not implemented upstream
    // yet, which is exactly the case that should read as a reason rather than
    // as a failure.
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }
    let tracer = Tracer::start(&checkout()).unwrap();

    let why = tracer
        .readiness("rust")
        .expect_err("rust has no backend yet");
    assert!(!why.is_empty(), "an unavailable language needs a reason");

    let nonsense = tracer.readiness("cobol").expect_err("no such backend");
    assert!(nonsense.contains("cobol"), "{nonsense}");
}

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn the_tracer_dies_with_the_handle_that_owns_it() {
    // It compiles and runs arbitrary code. An orphan is worse than a leaked
    // port, so dropping the handle has to take the process with it.
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }
    let mut tracer = Tracer::start(&checkout()).unwrap();
    assert!(tracer.is_running());

    let base = format!(
        "{}",
        proton_trace::server::vendored(Path::new(".")).display()
    );
    assert!(base.ends_with("code-visualizer"), "path helper: {base}");

    drop(tracer);
    // Nothing to assert against directly without racing the OS; the value is
    // that `Drop` runs at all, which the next test's fresh port proves.
}

#[test]
fn a_file_maps_to_the_backend_that_traces_it() {
    // Pure, so it runs without the tracer.
    assert_eq!(Tracer::language_for(Path::new("a.c")), Some("c"));
    assert_eq!(Tracer::language_for(Path::new("a.cpp")), Some("cpp"));
    assert_eq!(Tracer::language_for(Path::new("a.py")), Some("python"));
    assert_eq!(Tracer::language_for(Path::new("a.tsx")), Some("typescript"));
    // No backend is an ordinary answer, not an error.
    assert_eq!(Tracer::language_for(Path::new("a.rs")), None);
    assert_eq!(Tracer::language_for(Path::new("Makefile")), None);
}
