//! The manifest of a file (plan 7.4, 7.6-7.8): who made it and with what,
//! what it needs, what the person is asked for, and the content digest.

use serde::{Deserialize, Serialize};

use super::rejection::{echo, plain_message, Rejection, RejectionCode};
use super::text;
use crate::artifacts::limits::*;
use crate::context::identity::ApplicationIdentity;
use crate::plugins::external::manifest::valid_plugin_id;

/// The `app` of `createdWith`.
pub const CREATED_WITH_APP: &str = "launchdeck";
/// The algorithm prefix of `integrity.content`.
const DIGEST_PREFIX: &str = "sha256:";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestV1 {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    pub created_with: CreatedWith,
    pub requirements: Requirements,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tested_with: Vec<TestedWith>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<DependencyV1>,
    /// Pack only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputDef>,
    pub integrity: Integrity,
}

/// Shown as text, never fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Informational.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatedWith {
    pub app: String,
    pub version: String,
}

/// What a device needs to show everything (plan 7.7), computed by the
/// exporter and recomputed on import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    /// Absent when no control is on the grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridRequirement>,
    pub aux: AuxRequirement,
    pub rgb: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridRequirement {
    pub min_width: u8,
    pub min_height: u8,
}

/// Pads above (`top`) and beside (`side`) the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuxRequirement {
    pub top: u8,
    pub side: u8,
}

/// A device the file was used with, informational.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestedWith {
    pub model: String,
    pub kind: TestedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestedKind {
    Hardware,
    Virtual,
}

/// An installed plugin the content uses (plan 7.6). Host providers (one
/// segment: `system`, `shell`...) are never dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyV1 {
    pub plugin_id: String,
    /// For display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A `semver` requirement, Cargo semantics (`1.2.3` means `^1.2.3`).
    pub version: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Integrity {
    /// `sha256:` and 64 lowercase hex digits (`artifacts::content_digest`).
    pub content: String,
}

/// A value the person gives on import (plan 7.8, Pack only). There is no
/// secret type: secrets stay in the credential store and are never inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: InputType,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<InputValue>,
    /// Enum only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<InputOption>,
    /// String only; [`DEFAULT_STRING_INPUT`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputType {
    Path,
    Url,
    Enum,
    String,
    Application,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputOption {
    pub value: String,
    pub label: String,
}

/// An input's value: a text for every type but `application`, which takes
/// an application identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum InputValue {
    Text {
        value: String,
    },
    #[serde(rename_all = "camelCase")]
    Application {
        namespace: String,
        id: String,
        display_name: String,
    },
}

const M: RejectionCode = RejectionCode::InvalidManifest;

/// Check a manifest on its own (plan 7.4): text, ids, versions, the
/// requirement ranges, dependencies and inputs. Counts over a limit are
/// `tooMany`; everything else `invalidManifest`.
pub fn check_manifest(m: &ManifestV1) -> Result<(), Rejection> {
    let at = "/manifest";
    text::id(
        M,
        &format!("{at}/id"),
        "The id",
        &m.id,
        valid_plugin_id,
        "it is lowercase, dotted, with at least two parts, such as acme.video-editing",
    )?;
    text::name(M, &format!("{at}/name"), "The name", &m.name, MAX_NAME)?;
    version(&format!("{at}/version"), "The version", &m.version)?;
    text::description(
        M,
        &format!("{at}/description"),
        "The description",
        &m.description,
        MAX_DESCRIPTION,
    )?;
    if let Some(author) = &m.author {
        check_author(&format!("{at}/author"), author)?;
    }
    text::line(
        M,
        &format!("{at}/license"),
        "The license",
        &m.license,
        MAX_LICENSE,
    )?;
    check_created_with(&format!("{at}/createdWith"), &m.created_with)?;
    check_requirements(&format!("{at}/requirements"), &m.requirements)?;
    check_tested_with(&format!("{at}/testedWith"), &m.tested_with)?;
    check_dependencies(&format!("{at}/dependencies"), &m.dependencies)?;
    check_inputs(&format!("{at}/inputs"), &m.inputs)?;
    check_integrity(&format!("{at}/integrity/content"), &m.integrity.content)
}

fn too_many(at: &str, what: &str, max: usize) -> Rejection {
    Rejection::at(
        RejectionCode::TooMany,
        at,
        format!("A file has at most {max} {what}."),
    )
}

/// A SemVer version without build metadata, at most [`MAX_VERSION`] characters.
fn version(at: &str, what: &str, v: &str) -> Result<semver::Version, Rejection> {
    let refused = || {
        Rejection::at(
            M,
            at,
            format!(
                "{what} \"{}\" is not a version such as 1.2.0 or 1.2.0-beta.1.",
                echo(v)
            ),
        )
    };
    if v.chars().count() > MAX_VERSION {
        return Err(refused());
    }
    let parsed = semver::Version::parse(v).map_err(|_| refused())?;
    if !parsed.build.is_empty() {
        return Err(refused());
    }
    Ok(parsed)
}

