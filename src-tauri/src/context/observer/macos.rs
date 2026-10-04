//! macOS: `NSWorkspace` says which application is active.
//!
//! The workspace posts `NSWorkspaceDidActivateApplicationNotification` each
//! time another application comes to the front. The handler reads
//! `frontmostApplication` and takes its bundle identifier, its localized name
//! and its process id (to recognise LaunchDeck itself). No window is looked
//! at, so no Screen Recording or Accessibility permission is involved.
//!
//! A read every two seconds stands behind the notifications, so that one that
//! was missed corrects itself.
//!
//! **Not compiled or run where this was written** (a Linux machine without the
//! macOS Rust target). It is written against objc2-app-kit 0.3.2,
//! objc2-foundation 0.3.2 and block2 0.6.2 as vendored.

use std::ptr::NonNull;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
use objc2_foundation::NSNotification;

use super::{ForegroundObserver, ObserverStatus, Seen, Sink};
use crate::context::identity::from_bundle;

const SAFETY_NET: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct MacObserver;

impl MacObserver {
    pub fn new() -> MacObserver {
        MacObserver
    }
}

/// The application in front, by its bundle identifier.
fn frontmost() -> Seen {
    autoreleasepool(|_| {
        let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return Seen::Nothing;
        };
        let bundle = app
            .bundleIdentifier()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let name = app
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_default();
        match from_bundle(&bundle, &name) {
            Some(identity) => Seen::Application {
                identity,
                own: app.processIdentifier() as i64 == std::process::id() as i64,
            },
            // A process without a bundle has nothing stable to be known by.
            None => Seen::Nothing,
        }
    })
}

impl ForegroundObserver for MacObserver {
    fn status(&self) -> ObserverStatus {
        ObserverStatus::available("macOS application notifications")
    }

    fn run(self: Box<Self>, sink: Sink) {
        sink.seen(frontmost());
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let notify = sink.clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| notify.seen(frontmost()));
        // SAFETY: the name is AppKit's own constant, no object filter and no
        // queue are given, and the block captures only a `Sink`, which is
        // `Send + Sync`. The token is kept for as long as this thread lives.
        let _token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &block,
            )
        };
        while !sink.stopped() {
            std::thread::sleep(SAFETY_NET);
            sink.seen(frontmost());
        }
    }
}
