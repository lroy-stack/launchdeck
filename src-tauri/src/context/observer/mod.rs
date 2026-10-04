//! Which application is in front, as the operating system tells it.
//!
//! One small interface and one adapter per system. An adapter reads the
//! application's identity and nothing else: no window title, no window
//! contents, no list of what is running. What it sees goes to the engine as a
//! [`Seen`]; no platform handle leaves this module.
//!
//! | System | Mechanism |
//! | --- | --- |
//! | macOS | `NSWorkspace` activation notifications |
//! | Windows | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` |
//! | Linux, X11 | `PropertyNotify` for `_NET_ACTIVE_WINDOW` |
//! | Linux, Wayland | none: not available |

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;

use super::identity::ApplicationIdentity;

#[cfg(target_os = "macos")]
mod macos;
pub mod scripted;
#[cfg(windows)]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11;

/// What is in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Application {
        identity: ApplicationIdentity,
        /// It is LaunchDeck itself.
        own: bool,
    },
    /// No application: the desktop, a window that says nothing about what it
    /// belongs to, or nothing that can be told.
    Nothing,
}

/// Whether this system can tell which application is in front, said honestly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ObserverStatus {
    /// It can, through `mechanism` (a name for people: "X11 window manager").
    Available { mechanism: String },
    /// It could, and something went wrong since. The last known application stands.
    Degraded { mechanism: String, reason: String },
    /// It cannot, here. `reason` says why and, where there is one, what would help.
    Unavailable { reason: String },
}

impl ObserverStatus {
    pub fn available(mechanism: &str) -> ObserverStatus {
        ObserverStatus::Available {
            mechanism: mechanism.to_string(),
        }
    }

    pub fn unavailable(reason: &str) -> ObserverStatus {
        ObserverStatus::Unavailable {
            reason: reason.to_string(),
        }
    }

    /// Whether observations can arrive at all. A degraded observer may recover.
    pub fn can_observe(&self) -> bool {
        !matches!(self, ObserverStatus::Unavailable { .. })
    }
}

/// What an observer reports while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Seen(Seen),
    /// Its ability to observe changed (it lost its connection, for example).
    Status(ObserverStatus),
}

/// Where an observer sends what it sees, and how it learns it should stop.
#[derive(Clone)]
pub struct Sink {
    send: Arc<dyn Fn(Report) + Send + Sync>,
    stop: Arc<AtomicBool>,
}

impl Sink {
    pub fn new(send: impl Fn(Report) + Send + Sync + 'static) -> Sink {
        Sink {
            send: Arc::new(send),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn seen(&self, seen: Seen) {
        (self.send)(Report::Seen(seen));
    }

    pub fn status(&self, status: ObserverStatus) {
        (self.send)(Report::Status(status));
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Ask the observer to end. It notices at its next event or check.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Something that reports which application is in front.
pub trait ForegroundObserver: Send {
    /// What it can do on this system, known before it runs.
    fn status(&self) -> ObserverStatus;

    /// Report the application in front now and every change after it, until
    /// `sink.stopped()`. Blocks: the runtime gives it a thread of its own.
    /// Never called when `status()` is unavailable.
    fn run(self: Box<Self>, sink: Sink);
}

/// An observer for a system that has none. It says why and never runs.
pub struct NoObserver(pub String);

impl ForegroundObserver for NoObserver {
    fn status(&self) -> ObserverStatus {
        ObserverStatus::unavailable(&self.0)
    }

    fn run(self: Box<Self>, _sink: Sink) {}
}

/// The observer for the system LaunchDeck is running on.
pub fn native() -> Box<dyn ForegroundObserver> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacObserver::new())
    }
    #[cfg(windows)]
    {
        Box::new(windows::WindowsObserver::new())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        x11::native()
    }
    #[cfg(not(any(unix, windows)))]
    {
        Box::new(NoObserver(
            "LaunchDeck cannot tell which application is in front on this system.".into(),
        ))
    }
}
