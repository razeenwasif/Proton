//! The client against a real debug adapter.
//!
//! Ignored by default because it compiles a C program and runs a debugger on
//! it — `cargo test -- --include-ignored` runs it, the way the language-server
//! tests do. Everything the protocol module knows is tested against recorded
//! messages; this is the one that proves the messages are the ones a real
//! adapter sends.
//!
//! GDB's own DAP interpreter is the adapter, because it is already on this
//! machine for the Code Visualizer and it debugs C, C++ and Rust.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_dap::{Client, Incoming};

const GDB: &str = r"C:\msys64\ucrt64\bin\gdb.exe";
const GCC: &str = r"C:\msys64\ucrt64\bin\gcc.exe";

const PROGRAM: &str = r#"#include <stdio.h>

int add(int a, int b) {
    int sum = a + b;
    return sum;
}

int main(void) {
    int total = 0;
    for (int i = 1; i <= 3; i++) {
        total = add(total, i);
    }
    printf("total=%d\n", total);
    return 0;
}
"#;

/// A compiled program with debug info, and somewhere to put it.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    /// `None` only when the toolchain genuinely is not here.
    ///
    /// Everything *after* that check panics rather than skipping. A skip that
    /// can also mean "it broke" is a test that passes in 0.05 seconds and
    /// tells you nothing, which is exactly what this did on its first run.
    fn build() -> Option<Fixture> {
        if !Path::new(GDB).is_file() || !Path::new(GCC).is_file() {
            eprintln!("no gdb or gcc on this machine — skipped");
            return None;
        }
        // No parentheses in the name. `ThreadId(2)` would put a pair in the
        // path, and gcc fails on those with an empty stderr — which looks
        // exactly like the compiler not being there.
        let unique = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>();
        let dir = std::env::temp_dir().join(format!("proton-dap-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        std::fs::write(dir.join("demo.c"), PROGRAM).expect("the source");

        // `-O0` because an optimised build has no line 4 to stop on.
        //
        // MSYS2's directory goes on `PATH` for the child: gcc is a driver and
        // finds `cc1`, `as` and `ld` through it. Without that it exits 1 with
        // an *empty* stderr, which looks exactly like the compiler not being
        // installed — `gcc --version` works either way, because it spawns
        // nothing. The Visualizer's sidecar hit the same thing.
        let toolchain = Path::new(GCC).parent().expect("a bin directory");
        let path = std::env::var("PATH").unwrap_or_default();
        let built = std::process::Command::new(GCC)
            .env("PATH", format!("{};{path}", toolchain.display()))
            .args(["-g", "-O0", "-o"])
            .arg(dir.join("demo.exe"))
            .arg(dir.join("demo.c"))
            .output()
            .expect("gcc runs");
        assert!(
            built.status.success(),
            "gcc failed: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        Some(Fixture { dir })
    }

    fn source(&self) -> PathBuf {
        self.dir.join("demo.c")
    }

    fn program(&self) -> PathBuf {
        self.dir.join("demo.exe")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Poll until `want` says yes, or give up.
///
/// The adapter has its own clock — a compile, a process launch, a symbol load
/// — so a test that asks once always fails. Polling is what the shell does on
/// its heartbeat, and doing it here is the same arrangement by hand.
fn until<T>(
    client: &mut Client,
    seen: &mut Vec<Incoming>,
    mut want: impl FnMut(&[Incoming]) -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        seen.extend(client.poll());
        if let Some(found) = want(seen) {
            return Some(found);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "compiles a C program and runs gdb"]
fn a_breakpoint_stops_the_program_and_the_locals_can_be_read() {
    let Some(fixture) = Fixture::build() else {
        return;
    };

    // Line 4 is `int sum = a + b;` inside `add`, which is hit three times.
    let mut client = Client::launch(
        Path::new(GDB),
        &["--interpreter=dap".to_string()],
        &fixture.program(),
        &fixture.dir,
        vec![(fixture.source(), vec![4])],
    )
    .expect("gdb starts");

    let mut seen = Vec::new();

    // The breakpoint's *real* answer, which arrives as an event long after the
    // response said "pending".
    let verified = until(&mut client, &mut seen, |seen| {
        seen.iter().find_map(|item| match item {
            Incoming::BreakpointChanged {
                line: 4,
                verified: true,
                ..
            } => Some(true),
            _ => None,
        })
    });
    assert_eq!(verified, Some(true), "the breakpoint never verified");

    // And the program stops there.
    let thread = until(&mut client, &mut seen, |seen| {
        seen.iter().find_map(|item| match item {
            Incoming::Stopped { thread, reason } if reason == "breakpoint" => Some(*thread),
            _ => None,
        })
    })
    .expect("it never stopped");

    // The stack says where.
    client.stack(thread);
    let frames = until(&mut client, &mut seen, |seen| {
        seen.iter().find_map(|item| match item {
            Incoming::Stack(frames) if !frames.is_empty() => Some(frames.clone()),
            _ => None,
        })
    })
    .expect("no stack came back");
    assert_eq!(frames[0].name, "add", "stopped inside the right function");
    assert_eq!(frames[0].line, 4);
    assert!(
        frames.iter().any(|f| f.name == "main"),
        "and main is below it: {frames:?}"
    );

    // And the locals can be read.
    client.scopes(frames[0].id);
    let scopes = until(&mut client, &mut seen, |seen| {
        seen.iter().find_map(|item| match item {
            Incoming::Scopes(scopes) if !scopes.is_empty() => Some(scopes.clone()),
            _ => None,
        })
    })
    .expect("no scopes came back");

    client.variables(scopes[0].reference);
    let items = until(&mut client, &mut seen, |seen| {
        seen.iter().find_map(|item| match item {
            Incoming::Variables { items, .. } if !items.is_empty() => Some(items.clone()),
            _ => None,
        })
    })
    .expect("no variables came back");

    // First time through the loop: `add(0, 1)`.
    let by_name = |name: &str| {
        items
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.value.clone())
    };
    assert_eq!(by_name("a").as_deref(), Some("0"), "{items:?}");
    assert_eq!(by_name("b").as_deref(), Some("1"), "{items:?}");
}

#[test]
#[ignore = "compiles a C program and runs gdb"]
fn continuing_reaches_the_breakpoint_again_with_different_values() {
    // The loop runs three times, so the same line is hit with `a` growing.
    // This is what proves run control actually resumes rather than the first
    // stop being the only thing that ever works.
    let Some(fixture) = Fixture::build() else {
        return;
    };
    let mut client = Client::launch(
        Path::new(GDB),
        &["--interpreter=dap".to_string()],
        &fixture.program(),
        &fixture.dir,
        vec![(fixture.source(), vec![4])],
    )
    .expect("gdb starts");

    let mut seen = Vec::new();
    let mut values = Vec::new();

    for _ in 0..2 {
        let thread = until(&mut client, &mut seen, |seen| {
            seen.iter().find_map(|item| match item {
                Incoming::Stopped { thread, reason } if reason == "breakpoint" => Some(*thread),
                _ => None,
            })
        })
        .expect("it stopped");

        client.stack(thread);
        let frames = until(&mut client, &mut seen, |seen| {
            seen.iter().find_map(|item| match item {
                Incoming::Stack(frames) if !frames.is_empty() => Some(frames.clone()),
                _ => None,
            })
        })
        .expect("a stack");
        client.scopes(frames[0].id);
        let scopes = until(&mut client, &mut seen, |seen| {
            seen.iter().find_map(|item| match item {
                Incoming::Scopes(scopes) if !scopes.is_empty() => Some(scopes.clone()),
                _ => None,
            })
        })
        .expect("scopes");
        client.variables(scopes[0].reference);
        let items = until(&mut client, &mut seen, |seen| {
            seen.iter().find_map(|item| match item {
                Incoming::Variables { items, .. } if !items.is_empty() => Some(items.clone()),
                _ => None,
            })
        })
        .expect("variables");

        values.push(
            items
                .iter()
                .find(|v| v.name == "b")
                .map(|v| v.value.clone())
                .unwrap_or_default(),
        );

        // Clear what has been seen so the next round waits for a *new* stop
        // rather than matching the one already in the list.
        seen.clear();
        client.resume(thread);
    }

    assert_eq!(values, vec!["1", "2"], "the loop advanced between stops");
}

#[test]
#[ignore = "runs gdb"]
fn an_adapter_that_is_not_there_is_an_error_rather_than_a_hang() {
    let started = Client::launch(
        Path::new("no-such-debug-adapter"),
        &[],
        Path::new("x"),
        &std::env::temp_dir(),
        Vec::new(),
    );
    assert!(started.is_err(), "spawning nothing should fail");
}
