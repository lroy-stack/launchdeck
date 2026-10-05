//! Portable artifacts: a Surface file (`.ldsurface`, one surface) and a Pack
//! (`.ldpack`, several surfaces, context rules and typed inputs), each one
//! UTF-8 JSON document that can be shared and imported.
//!
//! The format is public and independent of the workspace schema: its own
//! types, converted to and from `core::model`, never the workspace types
//! written out. The specification of record is `PRODUCTIZATION_PLAN.md`
//! section 7.
//!
//! * `limits`  every limit of the format, and [`ArtifactLimits`] for the UI
//! * `json`    the pre-pass over the raw text, the content digest, the
//!   canonical writer
//! * `format`  the v1 types, their conversion to and from the workspace
//!   model, and their structural checks
//! * `env`     what export and import know about this installation, as
//!   plain data ([`Environment`])
//! * `review`  the actions that run things, which a person reviews
//! * `export`  a workspace's surfaces as a Surface file or a Pack, with a
//!   preview of what is written and what is left out
//! * `inspect` the bytes of a file, treated as hostile, read in the order of
//!   plan 7.3 into a [`ParsedArtifact`] or refused with a [`Rejection`]

mod env;
mod export;
mod format;
mod inspect;
mod json;
mod limits;
mod review;

#[cfg(test)]
mod testkit;

pub use env::{Environment, KnownAction, KnownDevice, KnownProvider};
pub use export::{
    export, ExportCode, ExportIssue, ExportMetadata, ExportPreview, ExportRequest, ExportResult,
    ExportedRule, ExportedSurface, ExtraIdentity, InputBinding, Parameter, RuleInput, Stripped,
    SuggestedType, Suggestion,
};
pub use format::*;
pub use inspect::{inspect, InspectWarning, InspectWarningCode, ParsedArtifact};
pub use json::{
    content_digest, prepass, to_canonical, to_canonical_members, JsonProblem, PrepassError,
};
pub use limits::ArtifactLimits;
pub use review::{classify, Location, ReviewItem, ReviewKind, Slot};

/// `format` of a Surface file.
pub const FORMAT_SURFACE: &str = "launchdeck.surface";
/// `format` of a Pack.
pub const FORMAT_PACK: &str = "launchdeck.pack";
/// The one format version this build reads and writes, shared by both kinds
/// and independent of every other version number.
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION_SURFACE: &str = "ldsurface";
pub const EXTENSION_PACK: &str = "ldpack";
pub const MEDIA_TYPE_SURFACE: &str = "application/vnd.launchdeck.surface+json";
pub const MEDIA_TYPE_PACK: &str = "application/vnd.launchdeck.pack+json";
