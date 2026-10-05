//! Novation Launchpad MK3 models. Only this directory knows their MIDI bytes;
//! it reaches the ports through `devices::midi`. `protocol` and `layout` are
//! the codec both the Mini MK3 and the X speak (the two manuals differ only in
//! the product byte for everything they hold); a model adds its own data as a
//! `profile::ModelProfile`, and `ports` finds each model's units on the
//! system's ports.

// The port parser and the profiles are put to use by the next commits.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod launchpad_mini_mk3;
pub(crate) mod launchpad_x;
#[cfg(test)]
mod launchpad_x_tests;
mod layout;
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod ports;
#[cfg(test)]
mod ports_tests;
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod profile;
// The Mini MK3 packages (M3-M8) put the rest of the codec to use.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod protocol;
#[cfg(test)]
mod protocol_tests;