fn check_author(at: &str, author: &Author) -> Result<(), Rejection> {
    text::name(
        M,
        &format!("{at}/name"),
        "The author's name",
        &author.name,
        MAX_AUTHOR_NAME,
    )?;
    let Some(url) = &author.url else {
        return Ok(());
    };
    let at = format!("{at}/url");
    text::line(M, &at, "The author's address", url, MAX_AUTHOR_URL)?;
    let safe = url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    });
    if safe {
        Ok(())
    } else {
        Err(Rejection::at(
            M,
            at,
            format!(
                "The author's address \"{}\" is not an https address without a user name or password.",
                echo(url)
            ),
        ))
    }
}

fn check_created_with(at: &str, created: &CreatedWith) -> Result<(), Rejection> {
    if created.app != CREATED_WITH_APP {
        return Err(Rejection::at(
            M,
            format!("{at}/app"),
            format!(
                "The file says it was made by \"{}\", not LaunchDeck.",
                echo(&created.app)
            ),
        ));
    }
    version(
        &format!("{at}/version"),
        "The LaunchDeck version",
        &created.version,
    )
    .map(|_| ())
}

fn check_requirements(at: &str, r: &Requirements) -> Result<(), Rejection> {
    if let Some(grid) = r.grid {
        let fits = |n: u8| (1..=MAX_GRID).contains(&n);
        if !fits(grid.min_width) || !fits(grid.min_height) {
            return Err(Rejection::at(
                M,
                format!("{at}/grid"),
                format!("A grid requirement is 1 to {MAX_GRID} columns and rows."),
            ));
        }
    }
    if r.aux.top > MAX_AUX || r.aux.side > MAX_AUX {
        return Err(Rejection::at(
            M,
            format!("{at}/aux"),
            format!("A device has at most {MAX_AUX} pads above and beside the grid."),
        ));
    }
    Ok(())
}

fn check_tested_with(at: &str, tested: &[TestedWith]) -> Result<(), Rejection> {
    if tested.len() > MAX_TESTED_WITH {
        return Err(too_many(at, "tested devices", MAX_TESTED_WITH));
    }
    for (i, t) in tested.iter().enumerate() {
        text::id(
            M,
            &format!("{at}/{i}/model"),
            "The device model",
            &t.model,
            text::model,
            "it is 1 to 32 lowercase letters, digits and dashes",
        )?;
    }
    Ok(())
}

fn check_dependencies(at: &str, deps: &[DependencyV1]) -> Result<(), Rejection> {
    if deps.len() > MAX_DEPENDENCIES {
        return Err(too_many(at, "dependencies", MAX_DEPENDENCIES));
    }
    for (i, d) in deps.iter().enumerate() {
        let at = format!("{at}/{i}");
        text::id(
            M,
            &format!("{at}/pluginId"),
            "The plugin id",
            &d.plugin_id,
            valid_plugin_id,
            "a plugin id is lowercase and dotted, with at least two parts",
        )?;
        if i > 0 && deps[i - 1].plugin_id >= d.plugin_id {
            return Err(Rejection::at(
                M,
                format!("{at}/pluginId"),
                "Dependencies are listed once each, in order of their plugin id.",
            ));
        }
        if let Some(name) = &d.name {
            text::name(
                M,
                &format!("{at}/name"),
                "A dependency's name",
                name,
                MAX_DEPENDENCY_NAME,
            )?;
        }
        requirement(&format!("{at}/version"), &d.version)?;
    }
    Ok(())
}

/// A version requirement: `semver` (Cargo semantics), at most
/// [`MAX_REQUIREMENT`] characters and [`MAX_COMPARATORS`] comparators, no
/// pre-release comparator; `*` is any version.
fn requirement(at: &str, req: &str) -> Result<(), Rejection> {
    let refused = |why: &str| {
        Rejection::at(
            M,
            at,
            format!("The version requirement \"{}\" {why}.", echo(req)),
        )
    };
    if req.chars().count() > MAX_REQUIREMENT {
        return Err(refused(&format!(
            "is longer than {MAX_REQUIREMENT} characters"
        )));
    }
    let parsed = semver::VersionReq::parse(req)
        .map_err(|_| refused("is not a requirement such as ^1.2, >=1.0, <2 or *"))?;
    if parsed.comparators.len() > MAX_COMPARATORS {
        return Err(refused(&format!(
            "has more than {MAX_COMPARATORS} comparisons"
        )));
    }
    if parsed.comparators.iter().any(|c| !c.pre.is_empty()) {
        return Err(refused("names a pre-release version"));
    }
    Ok(())
}

