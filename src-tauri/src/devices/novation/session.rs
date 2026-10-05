//! Who holds a unit: what to send to take it and to give it back, and when
//! its input counts. Pure: the session says what to do and `Mk3Device` does
//! it; nothing here waits. `Mk3Device` advances it on every render tick with
//! the replies that arrived since the last one and the time, so that no
//! answer is ever waited for under the runtime lock.
//!
//! Section 9 of the Mini MK3 plan, for every profile: on connect the unit is
//! asked who it is (the Universal Device Inquiry); the next tick that has the
//! answer, or the first one 500 ms later, takes it: Programmer mode, the
//! clear frame, then its input counts. A unit that answers as its bootloader
//! or as another model is turned down and sent nothing more. On release the
//! clear frame, then Live mode.

use std::time::{Duration, Instant};

use super::profile::ModelProfile;
use super::protocol::{programmer_mode, Reply, INQUIRY};
use crate::devices::{DeviceDetails, DeviceError, ModeState};

/// How long a question to the unit waits for its answer. Some drivers may
/// drop SysEx: then LaunchDeck goes on without the answer.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_millis(500);

/// The second byte of an inquiry family answered by a bootloader (MINI-PRM
/// p6: `13 11` for the Mini MK3's `13 01`).
const BOOTLOADER: u8 = 0x11;

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not holding the unit: nothing is drawn.
    Closed,
    /// Asked who the unit is; waiting for the answer since then.
    Probing { since: Instant },
    /// Holding it in Programmer mode: frames are drawn.
    Owned,
    /// Turned down: nothing more is sent, every render fails.
    Refused(Refusal),
}

/// Why LaunchDeck turned a unit down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// It answered as its bootloader, with this version.
    Bootloader([u8; 4]),
    /// It answered with a family its profile does not have.
    OtherModel([u8; 2]),
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
    profile: &'static ModelProfile,
    phase: Phase,
    /// The inquiry was answered by the profile's model.
    identified: bool,
    /// The four inquiry digits, as the bootloader shows them.
    firmware: Option<String>,
}

