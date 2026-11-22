//! One ordering shared by invoke replies and background pushes.
//! Assign sequence numbers while holding the session lock, before either
//! channel can be delayed. Empty polls do not consume a sequence number.
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use proton_core::{Command, Event, Session};
use serde::{Deserialize, Serialize};

pub(crate) const FAST: Duration = Duration::from_millis(16);
pub(crate) const BEAT: Duration = Duration::from_millis(50);
pub(crate) const IDLE: Duration = Duration::from_millis(250);

/// A lock that outlives a panic: the previous command's, already reported.
pub(crate) fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The heartbeat's alarm clock: sleep until the next beat, or until woken.
pub(crate) struct Heartbeat {
    woken: Mutex<bool>,
    bell: Condvar,
}

impl Heartbeat {
    pub(crate) const fn new() -> Heartbeat {
        Heartbeat {
            woken: Mutex::new(false),
            bell: Condvar::new(),
        }
    }

    /// Something happened: poll now rather than at the next beat.
    pub(crate) fn wake(&self) {
        if let Ok(mut woken) = self.woken.lock() {
            *woken = true;
            self.bell.notify_one();
        }
    }

    /// Sleep for `wait`, or less if woken. `true` if it was woken.
    pub(crate) fn sleep(&self, wait: Duration) -> bool {
        let Ok(mut woken) = self.woken.lock() else {
            std::thread::sleep(wait);
            return false;
        };
        if !*woken {
            woken = match self.bell.wait_timeout(woken, wait) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        std::mem::replace(&mut *woken, false)
    }
}

/// How long to sleep, given how long it has been since anything happened.
pub(crate) fn cadence(quiet: Duration) -> Duration {
    if quiet < Duration::from_secs(1) {
        FAST
    } else if quiet < Duration::from_secs(5) {
        BEAT
    } else {
        IDLE
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatch {
    pub sequence: u64,
    pub events: Vec<Event>,
}

pub struct BridgeSession {
    pub session: Session,
    sequence: u64,
}

impl BridgeSession {
    pub fn new(session: Session) -> Self {
        Self { session, sequence: 0 }
    }

    pub fn apply(&mut self, command: Command) -> EventBatch {
        let events = self.session.apply(command);
        self.batch(events)
    }

    /// Events the session made other than by a command, numbered as a
    /// command's are.
    pub fn batch(&mut self, events: Vec<Event>) -> EventBatch {
        if !events.is_empty() {
            self.sequence += 1;
        }
        EventBatch { sequence: self.sequence, events }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_and_polls_share_a_sequence_without_idle_gaps() {
        let mut bridge = BridgeSession::new(Session::new(std::env::temp_dir()));
        let initial = bridge.apply(Command::Sync);
        assert_eq!(initial.sequence, 1);
        assert!(!initial.events.is_empty());
        let idle = bridge.apply(Command::Poll);
        assert!(idle.events.is_empty());
        assert_eq!(idle.sequence, 1);
        let next = bridge.apply(Command::Sync);
        assert_eq!(next.sequence, 2);
        let json = serde_json::to_value(next).unwrap();
        assert_eq!(json["sequence"], 2);
        assert!(json["events"].is_array());
    }

    #[test]
    fn the_heartbeat_slows_as_things_go_quiet() {
        assert_eq!(cadence(Duration::ZERO), FAST);
        assert_eq!(cadence(Duration::from_secs(2)), BEAT);
        assert_eq!(cadence(Duration::from_secs(60)), IDLE);
    }

    #[test]
    fn a_wake_ends_the_sleep_at_once_and_is_used_up() {
        let beat = std::sync::Arc::new(Heartbeat::new());
        let waker = std::sync::Arc::clone(&beat);
        let started = std::time::Instant::now();
        let ringer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            waker.wake();
        });
        assert!(beat.sleep(Duration::from_secs(10)), "slept through the wake");
        assert!(started.elapsed() < Duration::from_secs(2));
        ringer.join().unwrap();
        // Woken once, so the next sleep is a whole one.
        assert!(!beat.sleep(Duration::from_millis(30)));
    }
}
