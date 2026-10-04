//! State and feedback.
//!
//! ```text
//!   providers ──► StateHub (current values only) ──► evaluate(rules) ──► look ──► renderer
//!   events    ──► overlay (temporary, with an expiry) ────────────────────────────┘
//! ```
//!
//! An **event** is something that happened; the bus carries it and forgets it.
//! A **state** is how things are now; the hub holds exactly that. A control's
//! look is a pure function of its rules and the hub, so it is right the moment
//! the control exists, with no event in flight.
//!
//! * [`value`]: the scalars a state can hold.
//! * [`address`]: where a state lives (provider, definition, scope).
//! * [`catalog`]: everything a control can observe, in one list, with the values.
//! * [`definition`]: what a provider says it offers.
//! * [`hub`]: the current values, the provider lifecycle and the invalidation.
//! * [`feedback`]: the rules a control carries, and the gate they pass.
//! * [`eval`]: the pure evaluator and its explanation.
//! * [`lint`]: what can be proved about a list of rules without any value.
//! * [`index`]: which controls depend on which state or event.

pub mod address;
pub mod catalog;
pub mod definition;
pub mod eval;
pub mod feedback;
pub mod hub;
pub mod index;
pub mod lint;
pub mod value;

pub use address::{StateAddress, StateKey};
pub use definition::{ObservableDefinition, ObservableKind, Role, ValueType};
pub use hub::{Changes, Observed, StateHub, Status};
pub use value::Scalar;
