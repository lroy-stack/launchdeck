//! Prints which application is in front, as LaunchDeck's observer sees it.
//! A way to check what identity an application has on this system before
//! writing a rule for it, and whether the observer works here at all.
//!
//! `cargo run --example foreground`
//!
//! On macOS the activation notifications arrive through the main thread's run
//! loop, which the app's window keeps running. Here the observer gets a thread
//! of its own and the main thread runs that loop.

use std::time::Instant;

use launchdeck_lib::context::observer::{self, Report, Sink};
use launchdeck_lib::context::{ForegroundObserver, ObserverStatus, Seen};

fn main() {
    let observer = observer::native();
    match observer.status() {
        ObserverStatus::Unavailable { reason } => {
            println!("unavailable: {reason}");
            return;
        }
        status => println!("{status:?}"),
    }
    let started = Instant::now();
    let sink = Sink::new(move |report| {
        let at = started.elapsed().as_millis();
        match report {
            Report::Seen(Seen::Application { identity, own }) => println!(
                "{at:>7} ms  {}:{}  \"{}\"{}",
                identity.namespace,
                identity.id,
                identity.display_name,
                if own { "  (this program)" } else { "" }
            ),
            Report::Seen(Seen::Nothing) => println!("{at:>7} ms  nothing in front"),
            Report::Status(status) => println!("{at:>7} ms  {status:?}"),
        }
    });
    run(observer, sink);
}

#[cfg(target_os = "macos")]
fn run(observer: Box<dyn ForegroundObserver>, sink: Sink) {
    use std::time::Duration;

    use objc2_foundation::{NSDate, NSRunLoop};

    std::thread::spawn(move || observer.run(sink));
    let main = NSRunLoop::mainRunLoop();
    loop {
        let turn = Instant::now();
        main.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(1.0));
        // A run loop with nothing attached returns at once; do not spin.
        if turn.elapsed() < Duration::from_millis(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn run(observer: Box<dyn ForegroundObserver>, sink: Sink) {
    observer.run(sink);
}
