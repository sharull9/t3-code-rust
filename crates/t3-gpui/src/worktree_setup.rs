//! Progress of a thread starting in a new worktree: the steps between
//! sending the first message and the agent picking it up. The backend
//! reports each step (see `Command::StartThread`); `T3App` keeps one
//! [`WorktreeSetup`] per thread and hands it to whichever view shows that
//! thread, since the draft's view is replaced once the thread streams in.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Checkout,
    SetupScript,
    StartAgent,
}

impl Stage {
    pub const ALL: [Self; 3] = [Self::Checkout, Self::SetupScript, Self::StartAgent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Checkout => "Check out files",
            Self::SetupScript => "Run setup script",
            Self::StartAgent => "Start agent",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What the backend reports for a stage. The text is for Details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageUpdate {
    Started,
    Done(Option<String>),
    Skipped(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StageState {
    Pending,
    Running(Instant),
    Done(Duration),
    Skipped,
    Failed,
}

#[derive(Debug, Clone)]
pub struct WorktreeSetup {
    pub states: [StageState; 3],
    /// Detail lines in arrival order, labelled by stage.
    pub details: Vec<String>,
}

impl Default for WorktreeSetup {
    fn default() -> Self {
        Self { states: [StageState::Pending, StageState::Pending, StageState::Pending], details: Vec::new() }
    }
}

impl WorktreeSetup {
    pub fn apply(&mut self, stage: Stage, update: StageUpdate) {
        let state = &mut self.states[stage.index()];
        let note = match update {
            StageUpdate::Started => {
                *state = StageState::Running(Instant::now());
                None
            }
            StageUpdate::Done(note) => {
                let elapsed = match state {
                    StageState::Running(started) => started.elapsed(),
                    _ => Duration::ZERO,
                };
                *state = StageState::Done(elapsed);
                note
            }
            StageUpdate::Skipped(note) => {
                *state = StageState::Skipped;
                Some(note)
            }
            StageUpdate::Failed(error) => {
                *state = StageState::Failed;
                Some(error)
            }
        };
        if let Some(note) = note {
            self.details.push(format!("{}: {note}", stage.label()));
        }
    }

    pub fn failed(&self) -> bool {
        self.states.iter().any(|state| *state == StageState::Failed)
    }

    /// The agent has its message; nothing is left to show.
    pub fn finished(&self) -> bool {
        matches!(self.states[Stage::StartAgent.index()], StageState::Done(_))
    }
}

/// "1ms", "2.4s", "1m 5s".
pub fn format_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    match ms {
        0..1_000 => format!("{ms}ms"),
        1_000..60_000 => format!("{:.1}s", duration.as_secs_f32()),
        _ => format!("{}m {}s", ms / 60_000, (ms / 1_000) % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_advance_and_failures_are_noted() {
        let mut setup = WorktreeSetup::default();
        setup.apply(Stage::Checkout, StageUpdate::Started);
        assert!(matches!(setup.states[0], StageState::Running(_)));
        setup.apply(Stage::Checkout, StageUpdate::Done(Some("t3code/abc at /w".into())));
        setup.apply(Stage::SetupScript, StageUpdate::Skipped("No setup script".into()));
        assert!(!setup.failed() && !setup.finished());
        setup.apply(Stage::StartAgent, StageUpdate::Failed("offline".into()));
        assert!(setup.failed());
        assert_eq!(
            setup.details,
            [
                "Check out files: t3code/abc at /w",
                "Run setup script: No setup script",
                "Start agent: offline"
            ]
        );
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(format_duration(Duration::from_millis(1)), "1ms");
        assert_eq!(format_duration(Duration::from_millis(2_400)), "2.4s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
    }
}
