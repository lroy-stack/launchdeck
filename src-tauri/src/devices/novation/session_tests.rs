//! The ownership lifecycle of `session.rs`, step by step, with the time
//! given by hand: identity, Programmer mode and its readback, what taking a
//! unit changes and release gives back, the heartbeat, takeovers and
//! notices. `device_tests.rs` checks the same lifecycle as bytes on the wire.

use std::time::{Duration, Instant};

use super::profile::ModelProfile;
use super::protocol::{query, Reply, FEEDBACK, INQUIRY, LAYOUT, MODE, SLEEP};
use super::session::{
    Action, Phase, Refusal, Session, HEARTBEAT, MISSED_LIMIT, REPLY_TIMEOUT, TAKEOVER_LIMIT,
    TAKEOVER_WINDOW,
};
use super::testkit::hex;
use super::{launchpad_mini_mk3, launchpad_x};
use crate::devices::{DeviceError, ModeState};

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
fn owned_from(profile: &'static ModelProfile, family: [u8; 2], layout: u8) -> (Session, Instant) {
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
    for layout in [0x05, 0x7F] {
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
