//! Export (plan 7.10): a pure, deterministic function from the workspace, an
//! [`Environment`](crate::artifacts::Environment) and an [`ExportRequest`] to
//! a preview and, when nothing blocks it, the bytes of a Surface file or a
//! Pack. It writes no file and reads no secret, grant, agent state, plugin
//! setting or runtime value: only the chosen surfaces, the context rules
//! that show them, and what the environment says about providers and devices.
//!
//! * `assemble`      [`export`]: the steps in order, and the format's own
//!   checks on the result
//! * `select`        the chosen surfaces and the ids the file gives them
//! * `surfaces`      each surface as the file carries it: legacy actions
//!   stripped, surface references; then, once bound, review items,
//!   credentials, arguments to offer as inputs
//! * `bindings`      the request's input bindings, on the exported
//!   assignments: a bound argument carries the input's default, never its
//!   value on this computer
//! * `credentials`   values shaped like a credential
//! * `rules`         the context rules, their application inputs and extra
//!   identities
//! * `dependencies`  the plugins the content uses (plan 7.6)
//! * `metadata`      the manifest: proposed id, name and version, the file
//!   name, the devices it was tested with
//! * `request`, `preview`  what goes in and what comes out

mod assemble;
mod bindings;
mod credentials;
mod dependencies;
mod metadata;
mod preview;
mod request;
mod rules;
mod select;
mod surfaces;

#[cfg(test)]
mod tests;

pub use assemble::export;
pub use preview::{
    ExportCode, ExportIssue, ExportPreview, ExportResult, ExportedRule, ExportedSurface, Parameter,
    Stripped, SuggestedType, Suggestion,
};
pub use request::{ExportMetadata, ExportRequest, ExtraIdentity, InputBinding, RuleInput};
