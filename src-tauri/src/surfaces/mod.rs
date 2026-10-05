//! Starter surfaces. These ship with the app and explain the product at a glance.
//!
//! Safe by default: shell commands only `echo`/sleep, URLs are public pages,
//! and HTTP actions point at obvious placeholders the user must edit.

mod accessibility;
pub mod agent_control;
mod build;
mod development;
mod navigation;
mod palette;
mod productivity;
mod smart_home;
mod upgrade;
mod workspace;

// The system a starter workspace is built for, kept at this path for its callers.
pub use crate::platform::Platform;
pub use upgrade::add_agent_control;
pub use workspace::{blank_surface, default_workspace, default_workspace_for, starter_surfaces};

#[cfg(test)]
mod tests;
