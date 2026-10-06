//! Every limit of a portable file (plan section 7.18), in one place, and the
//! same numbers as [`ArtifactLimits`] for the UI.
//!
//! A limit the workspace editor already enforces is taken from its owner by
//! reference. Where the owner keeps it private (the controls on a page, the
//! length of an application id or namespace), the number is written here and
//! a test proves the owner refuses one more.

use serde::Serialize;

use crate::actions::params;
use crate::context::{portable, rules};
use crate::core::validate::MAX_GRID_DIM;
use crate::state::feedback;

// The file and its JSON.

/// Bytes in one file, counted before anything is parsed.
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
/// Arrays and objects nested in one another.
pub const MAX_DEPTH: usize = 32;
/// Bytes in one string, after unescaping.
pub const MAX_STRING_BYTES: usize = 64 * 1024;
/// Bytes in one member name, after unescaping.
pub const MAX_KEY_BYTES: usize = 128;
/// Embedded files. Format 1 has none.
pub const MAX_ASSETS: usize = 0;

// Content.

/// Surfaces in a Surface file.
pub const SURFACE_FILE_SURFACES: usize = 1;
/// Surfaces in a Pack.
pub const MAX_PACK_SURFACES: usize = 16;
/// Pages in one surface.
pub const MAX_PAGES: usize = 32;
/// Controls on one page (the editor's own limit).
pub const MAX_CONTROLS_PER_PAGE: usize = 128;
/// Controls in one file.
pub const MAX_CONTROLS: usize = 2048;
/// Looks of one control.
pub const MAX_LOOKS: usize = 32;
/// Characters in a look name.
pub const MAX_LOOK_NAME: usize = feedback::MAX_LOOK_NAME;
/// Feedback rules of one control.
pub const MAX_FEEDBACK_RULES: usize = feedback::MAX_RULES;
/// Payload conditions of one event rule.
pub const MAX_WHERE: usize = feedback::MAX_WHERE;
pub const MIN_HOLD_MS: u32 = feedback::MIN_HOLD_MS;
pub const MAX_HOLD_MS: u32 = feedback::MAX_HOLD_MS;
/// Grid columns and rows; a coordinate is below it.
pub const MAX_GRID: u8 = MAX_GRID_DIM;
/// Cells a control spans in either direction.
pub const MAX_SPAN: u8 = 8;
/// Pads above or beside the grid.
pub const MAX_AUX: u8 = MAX_GRID_DIM;
/// Context rules in one file, and in a workspace after import.
pub const MAX_CONTEXT_RULES: usize = rules::MAX_RULES;
/// Identities of one application.
pub const MAX_IDENTITIES: usize = portable::MAX_IDENTITIES;
/// Characters in an application's name.
pub const MAX_APPLICATION_NAME: usize = portable::MAX_NAME;
/// Characters in an application id (the identity's own limit).
pub const MAX_IDENTITY_ID: usize = 255;
/// Bytes in an identity namespace (the identity's own limit).
pub const MAX_NAMESPACE: usize = 16;

// Assignments.

/// Members of one assignment's `args`.
pub const MAX_ARG_KEYS: usize = params::MAX_PARAMS;
/// Objects and arrays nested in `args`, `args` itself included.
pub const MAX_ARG_DEPTH: usize = 4;
/// Bytes of one assignment's `args`, written compactly.
pub const MAX_ARG_BYTES: usize = 32 * 1024;
/// Characters in one text of `args`.
pub const MAX_ARG_TEXT: usize = params::MAX_TEXT_VALUE;
/// Input bindings in one file: assignment arguments and context rules bound
/// to an input.
pub const MAX_BINDINGS: usize = 256;

// Manifest.

pub const MAX_NAME: usize = 60;
pub const MAX_DESCRIPTION: usize = 1000;
pub const MAX_AUTHOR_NAME: usize = 64;
pub const MAX_AUTHOR_URL: usize = 256;
pub const MAX_LICENSE: usize = 64;
/// Characters in a SemVer version.
pub const MAX_VERSION: usize = 32;
/// Characters in a version requirement.
pub const MAX_REQUIREMENT: usize = 64;
/// Comparators in a version requirement.
pub const MAX_COMPARATORS: usize = 4;
pub const MAX_DEPENDENCIES: usize = 32;
/// Characters in a dependency's display name.
pub const MAX_DEPENDENCY_NAME: usize = 64;
pub const MAX_TESTED_WITH: usize = 8;
/// Characters in a device model of `testedWith`.
pub const MAX_MODEL: usize = 32;

// Inputs.

