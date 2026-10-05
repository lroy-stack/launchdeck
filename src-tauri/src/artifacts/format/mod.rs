//! The v1 types of a portable file and their structural checks (plan
//! 7.3-7.9). Every type refuses unknown members and omits a member equal
//! to its default when written; none of them is a workspace type, except
//! the feedback rule, which is the workspace's own (`state::feedback`).
//!
//! * `envelope`   the two kinds, a whole file as written
//! * `manifest`   the manifest, dependencies, requirements, inputs; `check_manifest`
//! * `content`    surfaces, pages, controls, context rules; `check_content`
//! * `cross`      kind rules, requirements, declarations, references; `check_cross`
//! * `convert`    a workspace surface as a file carries it, and back
//! * `rejection`  why a file was refused, and how a message repeats a value
//! * `text`       the text rules and id grammars
//!
//! The checks run in the inspect order: manifest, then (after the digest)
//! content, then the cross-checks. Each returns the first problem it finds.

mod content;
mod convert;
mod cross;
mod envelope;
mod manifest;
mod rejection;
mod text;

#[cfg(test)]
mod tests;

pub use content::{
    check_content, AnimationV1, AppRefV1, AssignmentV1, Color, ContentV1, ContextRuleV1, ControlV1,
    InputRef, PageV1, Pos, Span, SurfaceV1, VisualV1,
};
pub(in crate::artifacts) use cross::assignments;
pub use cross::{check_cross, compute_requirements};
pub use envelope::{ArtifactKind, ArtifactV1};
pub(in crate::artifacts) use manifest::web_address;
pub use manifest::{
    check_manifest, Author, AuxRequirement, CreatedWith, DependencyV1, GridRequirement, InputDef,
    InputOption, InputType, InputValue, Integrity, ManifestV1, Requirements, TestedKind,
    TestedWith, CREATED_WITH_APP,
};
/// The format's own direction controls, which the review's hidden set must
/// include (a test holds it).
#[cfg(test)]
pub(in crate::artifacts) use rejection::is_bidi;
pub(in crate::artifacts) use rejection::{echo, plain_message};
pub use rejection::{Rejection, RejectionCode};
