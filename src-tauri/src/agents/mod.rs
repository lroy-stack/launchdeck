//! Agent Control: a provider-neutral module for observing, intervening in and
//! orchestrating autonomous agents from a physical control surface.
//!
//! * `model`    domain types, normalized statuses, capabilities, availability
//! * `visual`   the single status/command to appearance mapping
//! * `adapter`  the Agent Adapter SDK (trait + host)
//! * `adapters` real-harness adapters (the only place that names a vendor)
//! * `service`  `AgentControl`: registry, sessions, approvals, slots, commands
//! * `view`     read models for the desktop UI
//! * `bridge`   the Agent Bridge: the adapter contract over a localhost WebSocket
//! * `audit`, `settings`, `notify`  lightweight persistence and policy
//! * `pack`     the Agent Pack manifest (declarative; nothing loads it yet)
//!
//! Nothing in this module knows MIDI, pads or LEDs. Surfaces bind to the
//! projection events that `service` publishes.

pub mod adapter;
pub mod adapters;
pub mod audit;
pub mod bridge;
pub mod demo;
pub mod model;
pub mod notify;
pub mod pack;
pub mod plugin;
pub mod service;
pub mod settings;
pub mod view;
pub mod visual;

#[cfg(test)]
mod tests;
