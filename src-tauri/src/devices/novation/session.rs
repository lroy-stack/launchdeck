//! Who holds a unit: what to send to take it and to give it back, and when
//! its input counts. Pure: the session says what to do and `Mk3Device` does
//! it; nothing here waits. `Mk3Device` advances it on every render tick with
//! the replies that arrived since the last one and the time, so that no
//! answer is ever waited for under the runtime lock.
//!
//! Section 9 of the Mini MK3 plan, for every profile: on connect the unit is
//! asked who it is (the Universal Device Inquiry), which layout it shows,
//! its LED feedback and whether its LEDs sleep; the next tick that has the
//! answers, or the first one 500 ms later, enters Programmer mode and reads
//! it back; the next tick that has that answer, or the first one 500 ms
//! later, takes the unit: external LED feedback on and the LEDs awake if
//! they were not, the clear frame, then its input counts. Programmer mode
//! is confirmed only if the unit said so; otherwise LaunchDeck still draws
//! and says so in a notice. A unit that answers as its bootloader or as
//! another model is turned down and sent nothing more.
//!
//! An owned unit is asked every 2 s whether it is still in Programmer mode.
//! Live mode in the answer means it was power cycled or another program
//! took it: its input stops counting and it is taken again from the probe
//! on, with the clear frame and a full frame; the third time within 30 s
//! LaunchDeck yields it and sends nothing more. Three unanswered questions
//! in a row put a notice up until the next answer.
//!
//! On release the clear frame, the LED feedback the unit had if LaunchDeck
//! changed it, Live mode, then the layout the unit showed before, unless
//! Live mode lands there anyway. What release gives back is what the unit
//! first reported: taking it again after a takeover reads the unit again,
//! but what LaunchDeck or the other program set there is not the person's.
//! Replies that carry another model's product byte are ignored.

use std::time::{Duration, Instant};

use super::profile::ModelProfile;
use super::protocol::{
    feedback, programmer_mode, query, select_layout, sleep, Reply, FEEDBACK, INQUIRY, LAYOUT, MODE,
    SLEEP,
};
use crate::devices::{DeviceDetails, DeviceError, ModeState};

/// How long a question to the unit waits for its answer. Some drivers may
/// drop SysEx: then LaunchDeck goes on without the answer.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
/// How often an owned unit is asked whether it is still in Programmer mode.
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(2);
/// Unanswered heartbeats in a row before the person is told.
pub(crate) const MISSED_LIMIT: u32 = 3;
/// Takeovers within `TAKEOVER_WINDOW` after which LaunchDeck yields.
pub(crate) const TAKEOVER_LIMIT: usize = 3;
pub(crate) const TAKEOVER_WINDOW: Duration = Duration::from_secs(30);

/// The second byte of an inquiry family answered by a bootloader (MINI-PRM
/// p6: `13 11` for the Mini MK3's `13 01`).
const BOOTLOADER: u8 = 0x11;

/// The layout byte of Programmer mode (MINI-PRM p7).
const PROGRAMMER_LAYOUT: u8 = 0x7F;

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not holding the unit: nothing is drawn.
    Closed,
    /// Asked who the unit is; waiting for the answer since then.
    Probing { since: Instant },
    /// Asked for Programmer mode and to read it back; waiting since then.
    Entering { since: Instant },
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
    /// It left Programmer mode `TAKEOVER_LIMIT` times within
    /// `TAKEOVER_WINDOW`: another program is using it.
    TakenOver,
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
    /// What the unit read back about Programmer mode, if it answered.
    programmer: Option<bool>,
    /// What the last probe was told: what taking the unit changes.
    seen: Reported,
    /// The first answer to each question since connect: what release
    /// gives back.
    saved: Reported,
    /// External LED feedback was turned on: release turns it back off.
    feedback_changed: bool,
    /// While owned: when to ask next, and what is unanswered.
    heartbeat: Option<Heartbeat>,
    /// When the unit was found back in Live mode, within the window.
    takeovers: Vec<Instant>,
}

/// What the unit reported before it was taken; `None` for what it did not
/// answer.
#[derive(Debug, Default, Clone, Copy)]
struct Reported {
    layout: Option<u8>,
    /// Internal and external LED feedback, 0 or 1 each.
    feedback: Option<(u8, u8)>,
    /// Its LEDs were asleep.
    asleep: Option<bool>,
}

