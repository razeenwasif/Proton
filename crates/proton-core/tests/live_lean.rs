//! Proton's Lean, for real: the machine's Lean and Mathlib, made from the
//! Lab pane, checking files as a round's proofs are checked — the
//! toolchain's own `lean`, in the sandbox. It needs Lean made first, so it
//! never runs unless asked for:
//!
//! ```text
//! cargo test -p proton-core --test live_lean -- --ignored --nocapture
//! PROTON_LEAN_RECORD=1 cargo test -p proton-core --test live_lean -- --ignored --nocapture
//! ```
//!
//! Each of the files the offline tests read (`crates/proton-research/fixtures/lean/`)
//! is assembled with Proton's two questions and checked, and what Lean said
//! is read as a round reads it; with `PROTON_LEAN_RECORD=1`, what Lean said
//! is written over the hand-made `.jsonl` beside each. It says how long each
//! check took and the most memory it used, how much Lean keeps on disk, and
//! whether anything of Lean's is in `~/.elan` or `~/.cache`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use proton_lab::lean::check::{self, Check, CheckSpec, Gate, NotStarted};
use proton_lab::lean::{LeanEnv, LeanState};
use proton_lab::RunState;
use proton_research::lean::{self, Ended, LeanVerdict};

/// The fixtures, and what Lean must make of each — the files a vetted round
/// would refuse (`native_decide`, `axiom`, `#exit`) are checked anyway, to see
/// what Lean says of them.
fn expected(name: &str, verdict: &LeanVerdict) -> bool {
    match name {
        "clean" => *verdict == LeanVerdict::Clean,
        "sorry" | "fake" => *verdict == LeanVerdict::Sorry,
        "unsolved" | "unknown" => *verdict == LeanVerdict::Errors,
        "reduce_bool" => matches!(verdict, LeanVerdict::Untrusted(axioms) if axioms.iter().any(|a| a.starts_with("Lean.") || a.contains("._native.native_decide."))),
        "axiom" => matches!(verdict, LeanVerdict::Untrusted(axioms) if axioms.contains(&"cheat".to_string())),
        "exit" => *verdict == LeanVerdict::NoAnswer,
        _ => true,
    }
}

/// The most memory a process has, in bytes, as `ps` says it: sampled.
fn resident(pid: u32) -> Option<u64> {
    let out = std::process::Command::new("/bin/ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().ok().map(|kb| kb * 1024)
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../proton-research/fixtures/lean")
}

#[test]
#[ignore = "needs the machine's Lean and Mathlib, made from the Lab pane"]
fn the_machines_lean_checks_files_as_a_round_does() {
    let mut env = LeanEnv::at_home();
    env.check();
    let LeanState::Ready { lean: version, mathlib, size, home, .. } = env.state().clone() else {
        panic!("Lean is not made on this machine: {:?}", env.state());
    };
    let marker = env.marker().expect("its marker");
    let sandbox = env.sandbox();
    eprintln!("Lean {version}, Mathlib {mathlib}, {:.1} GB in {home}, checked in {sandbox:?}", size as f64 / 1e9);

    let record = std::env::var_os("PROTON_LEAN_RECORD").is_some();
    let work = std::env::temp_dir().join(format!("proton-live-lean-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let mut names: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".lean").map(str::to_string))
        .collect();
    names.sort();
    let mut wrong = Vec::new();
    for name in names {
        let source = std::fs::read_to_string(fixtures().join(format!("{name}.lean"))).unwrap();
        let assembled = lean::assemble(&source, "H004");
        let dir = work.join(&name);
        let spec = CheckSpec {
            dir: dir.clone(),
            source: assembled.text.clone(),
            lean: marker.lean_bin.clone(),
            lean_path: marker.lean_path.clone(),
            lean_home: env.home().to_path_buf(),
            home: env.person().to_path_buf(),
            timeout: Duration::from_secs(600),
            threads: 4,
            sandbox,
            about: serde_json::json!({ "live": name }),
        };
        let started = Instant::now();
        let mut check = match Check::start(&spec, &Arc::new(Gate::default())) {
            Ok(check) => check,
            Err(NotStarted::Busy) => unreachable!("a gate of its own"),
            Err(NotStarted::Failed(why)) => panic!("{name}: {why}"),
        };
        let pid = check.record()["pid"].as_u64().unwrap() as u32;
        let mut most = 0;
        while !check.poll() {
            most = most.max(resident(pid).unwrap_or(0));
            std::thread::sleep(Duration::from_millis(200));
        }
        let out = check::output(&dir).unwrap();
        let ended = match out.state {
            RunState::TimedOut => Ended::TimedOut,
            RunState::Stopped => Ended::Stopped,
            _ => Ended::Exited(out.exit),
        };
        let read = lean::read(&out.lines, ended, &out.err, &assembled);
        eprintln!(
            "{name:12} {:6.1} s  {:5.1} GB  {:?}  vetted: {}  statement: {:?}",
            started.elapsed().as_secs_f64(),
            most as f64 / 1e9,
            read.verdict,
            if lean::vet(&source, "H004").is_ok() { "checked" } else { "refused" },
            read.statement
        );
        if !expected(&name, &read.verdict) {
            wrong.push(format!("{name}: {:?} — {}", read.verdict, out.lines.join("\n")));
        }
        if record {
            let lines: String = out.lines.iter().map(|l| format!("{l}\n")).collect();
            std::fs::write(fixtures().join(format!("{name}.jsonl")), lines).unwrap();
        }
    }
    // What Lean writes is Proton's: nothing of it in the person's own Lean or caches.
    let person = env.person().to_path_buf();
    for elsewhere in [".elan/toolchains", ".cache/mathlib"] {
        let path = person.join(elsewhere);
        let theirs: Vec<String> = std::fs::read_dir(&path).map(|e| e.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
        eprintln!("~/{elsewhere}: {}", if theirs.is_empty() { "nothing".to_string() } else { format!("{} (the person's own?)", theirs.join(", ")) });
    }
    let _ = std::fs::remove_dir_all(&work);
    assert!(wrong.is_empty(), "Lean said otherwise:\n{}", wrong.join("\n\n"));
}