impl Session {
    pub(crate) fn new(profile: &'static ModelProfile) -> Self {
        Self {
            profile,
            phase: Phase::Closed,
            identified: false,
            firmware: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether frames are drawn.
    pub(crate) fn draws(&self) -> bool {
        self.phase == Phase::Owned
    }

    /// The ports are open: ask who the unit is.
    pub(crate) fn opened(&mut self, now: Instant) -> Vec<Action> {
        *self = Self::new(self.profile);
        self.phase = Phase::Probing { since: now };
        vec![Action::Send(INQUIRY.to_vec())]
    }

    /// A render tick, with the replies that arrived since the last one.
    pub(crate) fn tick(&mut self, now: Instant, replies: Vec<Reply>) -> Vec<Action> {
        for reply in replies {
            self.receive(reply);
        }
        match self.phase {
            Phase::Probing { since } if self.identified || timed_out(since, now) => self.take(),
            _ => Vec::new(),
        }
    }

    /// The ports are about to close: give the unit back. The device ignores
    /// send failures here, since the unit may be gone. A unit that was not
    /// taken is sent nothing.
    pub(crate) fn release(&mut self) -> Vec<Action> {
        match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Owned => vec![
                Action::Gate(false),
                Action::Clear,
                Action::Send(programmer_mode(self.profile.product, false)),
            ],
            Phase::Closed | Phase::Probing { .. } | Phase::Refused(_) => vec![Action::Gate(false)],
        }
    }

    /// What turned the unit down, as the sentence the person reads.
    pub(crate) fn refusal(&self) -> Option<DeviceError> {
        let Phase::Refused(refusal) = self.phase else {
            return None;
        };
        Some(match refusal {
            Refusal::Bootloader(version) => {
                DeviceError::Bootloader(format!("bootloader version {}", digits(version)))
            }
            Refusal::OtherModel([a, b]) => DeviceError::OtherModel(format!(
                "inquiry family {a:02X} {b:02X} on the ports of a {}",
                self.profile.name
            )),
        })
    }

    pub(crate) fn details(&self) -> DeviceDetails {
        let mode = match self.phase {
            Phase::Closed | Phase::Refused(_) => ModeState::None,
            Phase::Probing { .. } => ModeState::Acquiring,
            Phase::Owned => ModeState::Unconfirmed,
        };
        DeviceDetails {
            firmware: self.firmware.clone(),
            mode,
            ..DeviceDetails::default()
        }
    }

    fn receive(&mut self, reply: Reply) {
        match (self.phase, reply) {
            (
                Phase::Probing { .. },
                Reply::Identity {
                    family, version, ..
                },
            ) => self.identify(family, version),
            (_, reply) => log::debug!("a Launchpad reply nobody asked for: {reply:?}"),
        }
    }

    /// The inquiry's answer: the profile's model, its bootloader, or
    /// another model.
    fn identify(&mut self, family: [u8; 2], version: [u8; 4]) {
        let families = self.profile.families;
        if families.contains(&family) {
            self.identified = true;
            self.firmware = Some(digits(version));
        } else if family[1] == BOOTLOADER && families.iter().any(|f| f[0] == family[0]) {
            self.phase = Phase::Refused(Refusal::Bootloader(version));
        } else {
            self.phase = Phase::Refused(Refusal::OtherModel(family));
        }
    }

    /// Programmer mode, the clear frame, then the unit's input counts.
    fn take(&mut self) -> Vec<Action> {
        self.phase = Phase::Owned;
        vec![
            Action::Send(programmer_mode(self.profile.product, true)),
            Action::Clear,
            Action::Gate(true),
        ]
    }
}

fn timed_out(since: Instant, now: Instant) -> bool {
    now.saturating_duration_since(since) >= REPLY_TIMEOUT
}

/// Four digits 0-9 as one string: `0531`.
fn digits(version: [u8; 4]) -> String {
    version.iter().map(|d| char::from(b'0' + d % 10)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::novation::testkit::hex;
    use crate::devices::novation::{launchpad_mini_mk3, launchpad_x};

    fn identity(family: [u8; 2], version: [u8; 4]) -> Reply {
        Reply::Identity {
            family,
            member: [0, 0],
            version,
        }
    }

    const MINI: [u8; 2] = [0x13, 0x01];

    fn probing(profile: &'static ModelProfile) -> (Session, Instant) {
        let mut session = Session::new(profile);
        let t0 = Instant::now();
        assert_eq!(session.opened(t0), [Action::Send(hex("F0 7E 7F 06 01 F7"))]);
        assert_eq!(session.phase(), Phase::Probing { since: t0 });
        (session, t0)
    }

    #[test]
    fn an_identified_unit_is_taken_on_the_next_tick() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert_eq!(session.details().mode, ModeState::Acquiring);
        assert_eq!(
            session.tick(t0, vec![identity(MINI, [0, 5, 3, 1])]),
            [
                Action::Send(hex("F0 00 20 29 02 0D 0E 01 F7")),
                Action::Clear,
                Action::Gate(true),
            ]
        );
        assert!(session.draws());
        assert_eq!(session.details().firmware.as_deref(), Some("0531"));
        assert!(session.tick(t0, Vec::new()).is_empty(), "taken once");
    }

    #[test]
    fn a_unit_that_does_not_answer_is_taken_after_the_timeout() {
        let (mut session, t0) = probing(&launchpad_x::PROFILE);
        let almost = t0 + REPLY_TIMEOUT - Duration::from_millis(1);
        assert!(session.tick(almost, Vec::new()).is_empty());
        assert!(!session.draws());
        assert_eq!(
            session.tick(t0 + REPLY_TIMEOUT, Vec::new())[0],
            Action::Send(hex("F0 00 20 29 02 0C 0E 01 F7"))
        );
        assert!(session.draws());
        assert_eq!(session.details().firmware, None);
    }

    #[test]
    fn a_bootloader_or_another_model_is_turned_down_and_sent_nothing() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert!(session
            .tick(t0, vec![identity([0x13, 0x11], [0, 1, 0, 2])])
            .is_empty());
        assert_eq!(
            session.phase(),
            Phase::Refused(Refusal::Bootloader([0, 1, 0, 2]))
        );
        let error = session.refusal().unwrap();
        assert!(matches!(error, DeviceError::Bootloader(_)));
        assert_eq!(error.detail(), Some("bootloader version 0102"));
        assert!(session.tick(t0 + REPLY_TIMEOUT, Vec::new()).is_empty());
        assert_eq!(session.details().mode, ModeState::None);
        assert_eq!(session.release(), [Action::Gate(false)]);

        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert!(session
            .tick(t0, vec![identity([0x03, 0x01], [0, 2, 3, 8])])
            .is_empty());
        assert_eq!(
            session.refusal().unwrap().detail(),
            Some("inquiry family 03 01 on the ports of a Launchpad Mini MK3")
        );
    }

    #[test]
    fn the_launchpad_x_accepts_its_family_and_the_manuals_misprint() {
        for family in [[0x03, 0x01], [0x13, 0x01]] {
            let (mut session, t0) = probing(&launchpad_x::PROFILE);
            session.tick(t0, vec![identity(family, [0, 2, 3, 8])]);
            assert!(session.draws(), "{family:02X?}");
        }
        let (mut session, t0) = probing(&launchpad_x::PROFILE);
        session.tick(t0, vec![identity([0x03, 0x11], [0, 0, 0, 1])]);
        assert!(matches!(
            session.refusal(),
            Some(DeviceError::Bootloader(_))
        ));
    }

    #[test]
    fn release_stops_listening_clears_then_returns_to_live_mode_once() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert_eq!(session.release(), [Action::Gate(false)], "not taken yet");
        session.opened(t0);
        session.tick(t0, vec![identity(MINI, [0, 5, 3, 1])]);
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
    fn opening_again_forgets_a_refusal() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        session.tick(t0, vec![identity([0x13, 0x11], [0, 1, 0, 2])]);
        session.release();
        session.opened(t0);
        assert!(session.refusal().is_none());
        session.tick(t0, vec![identity(MINI, [0, 5, 3, 1])]);
        assert!(session.draws());
    }
}