/// The question an owned unit is asked every `HEARTBEAT`.
#[derive(Debug, Clone, Copy)]
struct Heartbeat {
    due: Instant,
    /// When the question still unanswered was sent.
    asked: Option<Instant>,
    /// Questions in a row that went unanswered.
    missed: u32,
}

impl Session {
    pub(crate) fn new(profile: &'static ModelProfile) -> Self {
        Self {
            profile,
            phase: Phase::Closed,
            identified: false,
            firmware: None,
            programmer: None,
            seen: Reported::default(),
            saved: Reported::default(),
            feedback_changed: false,
            heartbeat: None,
            takeovers: Vec::new(),
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

    /// The ports are open: ask who the unit is and what it shows.
    pub(crate) fn opened(&mut self, now: Instant) -> Vec<Action> {
        *self = Self::new(self.profile);
        self.probe(now)
    }

    /// A render tick, with the replies that arrived since the last one.
    pub(crate) fn tick(&mut self, now: Instant, replies: Vec<Reply>) -> Vec<Action> {
        let mut actions = Vec::new();
        for reply in replies {
            actions.extend(self.receive(now, reply));
        }
        actions.extend(match self.phase {
            Phase::Probing { since } if self.probe_answered() || timed_out(since, now) => {
                self.enter(now)
            }
            Phase::Entering { since } if self.programmer.is_some() || timed_out(since, now) => {
                self.take(now)
            }
            Phase::Owned => self.beat(now),
            _ => Vec::new(),
        });
        actions
    }

    /// The ports are about to close: give the unit back. The device ignores
    /// send failures here, since the unit may be gone. A unit that was not
    /// taken is sent nothing.
    pub(crate) fn release(&mut self) -> Vec<Action> {
        let product = self.profile.product;
        match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Entering { .. } | Phase::Owned => {
                let mut actions = vec![Action::Gate(false), Action::Clear];
                if self.feedback_changed {
                    if let Some((internal, external)) = self.saved.feedback {
                        actions.push(Action::Send(feedback(
                            product,
                            internal == 1,
                            external == 1,
                        )));
                    }
                }
                actions.push(Action::Send(programmer_mode(product, false)));
                // Live mode lands in the profile's Live layout; another one
                // the unit showed is selected again.
                let layout = self.saved.layout.filter(|&layout| {
                    layout != PROGRAMMER_LAYOUT && layout != self.profile.live_layout
                });
                actions.extend(layout.map(|layout| Action::Send(select_layout(product, layout))));
                actions
            }
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
            Refusal::TakenOver => DeviceError::TakenOver(self.profile.name.into()),
        })
    }

    pub(crate) fn details(&self) -> DeviceDetails {
        let mode = self.mode();
        let name = self.profile.name;
        let missed = self.heartbeat.map_or(0, |h| h.missed);
        let notice = match mode {
            ModeState::Confirmed if missed >= MISSED_LIMIT => {
                Some(format!("The {name} stopped answering."))
            }
            ModeState::Unconfirmed => Some(format!(
                "{name} connected, but Programmer Mode could not be confirmed."
            )),
            _ => None,
        };
        DeviceDetails {
            firmware: self.firmware.clone(),
            mode,
            notice,
            ..DeviceDetails::default()
        }
    }

    fn mode(&self) -> ModeState {
        match self.phase {
            Phase::Closed | Phase::Refused(_) => ModeState::None,
            Phase::Probing { .. } | Phase::Entering { .. } => ModeState::Acquiring,
            Phase::Owned if self.programmer == Some(true) => ModeState::Confirmed,
            Phase::Owned => ModeState::Unconfirmed,
        }
    }

    fn receive(&mut self, now: Instant, reply: Reply) -> Vec<Action> {
        match (self.phase, reply) {
            (
                Phase::Probing { .. },
                Reply::Identity {
                    family, version, ..
                },
            ) => self.identify(family, version),
            (_, Reply::Readback { product, .. }) if product != self.profile.product.0 => {
                log::warn!(
                    "a reply for product {product:02X} on the ports of a {}: ignored",
                    self.profile.name
                );
            }
            (Phase::Probing { .. }, Reply::Readback { command, data, .. }) => {
                self.probed(command, &data)
            }
            (
                Phase::Entering { .. },
                Reply::Readback {
                    command: MODE,
                    data,
                    ..
                },
            ) if matches!(data[..], [0 | 1]) => self.programmer = Some(data[0] == 1),
            (
                Phase::Owned,
                Reply::Readback {
                    command: MODE,
                    data,
                    ..
                },
            ) if matches!(data[..], [0 | 1]) => return self.answered(now, data[0] == 1),
            (_, reply) => log::debug!("a Launchpad reply nobody asked for: {reply:?}"),
        }
        Vec::new()
    }

