//! `Mk3Device`: a Novation MK3 unit as a `SurfaceDevice`, for every model
//! profile. It opens the unit's MIDI-interface pair through the transport,
//! does what the `Session` says when the ports open, on each render and
//! before they close, and draws frames through the `LedCache` once the
//! session holds the unit. A unit the session turned down fails every
//! render with the reason, and is given nothing back on disconnect.
//!
//! What the unit sends is framed into whole messages: a SysEx reply goes to
//! a queue the session reads on the next render; anything else, while the
//! input gate is open, is decoded by the shared layout and sent on as a
//! `DeviceInput`, with velocity only for a model that has it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use super::layout;
use super::leds::LedCache;
use super::ports::FoundUnit;
use super::profile::ModelProfile;
use super::protocol::{parse_reply, Reply};
use super::session::{Action, Session};
use crate::devices::midi::{MessageFramer, MidiBackend, MidiLink, OnBytes};
use crate::devices::{
    DeviceCapabilities, DeviceDetails, DeviceError, DeviceIdentity, DeviceInput, DeviceKind,
    InputSender, SurfaceDevice, SurfaceFrame,
};

/// Replies kept until the next render reads them; a unit that sends more
/// loses the oldest.
const REPLY_BACKLOG: usize = 64;

type Replies = Arc<Mutex<VecDeque<Reply>>>;
/// What the session is told the time is: `Instant::now`, or a test's clock.
pub(crate) type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

pub(crate) struct Mk3Device {
    profile: &'static ModelProfile,
    unit: FoundUnit,
    backend: Arc<dyn MidiBackend>,
    link: Option<Box<dyn MidiLink>>,
    session: Session,
    leds: LedCache,
    /// Filled by the input callback, read by the next render.
    replies: Replies,
    /// Open while the unit's input counts: what it sends outside Programmer
    /// mode is not a press.
    gate: Arc<AtomicBool>,
    clock: Clock,
}

impl Mk3Device {
    pub(crate) fn new(unit: FoundUnit, backend: Arc<dyn MidiBackend>) -> Self {
        Self {
            profile: unit.profile,
            session: Session::new(unit.profile),
            unit,
            backend,
            link: None,
            leds: LedCache::default(),
            replies: Replies::default(),
            gate: Arc::default(),
            clock: Arc::new(Instant::now),
        }
    }

    /// The same device on a test's clock.
    #[cfg(test)]
    pub(crate) fn with_clock(self, clock: Clock) -> Self {
        Self { clock, ..self }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let link = self.link.as_mut().ok_or(DeviceError::NotConnected)?;
        link.send(bytes)
    }

    fn act(&mut self, action: Action) -> Result<(), DeviceError> {
        match action {
            Action::Send(bytes) => self.send(&bytes),
            Action::Clear => self.clear(),
            Action::Gate(open) => {
                self.gate.store(open, Ordering::Release);
                Ok(())
            }
        }
    }

    /// What the input callback does with each call's bytes.
    fn inbound(&self, input: InputSender) -> OnBytes {
        let mut inbound = Inbound {
            framer: MessageFramer::default(),
            replies: self.replies.clone(),
            gate: self.gate.clone(),
            input,
            device_id: self.unit.id.clone(),
            velocity: self.profile.capabilities.velocity,
        };
        Box::new(move |bytes: &[u8]| inbound.receive(bytes))
    }
}

impl SurfaceDevice for Mk3Device {
    fn identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            id: self.unit.id.clone(),
            name: self.unit.name.clone(),
            model: self.profile.model.into(),
            kind: DeviceKind::Hardware,
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.profile.capabilities
    }

    fn connect(&mut self, input: InputSender) -> Result<(), DeviceError> {
        if self.link.is_some() {
            return Ok(());
        }
        self.gate.store(false, Ordering::Release);
        lock(&self.replies).clear();
        // Input and output together, or neither: the unit's own pair, by
        // name and id, so that identical units are not mixed up.
        let on_bytes = self.inbound(input);
        let link = self
            .backend
            .open(&self.unit.input, &self.unit.output, on_bytes)?;
        self.link = Some(link);
        self.leds.invalidate();
        for action in self.session.opened((self.clock)()) {
            self.act(action)?;
        }
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), DeviceError> {
        for action in self.session.release() {
            // The unit may be gone: what cannot be sent is not sent.
            let _ = self.act(action);
        }
        self.gate.store(false, Ordering::Release);
        if let Some(link) = self.link.take() {
            link.close();
        }
        self.leds.invalidate();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.link.is_some()
    }

    fn render(&mut self, frame: &SurfaceFrame) -> Result<(), DeviceError> {
        if self.link.is_none() {
            return Err(DeviceError::NotConnected);
        }
        let replies: Vec<Reply> = lock(&self.replies).drain(..).collect();
        for action in self.session.tick((self.clock)(), replies) {
            self.act(action)?;
        }
        if let Some(refusal) = self.session.refusal() {
            return Err(refusal);
        }
        if !self.session.draws() {
            return Ok(());
        }
        let plan = self.leds.plan(frame, self.profile.capabilities.animations);
        for message in plan.messages(self.profile.product) {
            self.send(&message)?;
        }
        self.leds.commit(plan);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DeviceError> {
        let plan = LedCache::clear(&self.profile.capabilities.positions());
        for message in plan.messages(self.profile.product) {
            self.send(&message)?;
        }
        self.leds.commit(plan);
        Ok(())
    }

    fn details(&self) -> DeviceDetails {
        self.session.details()
    }
}

