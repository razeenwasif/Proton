//! The engine every plugin runs on, and the deadline it runs under.
//!
//! One thing here is load-bearing and everything else is arrangement around
//! it: **wasm does not stop on its own.** By default an infinite loop inside a
//! plugin spins forever and takes the thread with it, and "plugins cannot hang
//! the editor" is a claim rather than a property. Wasmtime offers two ways to
//! make it true — fuel, which counts instructions, and epochs, which count
//! time — and this uses epochs, because the guarantee people actually want is
//! measured in milliseconds and not in instructions. A tight loop and a slow
//! loop should both be stopped after the same wall-clock wait.
//!
//! An epoch is a number the engine reads and a thread outside bumps. Wasm
//! checks it at every loop back-edge and every call, so the check costs about
//! nothing and there is nowhere for a plugin to hide from it. One ticker
//! thread serves the whole engine, so a hundred plugins cost one timer rather
//! than a hundred.

use std::sync::Arc;
use std::time::Duration;

use wasmtime::{Config, Engine, Store};

/// How long any one call into a plugin may take before the engine stops it.
///
/// A tenth of a second is generous for the work a plugin is meant to do and
/// short enough that a broken one is a blink rather than a hang. It is not
/// configurable yet, and when it becomes so it is a ceiling a plugin may lower
/// rather than raise — the whole point is that the editor decides.
pub const DEADLINE: Duration = Duration::from_millis(100);

/// How often the ticker bumps the epoch.
///
/// The deadline is counted in ticks, so this is also its resolution: a plugin
/// is stopped somewhere between `DEADLINE` and `DEADLINE + TICK`. Finer costs
/// a wakeup for nothing; coarser makes the deadline a suggestion.
const TICK: Duration = Duration::from_millis(10);

/// The wasm engine, and the thread that keeps its clock.
#[derive(Clone)]
pub struct Host {
    engine: Engine,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").finish()
    }
}

impl Host {
    /// Build the engine and start its ticker.
    ///
    /// Fails only if this build of wasmtime cannot do what is asked of it,
    /// which is a build problem rather than a runtime one — but it is returned
    /// rather than unwrapped, because an editor that cannot load plugins is
    /// still an editor.
    pub fn new() -> Result<Host, String> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        // The line the guarantee rests on. Without it `set_epoch_deadline`
        // below is ignored and a loop runs forever.
        config.epoch_interruption(true);
        // A plugin is a few hundred kilobytes and is compiled once per load.
        // Cranelift's cheapest tier is the right trade: a plugin's own work
        // is small, and spending a second optimising it to save a millisecond
        // is the wrong way round.
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);

        let engine = Engine::new(&config).map_err(|err| format!("wasm engine: {err}"))?;

        // One ticker for the whole engine. It holds a *weak* handle, so it
        // stops on its own when the last `Host` goes — a thread that outlives
        // what it was ticking is a leak nobody sees.
        let weak = engine.weak();
        std::thread::Builder::new()
            .name("wasm-epoch".into())
            .spawn(move || loop {
                std::thread::sleep(TICK);
                match weak.upgrade() {
                    Some(engine) => engine.increment_epoch(),
                    None => return,
                }
            })
            .map_err(|err| format!("epoch ticker: {err}"))?;

        Ok(Host { engine })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// A store with the deadline already set.
    ///
    /// Every path that runs plugin code goes through here, so there is no way
    /// to start a call that is not on a clock — which is the only way a
    /// guarantee like this survives a codebase growing around it.
    pub fn store<T>(&self, data: T) -> Store<T> {
        let mut store = Store::new(&self.engine, data);
        store.set_epoch_deadline(ticks(DEADLINE));
        store
    }

    /// Give a call that is already running its full deadline again.
    ///
    /// For the one place it is honest to: a plugin that has just returned
    /// from a host call has not been looping, and charging it for the time
    /// the *host* spent reading a file would make the deadline a measure of
    /// how slow the editor is.
    pub fn refresh<T>(store: &mut Store<T>) {
        store.set_epoch_deadline(ticks(DEADLINE));
    }
}

/// A duration as a number of ticks, rounded up and never zero — a deadline of
/// zero ticks means "stop at the next tick", which would kill every plugin.
fn ticks(within: Duration) -> u64 {
    let ticks = within.as_millis().div_ceil(TICK.as_millis());
    (ticks as u64).max(1)
}

/// Shared so the tree of plugins can hold one without cloning an engine per
/// plugin — `Engine` is already reference-counted, and this makes that
/// obvious at the call site.
pub type Shared = Arc<Host>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deadline_is_at_least_one_tick() {
        // Zero ticks means "stop at the next one", which would kill every
        // plugin before it ran an instruction.
        assert_eq!(ticks(Duration::from_millis(0)), 1);
        assert_eq!(ticks(Duration::from_millis(1)), 1);
        assert_eq!(ticks(TICK), 1);
        // Rounded up: a deadline that rounds *down* is one the plugin never
        // gets all of.
        assert_eq!(ticks(Duration::from_millis(11)), 2);
        assert_eq!(ticks(DEADLINE), 10);
    }

    #[test]
    fn the_engine_starts_and_carries_the_deadline() {
        let host = Host::new().expect("an engine");
        // Nothing to assert about the store's deadline directly — wasmtime
        // does not read it back — so this is a smoke test that the config is
        // accepted. The guarantee itself is tested against real wasm in
        // `tests/deadline.rs`, which is where it belongs.
        let _store = host.store(());
    }
}
