//! Novation Launchpad MK3 models. Only this directory knows their MIDI bytes;
//! it reaches the ports through `devices::midi`. `protocol` and `layout` are
//! the codec both the Mini MK3 and the X speak (the two manuals differ only in
//! the product byte for everything they hold); a model adds its own data as a
//! `profile::ModelProfile`, and `ports` finds each model's units on the
//! system's ports. `NovationHardware` is what the device manager sees.

mod hardware;
mod launchpad_mini_mk3;
mod launchpad_x;
#[cfg(test)]
mod launchpad_x_tests;
mod layout;
mod ports;
#[cfg(test)]
mod ports_tests;
// The product byte, inquiry families and Live layout are read from M5 on.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod profile;
// The Mini MK3 packages (M3-M8) put the rest of the codec to use.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod protocol;
#[cfg(test)]
mod protocol_tests;

pub(crate) use hardware::NovationHardware;