/// The input callback's state.
struct Inbound {
    framer: MessageFramer,
    replies: Replies,
    gate: Arc<AtomicBool>,
    input: InputSender,
    device_id: String,
    /// Whether the model reports how hard a pad was hit.
    velocity: bool,
}

impl Inbound {
    fn receive(&mut self, bytes: &[u8]) {
        for message in self.framer.feed(bytes) {
            match parse_reply(&message) {
                Some(reply) => self.queue(reply),
                None => self.forward(&message),
            }
        }
    }

    fn queue(&self, reply: Reply) {
        let mut replies = lock(&self.replies);
        if replies.len() >= REPLY_BACKLOG {
            replies.pop_front();
        }
        replies.push_back(reply);
    }

    fn forward(&self, message: &[u8]) {
        if !self.gate.load(Ordering::Acquire) {
            return;
        }
        if let Some((pos, kind)) = layout::decode(message, self.velocity) {
            let _ = self.input.send(DeviceInput {
                device_id: self.device_id.clone(),
                pos,
                kind,
            });
        }
    }
}

/// The queue even if a thread panicked holding it: replies are plain data.
fn lock(replies: &Replies) -> MutexGuard<'_, VecDeque<Reply>> {
    replies.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ControlPos;
    use crate::devices::novation::testkit::hex;
    use crate::devices::InputKind;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn inbound(velocity: bool) -> (Inbound, UnboundedReceiver<DeviceInput>) {
        let (input, rx) = unbounded_channel();
        let inbound = Inbound {
            framer: MessageFramer::default(),
            replies: Replies::default(),
            gate: Arc::default(),
            input,
            device_id: "launchpad-x:1".into(),
            velocity,
        };
        (inbound, rx)
    }

    fn queued(inbound: &Inbound) -> Vec<Reply> {
        lock(&inbound.replies).iter().cloned().collect()
    }

    #[test]
    fn a_reply_split_over_three_callbacks_is_queued_whole_gate_or_not() {
        let (mut inbound, mut rx) = inbound(true);
        let reply = hex("F0 00 20 29 02 0C 0E 01 F7");
        for part in [&reply[..2], &reply[2..6], &reply[6..]] {
            inbound.receive(part);
        }
        assert_eq!(
            queued(&inbound),
            [Reply::Readback {
                product: 0x0C,
                command: 0x0E,
                data: vec![0x01],
            }]
        );
        assert!(rx.try_recv().is_err(), "a reply is never input");
    }

    #[test]
    fn input_counts_only_while_the_gate_is_open() {
        let (mut inbound, mut rx) = inbound(true);
        inbound.receive(&hex("90 0B 64"));
        assert!(rx.try_recv().is_err(), "closed");
        inbound.gate.store(true, Ordering::Release);
        inbound.receive(&hex("90 0B 64 90 0B 00"));
        let kinds: Vec<InputKind> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|i| {
                assert_eq!(i.pos, ControlPos::grid(0, 7));
                i.kind
            })
            .collect();
        assert_eq!(
            kinds,
            [InputKind::Pressed { velocity: 100 }, InputKind::Released]
        );
    }

    #[test]
    fn a_model_without_velocity_reports_full_presses() {
        let (mut inbound, mut rx) = inbound(false);
        inbound.gate.store(true, Ordering::Release);
        inbound.receive(&hex("90 0B 05"));
        assert_eq!(
            rx.try_recv().unwrap().kind,
            InputKind::Pressed { velocity: 127 }
        );
    }

    #[test]
    fn a_flood_of_replies_keeps_the_newest() {
        let (mut inbound, _rx) = inbound(true);
        for level in 0..=REPLY_BACKLOG as u8 {
            inbound.receive(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x08, level, 0xF7]);
        }
        let kept = queued(&inbound);
        assert_eq!(kept.len(), REPLY_BACKLOG);
        assert_eq!(
            kept[0],
            Reply::Readback {
                product: 0x0C,
                command: 0x08,
                data: vec![1],
            },
            "the first was dropped"
        );
    }
}