fn check_inputs(at: &str, inputs: &[InputDef]) -> Result<(), Rejection> {
    if inputs.len() > MAX_INPUTS {
        return Err(too_many(at, "inputs", MAX_INPUTS));
    }
    for (i, input) in inputs.iter().enumerate() {
        let at = format!("{at}/{i}");
        text::id(
            M,
            &format!("{at}/id"),
            "The input id",
            &input.id,
            text::input_id,
            "it is a lowercase letter, then up to 31 lowercase letters, digits and underscores",
        )?;
        if inputs[..i].iter().any(|other| other.id == input.id) {
            return Err(Rejection::at(
                M,
                format!("{at}/id"),
                format!("Two inputs have the id \"{}\".", input.id),
            ));
        }
        check_input(&at, input)?;
    }
    Ok(())
}

fn check_input(at: &str, input: &InputDef) -> Result<(), Rejection> {
    text::name(
        M,
        &format!("{at}/label"),
        "An input's label",
        &input.label,
        MAX_INPUT_LABEL,
    )?;
    text::description(
        M,
        &format!("{at}/description"),
        "An input's description",
        &input.description,
        MAX_INPUT_DESCRIPTION,
    )?;
    let is_enum = input.kind == InputType::Enum;
    if is_enum != !input.options.is_empty() {
        return Err(Rejection::at(
            M,
            format!("{at}/options"),
            "An enum input lists its options, and no other input has any.",
        ));
    }
    if input.options.len() > MAX_ENUM_OPTIONS {
        return Err(too_many(
            &format!("{at}/options"),
            "options in one input",
            MAX_ENUM_OPTIONS,
        ));
    }
    for (i, option) in input.options.iter().enumerate() {
        let at = format!("{at}/options/{i}");
        text::name(
            M,
            &format!("{at}/value"),
            "An option's value",
            &option.value,
            MAX_OPTION_VALUE,
        )?;
        text::name(
            M,
            &format!("{at}/label"),
            "An option's label",
            &option.label,
            MAX_LABEL,
        )?;
        if input.options[..i].iter().any(|o| o.value == option.value) {
            return Err(Rejection::at(
                M,
                format!("{at}/value"),
                "Two options of an input have the same value.",
            ));
        }
    }
    match input.max_length {
        Some(n) if input.kind != InputType::String || !(1..=MAX_STRING_INPUT).contains(&n) => {
            return Err(Rejection::at(
                M,
                format!("{at}/maxLength"),
                format!("Only a string input has a maximum length, of 1 to {MAX_STRING_INPUT}."),
            ));
        }
        _ => {}
    }
    match &input.default {
        Some(value) => check_default(&format!("{at}/default"), input, value),
        None => Ok(()),
    }
}

/// A default must be a value the input accepts. Whether a path is absolute
/// depends on the system it is imported on, so it is checked then, not here.
fn check_default(at: &str, input: &InputDef, value: &InputValue) -> Result<(), Rejection> {
    let refused = |why: String| Rejection::at(M, at, format!("The default {why}."));
    match (input.kind, value) {
        (
            InputType::Application,
            InputValue::Application {
                namespace,
                id,
                display_name,
            },
        ) => {
            ApplicationIdentity {
                namespace: namespace.clone(),
                id: id.clone(),
                display_name: String::new(),
            }
            .validate()
            .map_err(|e| {
                refused(format!(
                    "is not an application identity: {}",
                    plain_message(&e.to_string())
                ))
            })?;
            text::line(
                M,
                &format!("{at}/displayName"),
                "An application's name",
                display_name,
                MAX_APPLICATION_NAME,
            )
        }
        (InputType::Application, _) | (_, InputValue::Application { .. }) => Err(refused(
            "is not of the input's type: an application input takes an application, every other input a text".into(),
        )),
        (kind, InputValue::Text { value }) => {
            let max = match kind {
                InputType::Path => MAX_PATH,
                InputType::Url => MAX_URL,
                InputType::String => input.max_length.unwrap_or(DEFAULT_STRING_INPUT),
                _ => MAX_OPTION_VALUE,
            };
            text::line(M, at, "The default", value, max)?;
            if value.is_empty() && kind != InputType::String {
                return Err(refused("is empty".into()));
            }
            if kind == InputType::Enum && !input.options.iter().any(|o| &o.value == value) {
                return Err(refused(format!("\"{}\" is not one of the options", echo(value))));
            }
            if kind == InputType::Url && !web_address(value) {
                return Err(refused(format!(
                    "\"{}\" is not an http or https address without a user name or password",
                    echo(value)
                )));
            }
            Ok(())
        }
    }
}

/// An http or https address with a host and no user name or password.
fn web_address(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

fn check_integrity(at: &str, digest: &str) -> Result<(), Rejection> {
    let well_formed = digest.strip_prefix(DIGEST_PREFIX).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    if well_formed {
        Ok(())
    } else {
        Err(Rejection::at(
            M,
            at,
            "The content digest is not \"sha256:\" followed by 64 lowercase hexadecimal digits.",
        ))
    }
}
