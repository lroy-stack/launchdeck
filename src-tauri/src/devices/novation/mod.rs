//! Novation Launchpad MK3 models. Only this directory knows their MIDI bytes;
//! it reaches the ports through `devices::midi`. `protocol` and `layout` are
//! the codec both the Mini MK3 and the X speak (the two manuals differ only in
//! the product byte for everything they hold); a model adds its own data.

pub(crate) mod launchpad_x;
#[cfg(test)]
mod launchpad_x_tests;
mod layout;
// The Mini MK3 packages (M3-M8) put the rest of the codec to use.
#[cfg_attr(not(test), expect(dead_code, reason = "used by later packages"))]
mod protocol;
#[cfg(test)]
mod protocol_tests;
