//! The suite's view of the wire: a `MidiBackend` over the system's that
//! gives `Mk3Device` its link as usual, keeps a timed transcript of every
//! message both ways, and lets the suite send on the same link for its own
//! probes. Only one link is open at a time, the device's or the suite's
//! alone, so that a system with exclusive ports (Windows) works too. While a
//! probe waits for its answer, the unit's SysEx replies are kept from the
//! device, whose session must never see answers it did not ask for.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::devices::midi::{MessageFramer, MidiBackend, MidiLink, OnBytes, PortInfo, PortSnapshot};
use crate::devices::DeviceError;

/// Messages kept for the suite to read; a unit that sends more loses the
/// oldest.
const INBOX_MAX: usize = 4096;
const SYSEX_START: u8 = 0xF0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dir {
    Sent,
    Received,
}

/// One entry of the transcript: what went which way, and when after the
/// suite started.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub at: Duration,
    pub dir: Dir,
    pub bytes: Vec<u8>,
}

/// A whole message from the unit, and when it arrived.
#[derive(Debug, Clone)]
pub(super) struct Message {
    pub at: Instant,
    pub bytes: Vec<u8>,
}

impl Message {
    /// A note, a CC or another channel message: what a press sends.
    pub(super) fn is_channel(&self) -> bool {
        self.bytes
            .first()
            .is_some_and(|&status| (0x80..0xF0).contains(&status))
    }

    pub(super) fn is_sysex(&self) -> bool {
        self.bytes.first() == Some(&SYSEX_START)
    }
}

pub(super) struct Tap {
    backend: Arc<dyn MidiBackend>,
    started: Instant,
    link: Mutex<Option<Box<dyn MidiLink>>>,
    log: Mutex<Vec<Entry>>,
    inbox: Mutex<VecDeque<Message>>,
    arrived: Condvar,
    /// A probe is waiting: SysEx from the unit is the suite's alone.
    probing: AtomicBool,
}

impl Tap {
    pub(super) fn new(backend: Arc<dyn MidiBackend>) -> Arc<Tap> {
        Arc::new(Tap {
            backend,
            started: Instant::now(),
            link: Mutex::default(),
            log: Mutex::default(),
            inbox: Mutex::default(),
            arrived: Condvar::new(),
            probing: AtomicBool::new(false),
        })
    }

    pub(super) fn ports(&self) -> PortSnapshot {
        self.backend.ports()
    }

    /// Opens the pair for the suite alone: no device reads what arrives.
    pub(super) fn open(
        self: &Arc<Self>,
        input: &PortInfo,
        output: &PortInfo,
    ) -> Result<(), DeviceError> {
        self.open_for(input, output, None)
    }

    /// Opens the pair, closing any link open before. What arrives goes to
    /// the transcript and the inbox, and to `device` unless a probe waits
    /// for it.
    fn open_for(
        self: &Arc<Self>,
        input: &PortInfo,
        output: &PortInfo,
        mut device: Option<OnBytes>,
    ) -> Result<(), DeviceError> {
        self.close();
        let tap = Arc::downgrade(self);
        let mut framer = MessageFramer::default();
        let on_bytes: OnBytes = Box::new(move |bytes: &[u8]| {
            let Some(tap) = tap.upgrade() else {
                return;
            };
            tap.record(Dir::Received, bytes);
            for message in framer.feed(bytes) {
                let hidden =
                    message.first() == Some(&SYSEX_START) && tap.probing.load(Ordering::Acquire);
                if let Some(device) = device.as_mut().filter(|_| !hidden) {
                    device(&message);
                }
                tap.deliver(message);
            }
        });
        let link = self.backend.open(input, output, on_bytes)?;
        *lock(&self.link) = Some(link);
        Ok(())
    }

    pub(super) fn close(&self) {
        let link = lock(&self.link).take();
        if let Some(link) = link {
            link.close();
        }
    }