pub const MAX_INPUTS: usize = 32;
/// Characters in an input id.
pub const MAX_INPUT_ID: usize = 32;
pub const MAX_INPUT_LABEL: usize = 40;
pub const MAX_INPUT_DESCRIPTION: usize = 280;
pub const MAX_ENUM_OPTIONS: usize = 32;
/// Characters in an enum option's value.
pub const MAX_OPTION_VALUE: usize = 64;
/// The largest `maxLength` of a string input.
pub const MAX_STRING_INPUT: usize = 2048;
/// A string input's `maxLength` when it does not say.
pub const DEFAULT_STRING_INPUT: usize = 256;
pub const MAX_PATH: usize = 1024;
pub const MAX_URL: usize = 2048;

// Names and ids.

/// Characters in an id: surfaces, pages, controls, providers and actions.
pub const MAX_ID: usize = 64;
/// Characters in a surface or page name, a control label, an option label.
pub const MAX_LABEL: usize = 40;
pub const MAX_SURFACE_DESCRIPTION: usize = 400;
pub const MAX_CONTROL_DESCRIPTION: usize = 400;
/// Characters in an icon.
pub const MAX_ICON: usize = 8;
/// Characters in a surface category.
pub const MAX_CATEGORY: usize = 32;

/// Surfaces in a workspace after an import.
pub const MAX_WORKSPACE_SURFACES: usize = 64;

/// The limits as the UI reads them, so it shows them without copying them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactLimits {
    pub file_bytes: usize,
    pub json_depth: usize,
    pub string_bytes: usize,
    pub key_bytes: usize,
    pub assets: usize,
    pub surface_file_surfaces: usize,
    pub pack_surfaces: usize,
    pub pages: usize,
    pub controls_per_page: usize,
    pub controls: usize,
    pub looks: usize,
    pub look_name: usize,
    pub feedback_rules: usize,
    pub where_conditions: usize,
    pub min_hold_ms: u32,
    pub max_hold_ms: u32,
    pub grid: u8,
    pub span: u8,
    pub aux: u8,
    pub context_rules: usize,
    pub identities: usize,
    pub application_name: usize,
    pub identity_id: usize,
    pub namespace: usize,
    pub arg_keys: usize,
    pub arg_depth: usize,
    pub arg_bytes: usize,
    pub arg_text: usize,
    pub bindings: usize,
    pub name: usize,
    pub description: usize,
    pub author_name: usize,
    pub author_url: usize,
    pub license: usize,
    pub version: usize,
    pub requirement: usize,
    pub comparators: usize,
    pub dependencies: usize,
    pub dependency_name: usize,
    pub tested_with: usize,
    pub model: usize,
    pub inputs: usize,
    pub input_id: usize,
    pub input_label: usize,
    pub input_description: usize,
    pub enum_options: usize,
    pub option_value: usize,
    pub string_input: usize,
    pub default_string_input: usize,
    pub path: usize,
    pub url: usize,
    pub id: usize,
    pub label: usize,
    pub surface_description: usize,
    pub control_description: usize,
    pub icon: usize,
    pub category: usize,
    pub workspace_surfaces: usize,
}

impl ArtifactLimits {
    pub const fn new() -> Self {
        Self {
            file_bytes: MAX_FILE_BYTES,
            json_depth: MAX_DEPTH,
            string_bytes: MAX_STRING_BYTES,
            key_bytes: MAX_KEY_BYTES,
            assets: MAX_ASSETS,
            surface_file_surfaces: SURFACE_FILE_SURFACES,
            pack_surfaces: MAX_PACK_SURFACES,
            pages: MAX_PAGES,
            controls_per_page: MAX_CONTROLS_PER_PAGE,
            controls: MAX_CONTROLS,
            looks: MAX_LOOKS,
            look_name: MAX_LOOK_NAME,
            feedback_rules: MAX_FEEDBACK_RULES,
            where_conditions: MAX_WHERE,
            min_hold_ms: MIN_HOLD_MS,
            max_hold_ms: MAX_HOLD_MS,
            grid: MAX_GRID,
            span: MAX_SPAN,
            aux: MAX_AUX,
            context_rules: MAX_CONTEXT_RULES,
            identities: MAX_IDENTITIES,
            application_name: MAX_APPLICATION_NAME,
            identity_id: MAX_IDENTITY_ID,
            namespace: MAX_NAMESPACE,
            arg_keys: MAX_ARG_KEYS,
            arg_depth: MAX_ARG_DEPTH,
            arg_bytes: MAX_ARG_BYTES,
            arg_text: MAX_ARG_TEXT,
            bindings: MAX_BINDINGS,
            name: MAX_NAME,
            description: MAX_DESCRIPTION,
            author_name: MAX_AUTHOR_NAME,
            author_url: MAX_AUTHOR_URL,
            license: MAX_LICENSE,
            version: MAX_VERSION,
            requirement: MAX_REQUIREMENT,
            comparators: MAX_COMPARATORS,
            dependencies: MAX_DEPENDENCIES,
            dependency_name: MAX_DEPENDENCY_NAME,
            tested_with: MAX_TESTED_WITH,
            model: MAX_MODEL,
            inputs: MAX_INPUTS,
            input_id: MAX_INPUT_ID,
            input_label: MAX_INPUT_LABEL,
            input_description: MAX_INPUT_DESCRIPTION,
            enum_options: MAX_ENUM_OPTIONS,
            option_value: MAX_OPTION_VALUE,
            string_input: MAX_STRING_INPUT,
            default_string_input: DEFAULT_STRING_INPUT,
            path: MAX_PATH,
            url: MAX_URL,
            id: MAX_ID,
            label: MAX_LABEL,
            surface_description: MAX_SURFACE_DESCRIPTION,
            control_description: MAX_CONTROL_DESCRIPTION,
            icon: MAX_ICON,
            category: MAX_CATEGORY,
            workspace_surfaces: MAX_WORKSPACE_SURFACES,
        }
    }
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::identity::ApplicationIdentity;
    use crate::core::model::{Control, ControlPos, Page, Rgb, Span};
    use crate::core::validate::validate_control;

