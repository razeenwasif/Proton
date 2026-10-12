//! The work you have asked for, and what became of it.
//!
//! The agent runs one turn at a time — a conversation is a sequence, and two
//! turns interleaving into one transcript would be neither. So a second
//! question asked while it is busy *waits* rather than being refused, and this
//! is the list it waits in.
//!
//! Two groups, as the design has always drawn them: **in progress** — the one
//! running plus everything behind it — and **ready for review**, which is
//! everything that finished, carrying what it produced. A task that staged
//! changes says which files, because that is the thing you have to go and
//! decide about.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    /// Asked for, waiting its turn.
    Queued,
    Running,
    /// Finished. "Ready" rather than "done" because most of them leave
    /// something for you to look at.
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: usize,
    /// What was asked, as it was typed.
    pub prompt: String,
    pub state: TaskState,
    /// One line about what happened: the answer's first sentence, or why it
    /// failed. Empty while it is still queued.
    pub note: String,
    /// How many tools it ran. The count rather than the calls: the transcript
    /// has those, and this list is a list.
    pub tools: usize,
    /// Files it staged, as chips.
    pub files: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Queue {
    tasks: Vec<Task>,
    next: usize,
}

impl Queue {
    pub fn all(&self) -> &[Task] {
        &self.tasks
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Add a question to the queue and say whether it should start now.
    ///
    /// `running` is whether the agent is already busy — asked for rather than
    /// tracked, because the agent is the one that knows and two answers to
    /// that question would eventually disagree.
    pub fn push(&mut self, prompt: &str, running: bool) -> usize {
        let id = self.next;
        self.next += 1;
        self.tasks.push(Task {
            id,
            prompt: prompt.to_string(),
            state: if running {
                TaskState::Queued
            } else {
                TaskState::Running
            },
            note: String::new(),
            tools: 0,
            files: Vec::new(),
        });
        id
    }

    pub fn running(&self) -> Option<&Task> {
        self.tasks.iter().find(|t| t.state == TaskState::Running)
    }

    fn running_mut(&mut self) -> Option<&mut Task> {
        self.tasks
            .iter_mut()
            .find(|t| t.state == TaskState::Running)
    }

    /// Keep the running task's line up to date while it works.
    /// Whether it changed: the same note and count again is nothing to say.
    pub fn progress(&mut self, note: &str, tools: usize) -> bool {
        match self.running_mut() {
            Some(task) if task.note != note || task.tools != tools => {
                task.note = note.to_string();
                task.tools = tools;
                true
            }
            _ => false,
        }
    }

    /// The running task is over. Returns the next one to start, if any.
    pub fn finished(
        &mut self,
        note: &str,
        tools: usize,
        files: Vec<String>,
        ok: bool,
    ) -> Option<String> {
        if let Some(task) = self.running_mut() {
            task.state = if ok {
                TaskState::Ready
            } else {
                TaskState::Failed
            };
            task.note = note.to_string();
            task.tools = tools;
            task.files = files;
        }
        // The oldest waiting one, because a queue is a queue.
        let next = self
            .tasks
            .iter_mut()
            .find(|t| t.state == TaskState::Queued)?;
        next.state = TaskState::Running;
        Some(next.prompt.clone())
    }

    /// Drop one by id, whatever state it is in.
    ///
    /// A queued one that is dropped simply never runs; a running one is left
    /// alone, because the turn is already in flight and removing its row
    /// would not stop it.
    pub fn drop_task(&mut self, id: usize) -> bool {
        let Some(at) = self.tasks.iter().position(|t| t.id == id) else {
            return false;
        };
        if self.tasks[at].state == TaskState::Running {
            return false;
        }
        self.tasks.remove(at);
        true
    }

    /// Clear everything that has finished, leaving what is still to come.
    pub fn clear_finished(&mut self) {
        self.tasks
            .retain(|t| matches!(t.state, TaskState::Queued | TaskState::Running));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_question_runs_and_the_next_one_waits() {
        let mut queue = Queue::default();
        queue.push("first", false);
        queue.push("second", true);

        assert_eq!(queue.all()[0].state, TaskState::Running);
        assert_eq!(queue.all()[1].state, TaskState::Queued);
        assert_eq!(queue.running().unwrap().prompt, "first");
    }

    #[test]
    fn progress_is_said_to_have_moved_only_when_it_did() {
        let mut queue = Queue::default();
        assert!(!queue.progress("nothing running", 0));
        queue.push("first", false);
        assert!(queue.progress("Reading main.rs", 1));
        assert!(!queue.progress("Reading main.rs", 1), "the same again is no news");
        assert!(queue.progress("Reading main.rs", 2));
    }

    #[test]
    fn finishing_one_starts_the_next() {
        let mut queue = Queue::default();
        queue.push("first", false);
        queue.push("second", true);
        queue.push("third", true);

        let next = queue.finished("answered", 2, vec!["a.rs".into()], true);
        assert_eq!(next.as_deref(), Some("second"));
        assert_eq!(queue.all()[0].state, TaskState::Ready);
        assert_eq!(queue.all()[0].files, vec!["a.rs"]);
        assert_eq!(queue.all()[1].state, TaskState::Running);
        // A queue is a queue: the third is still behind the second.
        assert_eq!(queue.all()[2].state, TaskState::Queued);
    }

    #[test]
    fn the_last_one_finishing_starts_nothing() {
        let mut queue = Queue::default();
        queue.push("only", false);
        assert!(queue.finished("answered", 0, Vec::new(), true).is_none());
        assert_eq!(queue.all()[0].state, TaskState::Ready);
    }

    #[test]
    fn a_failure_is_a_state_rather_than_a_disappearance() {
        // A task that vanished when it failed would leave you wondering
        // whether you had asked at all.
        let mut queue = Queue::default();
        queue.push("doomed", false);
        queue.finished("no credential", 0, Vec::new(), false);
        assert_eq!(queue.all()[0].state, TaskState::Failed);
        assert_eq!(queue.all()[0].note, "no credential");
    }

    #[test]
    fn the_running_one_cannot_be_dropped_but_a_queued_one_can() {
        // Removing the row would not stop the turn, and a list that lies
        // about what is happening is worse than one you cannot tidy.
        let mut queue = Queue::default();
        let running = queue.push("first", false);
        let waiting = queue.push("second", true);

        assert!(!queue.drop_task(running));
        assert!(queue.drop_task(waiting));
        assert_eq!(queue.all().len(), 1);
    }

    #[test]
    fn clearing_keeps_what_has_not_happened_yet() {
        let mut queue = Queue::default();
        queue.push("done", false);
        queue.push("waiting", true);
        queue.finished("answered", 1, Vec::new(), true);

        queue.clear_finished();
        let left: Vec<&str> = queue.all().iter().map(|t| t.prompt.as_str()).collect();
        assert_eq!(left, vec!["waiting"], "the one that started is still going");
    }
}
