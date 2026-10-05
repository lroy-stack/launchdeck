//! Who holds a unit: what to send once its ports are open and before they
//! close, and when its input counts. Pure: the session says what to do and
//! `Mk3Device` does it; nothing here waits.
//!
//! For every profile it does what the Launchpad X adapter has always done:
//! Programmer mode, then input, then the clear frame; on release the clear
//! frame, then Live mode. Identifying the unit, confirming the mode, giving
//! back the person's layout and watching for a takeover are the steps of
//! section 9 of the Mini MK3 plan (package M5); the render tick that will
//! drive them already reaches `tick`.

use std::time::Instant;

use super::profile::ModelProfile;
use super::protocol::{programmer_mode, Product, Reply};

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not holding the unit: nothing is drawn.
    Closed,
    /// Holding it in Programmer mode: frames are drawn.
    Owned,
}

/// One thing for the device to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// Send this message.
    Send(Vec<u8>),
    /// Send the clear frame: every LED off, and the cache knows it.
    Clear,
    /// Forward the unit's input (`true`) or not.
    Gate(bool),
}

#[derive(Debug)]
pub(crate) struct Session {
    product: Product,
    phase: Phase,
}

impl Session {
    pub(crate) fn new(profile: &ModelProfile) -> Self {
        Self {
            product: profile.product,
            phase: Phase::Closed,
        }
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    /// The ports are open: take the unit.
    pub(crate) fn opened(&mut self) -> Vec<Action> {
        self.phase = Phase::Owned;
        vec![
            Action::Send(programmer_mode(self.product, true)),
            Action::Gate(true),
            Action::Clear,
        ]
    }

    /// A render tick, with the replies that arrived since the last one.
    /// Nothing is asked of the unit yet, so a reply is only logged.
    pub(crate) fn tick(&mut self, _now: Instant, replies: Vec<Reply>) -> Vec<Action> {
        for reply in replies {
            log::debug!("a Launchpad reply nobody asked for: {reply:?}");
        }
        Vec::new()
    }

    /// The ports are about to close: give the unit back. The device ignores
    /// send failures here, since the unit may be gone.
    pub(crate) fn release(&mut self) -> Vec<Action> {
        match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Owned => vec![
                Action::Gate(false),
                Action::Clear,
                Action::Send(programmer_mode(self.product, false)),
            ],
            Phase::Closed => vec![Action::Gate(false)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::novation::testkit::hex;
    use crate::devices::novation::{launchpad_mini_mk3, launchpad_x};

    #[test]
    fn opening_enters_programmer_mode_then_listens_then_clears() {
        let mut session = Session::new(&launchpad_x::PROFILE);
        assert_eq!(session.phase(), Phase::Closed);
        assert_eq!(
            session.opened(),
            [
                Action::Send(hex("F0 00 20 29 02 0C 0E 01 F7")),
                Action::Gate(true),
                Action::Clear,
            ]
        );
        assert_eq!(session.phase(), Phase::Owned);
    }

    #[test]
    fn release_stops_listening_clears_then_returns_to_live_mode_once() {
        let mut session = Session::new(&launchpad_mini_mk3::PROFILE);
        session.opened();
        assert_eq!(
            session.release(),
            [
                Action::Gate(false),
                Action::Clear,
                Action::Send(hex("F0 00 20 29 02 0D 0E 00 F7")),
            ]
        );
        assert_eq!(session.phase(), Phase::Closed);
        assert_eq!(
            session.release(),
            [Action::Gate(false)],
            "nothing to give back"
        );
    }

    #[test]
    fn a_tick_asks_nothing_yet() {
        let mut session = Session::new(&launchpad_x::PROFILE);
        session.opened();
        let reply = Reply::Readback {
            product: 0x0C,
            command: 0x0E,
            data: vec![0x01],
        };
        assert!(session.tick(Instant::now(), vec![reply]).is_empty());
        assert_eq!(session.phase(), Phase::Owned);
    }
}
