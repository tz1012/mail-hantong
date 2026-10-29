//! Bounded restart decisions; process control belongs to the core supervisor.
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildExit {
    Clean,
    RequestedStop,
    Maintenance,
    AlreadyRunning,
    Abnormal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartDecision {
    Stop,
    RestartAfter(Duration),
    PauseAndNotify,
}

/// One supervisor's retry budget. Reconstructing this value starts a new budget;
/// persistence across supervisor crashes is deliberately not implied.
#[derive(Default)]
pub struct RestartBudget {
    attempts: usize,
    stopped: bool,
}

impl RestartBudget {
    /// `runtime` must be measured from a monotonic clock for this child process.
    /// This method neither sleeps nor starts a process or retries a mail action.
    pub fn child_exited(&mut self, reason: ChildExit, runtime: Duration) -> RestartDecision {
        const DELAYS: [u64; 5] = [1, 2, 4, 8, 16];
        if self.stopped {
            return RestartDecision::Stop;
        }
        if reason != ChildExit::Abnormal {
            self.stopped = true;
            return RestartDecision::Stop;
        }
        if runtime >= Duration::from_secs(300) {
            self.attempts = 0;
        }
        match DELAYS.get(self.attempts) {
            Some(seconds) => {
                self.attempts += 1;
                RestartDecision::RestartAfter(Duration::from_secs(*seconds))
            }
            None => {
                self.stopped = true;
                RestartDecision::PauseAndNotify
            }
        }
    }
}

#[cfg(test)]
mod tests;