    /// An answer to one of the probe's readback questions.
    fn probed(&mut self, command: u8, data: &[u8]) {
        let (seen, saved) = (&mut self.seen, &mut self.saved);
        match (command, data) {
            (LAYOUT, &[layout]) => {
                seen.layout = Some(layout);
                saved.layout.get_or_insert(layout);
            }
            (FEEDBACK, &[internal @ (0 | 1), external @ (0 | 1)]) => {
                seen.feedback = Some((internal, external));
                saved.feedback.get_or_insert((internal, external));
            }
            (SLEEP, &[leds @ (0 | 1)]) => {
                seen.asleep = Some(leds == 0);
                saved.asleep.get_or_insert(leds == 0);
            }
            _ => log::debug!("a Launchpad readback nobody asked for: {command:02X} {data:02X?}"),
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

    /// Who the unit is and what it shows: the first step of taking it, on
    /// connect and after a takeover.
    fn probe(&mut self, now: Instant) -> Vec<Action> {
        self.phase = Phase::Probing { since: now };
        self.identified = false;
        self.programmer = None;
        self.seen = Reported::default();
        self.heartbeat = None;
        let product = self.profile.product;
        vec![
            Action::Send(INQUIRY.to_vec()),
            Action::Send(query(product, LAYOUT)),
            Action::Send(query(product, FEEDBACK)),
            Action::Send(query(product, SLEEP)),
        ]
    }

    /// Whether everything the probe asked was answered.
    fn probe_answered(&self) -> bool {
        let seen = &self.seen;
        self.identified && seen.layout.is_some() && seen.feedback.is_some() && seen.asleep.is_some()
    }

    /// Programmer mode, and the question whether the unit is in it.
    fn enter(&mut self, now: Instant) -> Vec<Action> {
        self.phase = Phase::Entering { since: now };
        let product = self.profile.product;
        vec![
            Action::Send(programmer_mode(product, true)),
            Action::Send(query(product, MODE)),
        ]
    }

    /// External LED feedback on and the LEDs awake, if they were not, the
    /// clear frame, then the unit's input counts.
    fn take(&mut self, now: Instant) -> Vec<Action> {
        self.phase = Phase::Owned;
        self.heartbeat = Some(Heartbeat {
            due: now + HEARTBEAT,
            asked: None,
            missed: 0,
        });
        let product = self.profile.product;
        let mut actions = Vec::new();
        // External feedback off hides lighting by notes in Programmer mode
        // (MINI-UG p19); whether it hides lighting by SysEx is unknown.
        // Internal feedback only acts in Custom modes and is left alone.
        if let Some((internal, 0)) = self.seen.feedback {
            actions.push(Action::Send(feedback(product, internal == 1, true)));
            self.feedback_changed = true;
        }
        if self.seen.asleep == Some(true) {
            actions.push(Action::Send(sleep(product, true)));
        }
        actions.extend([Action::Clear, Action::Gate(true)]);
        actions
    }

    /// Counts an unanswered heartbeat, and asks again when one is due.
    fn beat(&mut self, now: Instant) -> Vec<Action> {
        let Some(heartbeat) = self.heartbeat.as_mut() else {
            return Vec::new();
        };
        if heartbeat.asked.is_some_and(|asked| timed_out(asked, now)) {
            heartbeat.asked = None;
            heartbeat.missed = heartbeat.missed.saturating_add(1);
        }
        if now < heartbeat.due {
            return Vec::new();
        }
        heartbeat.due = now + HEARTBEAT;
        heartbeat.asked = Some(now);
        vec![Action::Send(query(self.profile.product, MODE))]
    }

    /// A heartbeat's answer. Programmer mode: all is well (and confirmed,
    /// if it was not). Live mode: the unit was power cycled or another
    /// program took it, so its input stops counting and it is taken again,
    /// unless that is the `TAKEOVER_LIMIT`th time within `TAKEOVER_WINDOW`.
    fn answered(&mut self, now: Instant, programmer: bool) -> Vec<Action> {
        if let Some(heartbeat) = self.heartbeat.as_mut() {
            heartbeat.asked = None;
            heartbeat.missed = 0;
        }
        if programmer {
            self.programmer = Some(true);
            return Vec::new();
        }
        self.takeovers
            .retain(|&at| now.saturating_duration_since(at) < TAKEOVER_WINDOW);
        self.takeovers.push(now);
        log::info!(
            "the {} left Programmer mode ({} time(s) within {} s)",
            self.profile.name,
            self.takeovers.len(),
            TAKEOVER_WINDOW.as_secs()
        );
        let mut actions = vec![Action::Gate(false)];
        if self.takeovers.len() >= TAKEOVER_LIMIT {
            self.phase = Phase::Refused(Refusal::TakenOver);
        } else {
            actions.extend(self.probe(now));
        }
        actions
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

    fn readback(product: u8, command: u8, data: &[u8]) -> Reply {
        Reply::Readback {
            product,
            command,
            data: data.to_vec(),
        }
    }

    const MINI: [u8; 2] = [0x13, 0x01];
    const X: [u8; 2] = [0x03, 0x01];

    fn probing(profile: &'static ModelProfile) -> (Session, Instant) {
        let mut session = Session::new(profile);
        let t0 = Instant::now();
        let product = profile.product;
        assert_eq!(
            session.opened(t0),
            [
                Action::Send(INQUIRY.to_vec()),
                Action::Send(query(product, LAYOUT)),
                Action::Send(query(product, FEEDBACK)),
                Action::Send(query(product, SLEEP)),
            ]
        );
        assert_eq!(session.phase(), Phase::Probing { since: t0 });
        (session, t0)
    }

    /// What a unit of this family showing this layout answers the probe,
    /// with its LED feedback on and its LEDs awake.
    fn answers(profile: &ModelProfile, family: [u8; 2], layout: u8) -> Vec<Reply> {
        let product = profile.product.0;
        vec![
            identity(family, [0, 5, 3, 1]),
            readback(product, LAYOUT, &[layout]),
            readback(product, FEEDBACK, &[1, 1]),
            readback(product, SLEEP, &[1]),
        ]
    }

    /// Identified as this family, showing this layout, and confirmed in
    /// Programmer mode.
    fn owned_from(
        profile: &'static ModelProfile,
        family: [u8; 2],
        layout: u8,
    ) -> (Session, Instant) {
        let (mut session, t0) = probing(profile);
        session.tick(t0, answers(profile, family, layout));
        session.tick(t0, vec![readback(profile.product.0, MODE, &[1])]);
        assert!(session.draws());
        (session, t0)
    }

    fn owned(profile: &'static ModelProfile, family: [u8; 2]) -> (Session, Instant) {
        owned_from(profile, family, profile.live_layout)
    }

    #[test]
    fn the_probe_asks_who_the_unit_is_and_what_it_shows() {
        let mut session = Session::new(&launchpad_mini_mk3::PROFILE);
        assert_eq!(
            session.opened(Instant::now()),
            [
                Action::Send(hex("F0 7E 7F 06 01 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 00 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 0A F7")),
                Action::Send(hex("F0 00 20 29 02 0D 09 F7")),
            ]
        );
    }

    #[test]
    fn an_identified_unit_enters_programmer_mode_and_is_taken_once_it_says_so() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert_eq!(session.details().mode, ModeState::Acquiring);
        let mut answers = answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05);
        let sleep = answers.pop().unwrap();
        assert!(
            session.tick(t0, answers).is_empty(),
            "whether the LEDs sleep is not known yet"
        );
        assert_eq!(
            session.tick(t0, vec![sleep]),
            [
                Action::Send(hex("F0 00 20 29 02 0D 0E 01 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 0E F7")),
            ]
        );
        assert_eq!(session.phase(), Phase::Entering { since: t0 });
        assert_eq!(session.details().mode, ModeState::Acquiring);
        assert_eq!(session.details().firmware.as_deref(), Some("0531"));
        assert_eq!(
            session.tick(t0, vec![readback(0x0D, MODE, &[1])]),
            [Action::Clear, Action::Gate(true)]
        );
        assert!(session.draws());
        let details = session.details();
        assert_eq!((details.mode, details.notice), (ModeState::Confirmed, None));
        assert!(session.tick(t0, Vec::new()).is_empty(), "taken once");
    }

    #[test]
    fn a_unit_that_does_not_answer_is_taken_unconfirmed_after_two_timeouts() {
        let (mut session, t0) = probing(&launchpad_x::PROFILE);
        let almost = t0 + REPLY_TIMEOUT - Duration::from_millis(1);
        assert!(session.tick(almost, Vec::new()).is_empty());
        let t1 = t0 + REPLY_TIMEOUT;
        assert_eq!(
            session.tick(t1, Vec::new())[0],
            Action::Send(hex("F0 00 20 29 02 0C 0E 01 F7"))
        );
        assert!(session.tick(t1 + REPLY_TIMEOUT / 2, Vec::new()).is_empty());
        assert!(!session.draws());
        assert_eq!(
            session.tick(t1 + REPLY_TIMEOUT, Vec::new()),
            [Action::Clear, Action::Gate(true)]
        );
        let details = session.details();
        assert_eq!(details.mode, ModeState::Unconfirmed);
        assert_eq!(details.firmware, None);
        assert_eq!(
            details.notice.as_deref(),
            Some("Launchpad X connected, but Programmer Mode could not be confirmed.")
        );
    }

    #[test]
    fn a_unit_that_reads_back_live_mode_is_taken_unconfirmed() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        session.tick(t0, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
        session.tick(t0, vec![readback(0x0D, MODE, &[0])]);
        assert!(session.draws());
        assert_eq!(session.details().mode, ModeState::Unconfirmed);
    }

    #[test]
    fn replies_for_another_product_are_ignored() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        let foreign = answers(&launchpad_x::PROFILE, MINI, 0x04);
        assert!(
            session.tick(t0, foreign).is_empty(),
            "the layout is the X's"
        );
        session.tick(t0, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
        assert!(session
            .tick(t0, vec![readback(0x0C, MODE, &[1])])
            .is_empty());
        assert_eq!(session.phase(), Phase::Entering { since: t0 });
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
        assert!(session.tick(t0, vec![identity(X, [0, 2, 3, 8])]).is_empty());
        assert_eq!(
            session.refusal().unwrap().detail(),
            Some("inquiry family 03 01 on the ports of a Launchpad Mini MK3")
        );
    }

    #[test]
    fn the_launchpad_x_accepts_its_family_and_the_manuals_misprint() {
        owned(&launchpad_x::PROFILE, X);
        owned(&launchpad_x::PROFILE, MINI);
        let (mut session, t0) = probing(&launchpad_x::PROFILE);
        session.tick(t0, vec![identity([0x03, 0x11], [0, 0, 0, 1])]);
        assert!(matches!(
            session.refusal(),
            Some(DeviceError::Bootloader(_))
        ));
    }

    #[test]
    fn release_stops_listening_clears_then_returns_to_live_mode_once() {
        let given_back = [
            Action::Gate(false),
            Action::Clear,
            Action::Send(hex("F0 00 20 29 02 0D 0E 00 F7")),
        ];
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        assert_eq!(session.release(), [Action::Gate(false)], "not taken yet");
        session.opened(t0);
        session.tick(t0, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
        assert_eq!(
            session.release(),
            given_back,
            "Programmer mode was asked for"
        );

        let (mut session, _) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        assert_eq!(session.release(), given_back);
        assert_eq!(session.phase(), Phase::Closed);
        assert_eq!(
            session.release(),
            [Action::Gate(false)],
            "nothing to give back"
        );
    }

    #[test]
    fn release_selects_the_layout_the_unit_showed_unless_live_mode_lands_there() {
        let live = hex("F0 00 20 29 02 0D 0E 00 F7");
        let (mut session, _) = owned_from(&launchpad_mini_mk3::PROFILE, MINI, 0x04);
        assert_eq!(
            session.release()[2..],
            [
                Action::Send(live.clone()),
                Action::Send(hex("F0 00 20 29 02 0D 00 04 F7")),
            ]
        );
        for layout in [0x05, PROGRAMMER_LAYOUT] {
            let (mut session, _) = owned_from(&launchpad_mini_mk3::PROFILE, MINI, layout);
            assert_eq!(
                session.release().last(),
                Some(&Action::Send(live.clone())),
                "{layout:02X}"
            );
        }
        let (mut session, _) = owned_from(&launchpad_x::PROFILE, X, 0x01);
        assert_eq!(
            session.release().last(),
            Some(&Action::Send(hex("F0 00 20 29 02 0C 0E 00 F7"))),
            "Note mode, where the X's Live mode lands"
        );
    }

    #[test]
    fn a_layout_the_unit_did_not_report_is_not_selected() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        session.tick(t0, vec![identity(MINI, [0, 5, 3, 1])]);
        session.tick(t0 + REPLY_TIMEOUT, Vec::new());
        session.tick(t0 + REPLY_TIMEOUT, vec![readback(0x0D, MODE, &[1])]);
        assert!(session.draws());
        assert_eq!(
            session.release().last(),
            Some(&Action::Send(hex("F0 00 20 29 02 0D 0E 00 F7")))
        );
    }

    #[test]
    fn taking_a_unit_turns_external_feedback_on_and_wakes_it_and_release_restores_feedback() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        let mut answers = answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05);
        answers[2] = readback(0x0D, FEEDBACK, &[1, 0]);
        answers[3] = readback(0x0D, SLEEP, &[0]);
        session.tick(t0, answers);
        assert_eq!(
            session.tick(t0, vec![readback(0x0D, MODE, &[1])]),
            [
                Action::Send(hex("F0 00 20 29 02 0D 0A 01 01 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 09 01 F7")),
                Action::Clear,
                Action::Gate(true),
            ]
        );
        assert_eq!(
            session.release(),
            [
                Action::Gate(false),
                Action::Clear,
                Action::Send(hex("F0 00 20 29 02 0D 0A 01 00 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 0E 00 F7")),
            ]
        );
    }

    #[test]
    fn feedback_the_unit_did_not_report_is_left_alone() {
        let (mut session, t0) = probing(&launchpad_x::PROFILE);
        let mut answers = answers(&launchpad_x::PROFILE, X, 0x01);
        answers.remove(2);
        session.tick(t0, answers);
        session.tick(t0 + REPLY_TIMEOUT, Vec::new());
        assert_eq!(
            session.tick(t0 + REPLY_TIMEOUT, vec![readback(0x0C, MODE, &[1])]),
            [Action::Clear, Action::Gate(true)]
        );
        assert_eq!(session.release().len(), 3, "no feedback to give back");
    }

    const SECOND: Duration = Duration::from_secs(1);

    fn mini_mode(on: u8) -> Vec<Reply> {
        vec![readback(0x0D, MODE, &[on])]
    }

    #[test]
    fn an_owned_unit_is_asked_every_two_seconds_whether_it_is_in_programmer_mode() {
        let (mut session, t0) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        let ask = [Action::Send(hex("F0 00 20 29 02 0D 0E F7"))];
        assert!(session
            .tick(t0 + HEARTBEAT - SECOND / 100, Vec::new())
            .is_empty());
        assert_eq!(session.tick(t0 + HEARTBEAT, Vec::new()), ask);
        assert!(session.tick(t0 + HEARTBEAT, mini_mode(1)).is_empty());
        assert!(session.tick(t0 + 3 * SECOND, Vec::new()).is_empty());
        assert_eq!(session.tick(t0 + 2 * HEARTBEAT, Vec::new()), ask);
    }

    /// Owned at `t0`, then found back in Live mode by the heartbeat at
    /// `at`: what the session does about it.
    fn found_in_live_mode(session: &mut Session, at: Instant) -> Vec<Action> {
        assert_eq!(session.tick(at, Vec::new()).len(), 1, "the heartbeat");
        session.tick(at, mini_mode(0))
    }

    #[test]
    fn live_mode_in_the_answer_takes_the_unit_again() {
        let (mut session, t0) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        let t1 = t0 + HEARTBEAT;
        let mut probe = vec![Action::Gate(false)];
        probe.extend(probing(&launchpad_mini_mk3::PROFILE).0.opened(t1));
        assert_eq!(found_in_live_mode(&mut session, t1), probe);
        assert_eq!(session.phase(), Phase::Probing { since: t1 });
        assert_eq!(session.details().mode, ModeState::Acquiring);
        session.tick(t1, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
        assert_eq!(
            session.tick(t1, mini_mode(1)),
            [Action::Clear, Action::Gate(true)]
        );
        assert_eq!(session.details().mode, ModeState::Confirmed);
    }

    #[test]
    fn the_third_takeover_within_thirty_seconds_yields_the_unit() {
        let (mut session, t0) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        let mut at = t0;
        for _ in 1..TAKEOVER_LIMIT {
            at += HEARTBEAT;
            found_in_live_mode(&mut session, at);
            session.tick(at, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x00));
            session.tick(at, mini_mode(1));
            assert!(session.draws());
        }
        at += HEARTBEAT;
        assert_eq!(found_in_live_mode(&mut session, at), [Action::Gate(false)]);
        assert_eq!(session.phase(), Phase::Refused(Refusal::TakenOver));
        assert_eq!(
            session.refusal().unwrap().to_string(),
            "Another program is using the Launchpad Mini MK3. Press Look again to use it here."
        );
        assert!(session.tick(at + HEARTBEAT, Vec::new()).is_empty());
        assert_eq!(session.release(), [Action::Gate(false)], "nothing sent");
    }

    #[test]
    fn takeovers_further_apart_than_the_window_do_not_add_up() {
        let (mut session, t0) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        let mut at = t0;
        for _ in 0..TAKEOVER_LIMIT {
            at += TAKEOVER_WINDOW / 2 + SECOND;
            found_in_live_mode(&mut session, at);
            assert!(session.refusal().is_none());
            session.tick(at, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
            session.tick(at, mini_mode(1));
        }
        assert!(session.draws());
    }

    #[test]
    fn three_unanswered_heartbeats_put_up_a_notice_until_the_next_answer() {
        let (mut session, t0) = owned(&launchpad_mini_mk3::PROFILE, MINI);
        let mut at = t0;
        for missed in 1..=MISSED_LIMIT {
            at += HEARTBEAT;
            assert_eq!(session.tick(at, Vec::new()).len(), 1, "asked");
            assert_eq!(session.details().notice, None, "{missed} missed");
            at += REPLY_TIMEOUT;
            session.tick(at, Vec::new());
            at -= REPLY_TIMEOUT;
        }
        let details = session.details();
        assert_eq!(details.mode, ModeState::Confirmed);
        assert_eq!(
            details.notice.as_deref(),
            Some("The Launchpad Mini MK3 stopped answering.")
        );
        session.tick(at + HEARTBEAT, Vec::new());
        session.tick(at + HEARTBEAT, mini_mode(1));
        assert_eq!(session.details().notice, None);
    }

    #[test]
    fn release_after_a_takeover_gives_back_what_the_unit_first_reported() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        let mut first = answers(&launchpad_mini_mk3::PROFILE, MINI, 0x04);
        first[2] = readback(0x0D, FEEDBACK, &[1, 0]);
        session.tick(t0, first);
        assert_eq!(
            session.tick(t0, mini_mode(1)).len(),
            3,
            "feedback turned on"
        );
        // Another program selects its own layout; feedback stays as set.
        let t1 = t0 + HEARTBEAT;
        found_in_live_mode(&mut session, t1);
        session.tick(t1, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x00));
        assert_eq!(
            session.tick(t1, mini_mode(1)),
            [Action::Clear, Action::Gate(true)],
            "feedback is on already"
        );
        assert_eq!(
            session.release()[2..],
            [
                Action::Send(hex("F0 00 20 29 02 0D 0A 01 00 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 0E 00 F7")),
                Action::Send(hex("F0 00 20 29 02 0D 00 04 F7")),
            ]
        );
    }

    #[test]
    fn opening_again_forgets_a_refusal() {
        let (mut session, t0) = probing(&launchpad_mini_mk3::PROFILE);
        session.tick(t0, vec![identity([0x13, 0x11], [0, 1, 0, 2])]);
        session.release();
        session.opened(t0);
        assert!(session.refusal().is_none());
        session.tick(t0, answers(&launchpad_mini_mk3::PROFILE, MINI, 0x05));
        session.tick(t0, vec![readback(0x0D, MODE, &[1])]);
        assert!(session.draws());
    }
}
