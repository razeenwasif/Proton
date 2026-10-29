//! C9's budget for the simulator: a small CPU clocked at 100 kHz or more.
//! The CPU is `tiny-cpu.circuit`, written by `examples/tiny_cpu.rs`. Its
//! own table proves it computes; the clock is then flipped for a second.
//! Speed is a release build's to keep: a debug build only says how fast.
//!
//! ```text
//! cargo test --release -p proton-circuit --test speed -- --nocapture
//! ```

use std::time::{Duration, Instant};

const MOST_HZ_NEEDED: f64 = 100_000.0;

fn tiny_cpu() -> proton_circuit::dig::Built {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tiny-cpu.circuit");
    let project = proton_circuit::circ::read(&dir).unwrap();
    project.build(&project.top).unwrap()
}

#[test]
fn the_tiny_cpu_counts_down_by_its_own_table() {
    let built = tiny_cpu();
    let outcomes = proton_circuit::testlang::run_all(&built);
    assert_eq!(outcomes.len(), 1);
    for o in outcomes {
        assert!(o.passed(), "{}: {:?} {:?}", o.label, o.error, o.failures.first());
        assert_eq!(o.rows, 37);
    }
}

#[test]
fn a_small_cpu_is_clocked_at_100_khz_or_more() {
    let built = tiny_cpu();
    let &(clock, _) = built.inputs.get("CLK").unwrap();
    let mut sim = proton_circuit::Sim::new(built.netlist.clone(), 0x5eed).unwrap();
    let started = Instant::now();
    let mut cycles = 0u64;
    while started.elapsed() < Duration::from_secs(1) {
        for _ in 0..1000 {
            sim.tick(clock).unwrap();
        }
        cycles += 1000;
    }
    let hz = cycles as f64 / started.elapsed().as_secs_f64();
    println!("tiny-cpu: {} parts, {} nets, {cycles} cycles, {:.0} Hz", built.netlist.parts(), built.netlist.nets(), hz);
    // Clocked this long, the program has long since stopped at seven.
    assert_eq!(sim.shown(built.outputs["ACC"]).get(), 7);
    if !cfg!(debug_assertions) {
        assert!(hz >= MOST_HZ_NEEDED, "{hz:.0} Hz is under {MOST_HZ_NEEDED:.0}");
    }
}