    pub(super) fn send(&self, bytes: &[u8]) -> Result<(), DeviceError> {
        let mut link = lock(&self.link);
        let link = link.as_mut().ok_or(DeviceError::NotConnected)?;
        self.record(Dir::Sent, bytes);
        link.send(bytes)
    }

    pub(super) fn send_all(&self, messages: &[Vec<u8>]) -> Result<(), DeviceError> {
        messages.iter().try_for_each(|m| self.send(m))
    }

    /// Sends `request` and waits up to `timeout` for the first message that
    /// `answers` it: the message and how long it took, or `None`.
    pub(super) fn probe(
        &self,
        request: &[u8],
        answers: impl Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Result<Option<(Vec<u8>, Duration)>, DeviceError> {
        self.probe_after(&[], request, answers, timeout)
    }

    /// Sends `before`, then `request`, and times from the first byte sent
    /// to the answer: the unit answers after it processed everything sent
    /// before the request (a readback barrier).
    pub(super) fn probe_after(
        &self,
        before: &[Vec<u8>],
        request: &[u8],
        answers: impl Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Result<Option<(Vec<u8>, Duration)>, DeviceError> {
        self.probing.store(true, Ordering::Release);
        self.clear_inbox();
        let began = Instant::now();
        let sent = self.send_all(before).and_then(|()| self.send(request));
        let answer = sent.map(|()| self.wait_for(answers, began + timeout));
        self.probing.store(false, Ordering::Release);
        Ok(answer?.map(|m| (m.bytes, m.at.saturating_duration_since(began))))
    }

    /// The first message `wanted` takes, waiting until `deadline`; the
    /// messages before it are dropped.
    pub(super) fn wait_for(
        &self,
        wanted: impl Fn(&[u8]) -> bool,
        deadline: Instant,
    ) -> Option<Message> {
        let mut inbox = lock(&self.inbox);
        loop {
            while let Some(message) = inbox.pop_front() {
                if wanted(&message.bytes) {
                    return Some(message);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            inbox = self
                .arrived
                .wait_timeout(inbox, deadline - now)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|p| p.into_inner().0);
        }
    }

    /// Every message that arrived since the inbox was last read.
    pub(super) fn take_inbox(&self) -> Vec<Message> {
        lock(&self.inbox).drain(..).collect()
    }

    pub(super) fn clear_inbox(&self) {
        lock(&self.inbox).clear();
    }

    /// Where the transcript is now, for `since`.
    pub(super) fn mark(&self) -> usize {
        lock(&self.log).len()
    }

    pub(super) fn since(&self, mark: usize) -> Vec<Entry> {
        lock(&self.log).get(mark..).unwrap_or_default().to_vec()
    }

    fn record(&self, dir: Dir, bytes: &[u8]) {
        let entry = Entry {
            at: self.started.elapsed(),
            dir,
            bytes: bytes.to_vec(),
        };
        lock(&self.log).push(entry);
    }

    fn deliver(&self, bytes: Vec<u8>) {
        let mut inbox = lock(&self.inbox);
        if inbox.len() >= INBOX_MAX {
            inbox.pop_front();
        }
        inbox.push_back(Message {
            at: Instant::now(),
            bytes,
        });
        self.arrived.notify_all();
    }
}

/// The system's backend as `Mk3Device` sees it, through the tap.
pub(super) struct TapBackend(pub Arc<Tap>);

impl MidiBackend for TapBackend {
    fn ports(&self) -> PortSnapshot {
        self.0.ports()
    }

    fn open(
        &self,
        input: &PortInfo,
        output: &PortInfo,
        on_bytes: OnBytes,
    ) -> Result<Box<dyn MidiLink>, DeviceError> {
        self.0.open_for(input, output, Some(on_bytes))?;
        Ok(Box::new(TapLink(self.0.clone())))
    }
}

struct TapLink(Arc<Tap>);

impl MidiLink for TapLink {
    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        self.0.send(bytes)
    }

    fn close(self: Box<Self>) {
        self.0.close();
    }
}

/// The value even if a thread panicked holding it: it is plain data.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
