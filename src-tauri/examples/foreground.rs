//! Prints which application is in front, as LaunchDeck's observer sees it.
//! A way to check what identity an application has on this system before
//! writing a rule for it, and whether the observer works here at all.
//!
//! `cargo run --example foreground`

use std::time::Instant;

use launchdeck_lib::context::observer::{self, Report, Sink};
use launchdeck_lib::context::{ObserverStatus, Seen};

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
    observer.run(sink);
}
