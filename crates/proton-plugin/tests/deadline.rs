//! The guarantee: a plugin that will not stop is stopped.
//!
//! Half of M5's exit criterion, and the half that is a property rather than a
//! feature — so it is tested against real wasm running on the real engine
//! rather than against anything that stands in for one. The modules are
//! written in the text format and assembled at test time, which needs no wasm
//! toolchain to run: the point is what the engine does with a loop, and the
//! smallest honest loop is three lines of WAT.

use std::time::{Duration, Instant};

use proton_plugin::Host;
use wasmtime::{Instance, Module};

/// A module with one export, from text.
fn module(host: &Host, text: &str) -> Module {
    let bytes = wat::parse_str(text).expect("valid wat");
    Module::new(host.engine(), bytes).expect("a module")
}

#[test]
fn a_plugin_that_never_returns_is_killed() {
    // The whole reason `epoch_interruption` is switched on. Without it this
    // test does not fail — it hangs, and so does the editor.
    let host = Host::new().expect("an engine");
    let module = module(&host, r#"(module (func (export "spin") (loop (br 0))))"#);

    let mut store = host.store(());
    let instance = Instance::new(&mut store, &module, &[]).expect("an instance");
    let spin = instance
        .get_typed_func::<(), ()>(&mut store, "spin")
        .expect("the export");

    let started = Instant::now();
    let result = spin.call(&mut store, ());
    let took = started.elapsed();

    assert!(result.is_err(), "an infinite loop must not return Ok");
    // Stopped near the deadline rather than eventually. The upper bound is
    // generous because a loaded machine can miss a tick, but it is an order
    // of magnitude below "forever", which is what is being ruled out.
    assert!(
        took < Duration::from_secs(2),
        "it should have been stopped promptly, took {took:?}"
    );
    // The trap itself, not the message: `Interrupt` is what the epoch
    // deadline raises, and asserting on it rules out having been stopped by
    // something incidental — a stack overflow, an out-of-bounds — which
    // would pass a test that only checked that it failed.
    assert_eq!(
        result
            .unwrap_err()
            .downcast_ref::<wasmtime::Trap>()
            .copied(),
        Some(wasmtime::Trap::Interrupt),
        "it should have been stopped by the deadline"
    );
}

#[test]
fn a_plugin_that_returns_is_left_alone() {
    // The other half, and the one that would fail if the deadline were too
    // tight or the ticks counted wrong: ordinary work must not be killed.
    let host = Host::new().expect("an engine");
    let module = module(
        &host,
        r#"(module
             (func (export "add") (param i32 i32) (result i32)
               local.get 0
               local.get 1
               i32.add))"#,
    );

    let mut store = host.store(());
    let instance = Instance::new(&mut store, &module, &[]).expect("an instance");
    let add = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .expect("the export");

    assert_eq!(add.call(&mut store, (2, 40)).expect("it should return"), 42);
}

#[test]
fn a_slow_loop_is_stopped_as_well_as_a_tight_one() {
    // Why epochs rather than fuel. Fuel counts instructions, so a loop that
    // does very little per iteration burns it fast and a loop that does a lot
    // per iteration burns it slowly — two plugins hanging the editor for the
    // same wall-clock time would get very different treatment. Epochs count
    // time, which is the thing anybody waiting actually cares about.
    let host = Host::new().expect("an engine");
    let module = module(
        &host,
        // A loop whose body is a hundred adds rather than nothing.
        &format!(
            r#"(module (func (export "spin") (local i32) (loop {} (br 0))))"#,
            "local.get 0 i32.const 1 i32.add local.set 0 ".repeat(100)
        ),
    );

    let mut store = host.store(());
    let instance = Instance::new(&mut store, &module, &[]).expect("an instance");
    let spin = instance
        .get_typed_func::<(), ()>(&mut store, "spin")
        .expect("the export");

    let started = Instant::now();
    assert!(spin.call(&mut store, ()).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_deadline_is_per_call_rather_than_per_plugin() {
    // A plugin that did some work must not have less time next time it is
    // asked. Without a fresh deadline the second call inherits whatever was
    // left of the first one's, and a plugin gets slower every time you use
    // it until it is killed for no reason at all.
    let host = Host::new().expect("an engine");
    let module = module(
        &host,
        r#"(module
             (func (export "work") (result i32)
               (local i32)
               (loop $l
                 local.get 0
                 i32.const 1
                 i32.add
                 local.set 0
                 local.get 0
                 i32.const 2000000
                 i32.lt_s
                 br_if $l)
               local.get 0))"#,
    );

    let mut store = host.store(());
    let instance = Instance::new(&mut store, &module, &[]).expect("an instance");
    let work = instance
        .get_typed_func::<(), i32>(&mut store, "work")
        .expect("the export");

    for round in 0..5 {
        Host::refresh(&mut store);
        assert!(
            work.call(&mut store, ()).is_ok(),
            "round {round} should still have its own deadline"
        );
    }
}
