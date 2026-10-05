//! Novation Launchpad models. Only this directory knows their MIDI bytes; it
//! reaches the ports through `devices::midi`.

pub(crate) mod launchpad_x;
#[cfg(test)]
mod launchpad_x_tests;
