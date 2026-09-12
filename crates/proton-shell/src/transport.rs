//! One ordering shared by invoke replies and background pushes.
//! Assign sequence numbers while holding the session lock, before either
//! channel can be delayed. Empty polls do not consume a sequence number.
use proton_core::{Command, Event, Session};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
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
}