    fn pad(col: u8, row: u8) -> Control {
        Control::new(ControlPos::grid(col, row), "Pad", Rgb::new(1, 1, 1))
    }

    /// The numbers written here agree with the editor that keeps its own.
    #[test]
    fn the_editor_refuses_one_more_than_the_format_allows() {
        let full = |n: usize| Page {
            id: "p".into(),
            name: "P".into(),
            controls: (0..n)
                .map(|i| {
                    let mut c = pad(0, 0);
                    c.id = format!("c{i}");
                    c.pos = ControlPos::Top { index: 0 };
                    c
                })
                .collect(),
        };
        let mut one_more = pad(5, 5);
        one_more.id = "new".into();
        assert!(validate_control(&full(MAX_CONTROLS_PER_PAGE - 1), &one_more).is_ok());
        assert_eq!(
            validate_control(&full(MAX_CONTROLS_PER_PAGE), &one_more).unwrap_err(),
            "A page can hold at most 128 pads."
        );

        let empty = full(0);
        let mut c = pad(0, 0);
        c.label = "x".repeat(MAX_LABEL);
        assert!(validate_control(&empty, &c).is_ok());
        c.label.push('x');
        assert!(validate_control(&empty, &c).is_err());
        let mut c = pad(0, 0);
        c.description = "x".repeat(MAX_CONTROL_DESCRIPTION + 1);
        assert!(validate_control(&empty, &c).is_err());
        let mut c = pad(0, 0);
        c.id = "x".repeat(MAX_ID + 1);
        assert!(validate_control(&empty, &c).is_err());
        let mut c = pad(0, 0);
        c.span = Span {
            w: MAX_SPAN + 1,
            h: 1,
        };
        assert!(validate_control(&empty, &c).is_err());
        assert!(validate_control(&empty, &pad(MAX_GRID - 1, 0)).is_ok());
        assert!(validate_control(&empty, &pad(MAX_GRID, 0)).is_err());

        let identity = |ns: &str, id: &str| ApplicationIdentity {
            namespace: ns.into(),
            id: id.into(),
            display_name: String::new(),
        };
        let longest = "a".repeat(MAX_NAMESPACE);
        assert!(identity(&longest, "app").validate().is_ok());
        assert!(identity(&format!("{longest}a"), "app").validate().is_err());
        assert!(identity("x11", &"a".repeat(MAX_IDENTITY_ID))
            .validate()
            .is_ok());
        assert!(identity("x11", &"a".repeat(MAX_IDENTITY_ID + 1))
            .validate()
            .is_err());
    }

    #[test]
    fn the_ui_reads_the_same_numbers() {
        let limits = serde_json::to_value(ArtifactLimits::default()).unwrap();
        assert_eq!(limits["fileBytes"], 2 * 1024 * 1024);
        assert_eq!(limits["jsonDepth"], 32);
        assert_eq!(limits["packSurfaces"], 16);
        assert_eq!(limits["controlsPerPage"], 128);
        assert_eq!(limits["feedbackRules"], 32);
        assert_eq!(limits["contextRules"], 64);
        assert_eq!(limits["argText"], 16 * 1024);
        assert_eq!(limits["assets"], 0);
        assert_eq!(limits["workspaceSurfaces"], 64);
        assert_eq!(limits.as_object().unwrap().len(), 58);
    }
}
