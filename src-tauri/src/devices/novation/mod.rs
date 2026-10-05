//! Novation Launchpad MK3 models. Only this directory knows their MIDI bytes;
//! it reaches the ports through `devices::midi`. `protocol` and `layout` are
//! the codec both the Mini MK3 and the X speak (the two manuals differ only in
//! the product byte for everything they hold); a model adds its own data as a
//! `profile::ModelProfile`, and `ports` finds each model's units on the
//! system's ports. One `device::Mk3Device` drives a unit of any model: the
//! `session` says what to send to take and give back the unit, `settings`
//! what brightness and scrolling text send, `leds` what a frame needs sent.
//! `NovationHardware` is what the device manager sees; `virtual_model` is a
//! software unit of a model, for the virtual device.

mod device;
#[cfg(test)]
mod device_tests;
mod hardware;
mod launchpad_mini_mk3;
mod launchpad_x;
mod layout;
mod leds;
mod palette;
mod ports;
#[cfg(test)]
mod ports_tests;
mod profile;
// The Mini MK3 packages (M3-M8) put the rest of the codec to use.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod protocol;
#[cfg(test)]
mod protocol_tests;
mod session;
#[cfg(test)]
mod session_tests;
mod settings;
#[cfg(test)]
mod testkit;

pub(crate) use hardware::NovationHardware;
#[cfg(test)]
pub(crate) use launchpad_mini_mk3::PROFILE as LAUNCHPAD_MINI_MK3;
#[cfg(test)]
pub(crate) use palette::{nearest, palette_rgb};
#[cfg_attr(
    not(test),
    expect(unused_imports, reason = "the app registers no virtual Mini yet")
)]
pub(crate) use profile::virtual_model;
#[cfg(test)]
pub(crate) use protocol::to_7bit;
#[cfg(test)]
pub(crate) use testkit::{SimLaunchpad, SimLed};
