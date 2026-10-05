//! An observer that reports what it is told to. It stands in for the
//! operating system in tests of the engine and the runtime, which then need
//! no display.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use super::{ForegroundObserver, ObserverStatus, Report, Sink};
use crate::context::identity::X11;

pub struct ScriptedObserver {
    status: ObserverStatus,
    reports: Receiver<Report>,
}

/// The other end: what a test says is in front.
#[derive(Clone)]
pub struct Script(Sender<Report>);

impl Script {
    pub fn report(&self, report: Report) {
        let _ = self.0.send(report);
    }
}

impl ScriptedObserver {
    pub fn new(status: ObserverStatus) -> (ScriptedObserver, Script) {
        let (tx, rx) = channel();
        (
            ScriptedObserver {
                status,
                reports: rx,
            },
            Script(tx),
        )
    }
}

impl ForegroundObserver for ScriptedObserver {
    fn status(&self) -> ObserverStatus {
        self.status.clone()
    }

    /// X11 identities, whatever system the tests run on.
    fn namespace(&self) -> Option<&'static str> {
        Some(X11)
    }

    fn run(self: Box<Self>, sink: Sink) {
        while !sink.stopped() {
            match self.reports.recv_timeout(Duration::from_millis(50)) {
                Ok(Report::Seen(seen)) => sink.seen(seen),
                Ok(Report::Status(status)) => sink.status(status),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}
