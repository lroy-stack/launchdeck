//! Context Engine: the surface follows the application in front.
//!
//! ```text
//! observer (per system)  →  ApplicationIdentity  →  ordered rules
//!        →  Auto / Manual Override state machine  →  the runtime's activation
//! ```
//!
//! * [`identity`]: what an application is called, stably.
//! * [`rules`]: the saved settings and the pure evaluation of them.
//! * [`engine`]: when a decision may be applied.
//! * [`observer`]: which application is in front, per operating system.
//! * [`status`]: what the interface is shown about all of it.
//! * [`portable`]: an application as a Pack names it on every system, and
//!   what it is on this one.
//!
//! Nothing here knows about MIDI, a device or the renderer, and nothing here
//! is written to disk except the rules themselves.

pub mod engine;
pub mod identity;
pub mod observer;
pub mod portable;
pub mod rules;
pub mod status;

pub use engine::{Decision, Engine, Held, Mode, Outcome, World, SETTLE};
pub use identity::ApplicationIdentity;
pub use observer::{ForegroundObserver, ObserverStatus, Report, Seen, Sink};
pub use portable::{PortableApplication, PortableIdentity, Resolved};
pub use rules::{ContextRule, ContextSettings, Fallback, Target};
pub use status::ContextStatus;
