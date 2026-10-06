//! The content of a file (plan 7.5): surfaces, their pages and controls, and
//! context rules. The types are the format's own; [`super::convert`] turns
//! them into the workspace model and back.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::convert::page_to_model;
use super::rejection::{echo, plain_message, Rejection, RejectionCode};
use super::text;
use crate::actions::params::valid_param_name;
use crate::artifacts::json::segment;
use crate::artifacts::limits::*;
use crate::context::portable::PortableApplication;
use crate::core::model::Rgb;
use crate::core::validate::validate_control;
use crate::state::feedback::{check_rule_shape, FeedbackRule};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentV1 {
    pub surfaces: Vec<SurfaceV1>,
    /// Pack only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_rules: Vec<ContextRuleV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceV1 {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub category: String,
    pub pages: Vec<PageV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageV1 {
    pub id: String,
    pub name: String,
    pub controls: Vec<ControlV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlV1 {
    pub id: String,
    pub pos: Pos,
    /// Grid only; absent for one cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The `idle` look.
    pub visual: VisualV1,
    /// The workspace calls them `states`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub looks: BTreeMap<String, VisualV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<AssignmentV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_press: Option<AssignmentV1>,
    /// Exactly the workspace's rules.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "strict_rules"
    )]
    pub feedback: Vec<FeedbackRule>,
}

/// Where a control is. No logo: it is output only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Pos {
    Grid { col: u8, row: u8 },
    Top { index: u8 },
    Side { index: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub w: u8,
    pub h: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualV1 {
    pub color: Color,
    #[serde(default, skip_serializing_if = "AnimationV1::is_none")]
    pub animation: AnimationV1,
}

/// `#rrggbb`, lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub Rgb);

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let hex = s.strip_prefix('#').unwrap_or_default();
        let well_formed = hex.len() == 6
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        match Rgb::from_hex(&s) {
            Some(rgb) if well_formed => Ok(Color(rgb)),
            _ => Err(serde::de::Error::custom(format!(
                "\"{}\" is not a colour written #rrggbb in lowercase",
                echo(&s)
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimationV1 {
    #[default]
    None,
    Pulse,
    Flash,
}

impl AnimationV1 {
    fn is_none(&self) -> bool {
        *self == AnimationV1::None
    }
}

/// What a press or a long press does: a provider, one of its actions, the
/// arguments, and how they relate to the rest of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentV1 {
    pub provider: String,
    pub action: String,
    pub args: Map<String, Value>,
    /// Pack only: arguments the person's answer to an input replaces.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    /// Arguments that name a surface of this file, renamed with it on import.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_refs: Vec<String>,
}

/// Pack only: show `surface` (and `page`) when `application` is in front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextRuleV1 {
    pub application: AppRefV1,
    pub surface: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

/// Exactly one of: the application, named on each system, or an
/// `application` input the person answers on import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AppRefV1 {
    Application(PortableApplication),
    Input(InputRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRef {
    pub input: String,
}

/// The workspace's `FeedbackRule` refuses unknown members everywhere but in
/// a test (`is`) and a timed hold, which it ignores; a file refuses them
/// there too. A rule that reads back without a member it had, had one too many.
fn strict_rules<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FeedbackRule>, D::Error> {
    use serde::de::Error;
    let raw = Vec::<Value>::deserialize(d)?;
    let mut rules = Vec::with_capacity(raw.len());
    for value in raw {
        let rule: FeedbackRule = serde_json::from_value(value.clone()).map_err(D::Error::custom)?;
        let back = serde_json::to_value(&rule).map_err(D::Error::custom)?;
        for path in [&["when", "is"][..], &["hold"]] {
            let (Some(Value::Object(read)), Some(Value::Object(kept))) =
                (pointer(&value, path), pointer(&back, path))
            else {
                continue;
            };
            if let Some(extra) = read.keys().find(|k| !kept.contains_key(*k)) {
                return Err(D::Error::custom(format!(
                    "unknown field `{}` in `{}` of a feedback rule",
                    echo(extra),
                    path.join(".")
                )));
            }
        }
        rules.push(rule);
    }
    Ok(rules)
}

fn pointer<'v>(value: &'v Value, path: &[&str]) -> Option<&'v Value> {
    path.iter().try_fold(value, |v, key| v.get(key))
}

const C: RejectionCode = RejectionCode::InvalidContent;

fn too_many(at: &str, what: &str, max: usize) -> Rejection {
    Rejection::at(
        RejectionCode::TooMany,
        at,
        format!("A file has at most {max} {what}."),
    )
}

/// Check content on its own (plan 7.5): counts, text, ids, duplicate ids,
/// looks, arguments, feedback rules and geometry. References between parts
/// and to the manifest are [`super::check_cross`]'s. Counts over a limit are
/// `tooMany`; everything else `invalidContent`.
pub fn check_content(content: &ContentV1) -> Result<(), Rejection> {
    let at = "/content/surfaces";
    if content.surfaces.is_empty() {
        return Err(Rejection::at(C, at, "A file holds at least one surface."));
    }
    if content.surfaces.len() > MAX_PACK_SURFACES {
        return Err(too_many(at, "surfaces", MAX_PACK_SURFACES));
    }
    let controls: usize = content
        .surfaces
        .iter()
        .flat_map(|s| &s.pages)
        .map(|p| p.controls.len())
        .sum();
    if controls > MAX_CONTROLS {
        return Err(too_many(at, "controls", MAX_CONTROLS));
    }
    if content.context_rules.len() > MAX_CONTEXT_RULES {
        return Err(too_many(
            "/content/contextRules",
            "context rules",
            MAX_CONTEXT_RULES,
        ));
    }
    if bindings(content) > MAX_BINDINGS {
        return Err(too_many("/content", "input bindings", MAX_BINDINGS));
    }
    let mut ids = HashSet::new();
    for (i, surface) in content.surfaces.iter().enumerate() {
        let at = format!("{at}/{i}");
        check_surface(&at, surface)?;
        if !ids.insert(surface.id.as_str()) {
            return Err(duplicate(&format!("{at}/id"), "surfaces", &surface.id));
        }
    }
    for (i, rule) in content.context_rules.iter().enumerate() {
        check_context_rule(&format!("/content/contextRules/{i}"), rule)?;
    }
    Ok(())
}

/// Arguments and context rules bound to an input.
fn bindings(content: &ContentV1) -> usize {
    let assignments: usize = content
        .surfaces
        .iter()
        .flat_map(|s| &s.pages)
        .flat_map(|p| &p.controls)
        .flat_map(|c| [&c.press, &c.long_press])
        .flatten()
        .map(|a| a.inputs.len())
        .sum();
    let rules = content
        .context_rules
        .iter()
        .filter(|r| matches!(r.application, AppRefV1::Input(_)))
        .count();
    assignments + rules
}

fn duplicate(at: &str, what: &str, id: &str) -> Rejection {
    Rejection::at(C, at, format!("Two {what} have the id \"{}\".", echo(id)))
}

fn check_icon(at: &str, icon: Option<&str>) -> Result<(), Rejection> {
    match icon {
        Some(icon) => text::name(C, at, "An icon", icon, MAX_ICON),
        None => Ok(()),
    }
}

fn check_surface(at: &str, s: &SurfaceV1) -> Result<(), Rejection> {
    text::id(
        C,
        &format!("{at}/id"),
        "The surface id",
        &s.id,
        text::surface_id,
        "it is up to 64 lowercase letters, digits and dashes",
    )?;
    text::name(
        C,
        &format!("{at}/name"),
        "A surface name",
        &s.name,
        MAX_LABEL,
    )?;
    text::description(
        C,
        &format!("{at}/description"),
        "A surface description",
        &s.description,
        MAX_SURFACE_DESCRIPTION,
    )?;
    check_icon(&format!("{at}/icon"), s.icon.as_deref())?;
    text::id(
        C,
        &format!("{at}/category"),
        "The category",
        &s.category,
        text::category,
        "it is a lowercase letter, then up to 31 lowercase letters, digits and dashes",
    )?;
    let pages = format!("{at}/pages");
    if s.pages.is_empty() {
        return Err(Rejection::at(C, pages, "A surface has at least one page."));
    }
    if s.pages.len() > MAX_PAGES {
        return Err(too_many(&pages, "pages in one surface", MAX_PAGES));
    }
    let mut ids = HashSet::new();
    for (i, page) in s.pages.iter().enumerate() {
        let at = format!("{pages}/{i}");
        check_page(&at, page)?;
        if !ids.insert(page.id.as_str()) {
            return Err(duplicate(
                &format!("{at}/id"),
                "pages of a surface",
                &page.id,
            ));
        }
    }
    Ok(())
}

fn check_page(at: &str, p: &PageV1) -> Result<(), Rejection> {
    text::id(
        C,
        &format!("{at}/id"),
        "The page id",
        &p.id,
        text::local_id,
        "it is 1 to 64 letters, digits, dots, colons, dashes and underscores",
    )?;
    text::name(C, &format!("{at}/name"), "A page name", &p.name, MAX_LABEL)?;
    let controls = format!("{at}/controls");
    if p.controls.len() > MAX_CONTROLS_PER_PAGE {
        return Err(too_many(
            &controls,
            "controls on one page",
            MAX_CONTROLS_PER_PAGE,
        ));
    }
    let mut ids = HashSet::new();
    for (i, control) in p.controls.iter().enumerate() {
        let at = format!("{controls}/{i}");
        check_control(&at, control)?;
        if !ids.insert(control.id.as_str()) {
            return Err(duplicate(
                &format!("{at}/id"),
                "controls of a page",
                &control.id,
            ));
        }
    }
    // The editor's own geometry check, on the page as it would be imported.
    let page = page_to_model(p);
    for (i, (control, dto)) in page.controls.iter().zip(&p.controls).enumerate() {
        let placed = validate_control(&page, control).and_then(|()| {
            within_grid(dto)
                .then_some(())
                .ok_or_else(|| "it reaches past the 16 by 16 grid".to_string())
        });
        placed.map_err(|why| {
            Rejection::at(
                C,
                format!("{controls}/{i}"),
                format!(
                    "The control \"{}\" cannot be placed: {}.",
                    echo(&control.label),
                    plain_message(&why)
                ),
            )
        })?;
    }
    Ok(())
}

fn check_control(at: &str, c: &ControlV1) -> Result<(), Rejection> {
    text::id(
        C,
        &format!("{at}/id"),
        "The control id",
        &c.id,
        text::local_id,
        "it is 1 to 64 letters, digits, dots, colons, dashes and underscores",
    )?;
    text::name(C, &format!("{at}/label"), "A label", &c.label, MAX_LABEL)?;
    text::description(
        C,
        &format!("{at}/description"),
        "A control description",
        &c.description,
        MAX_CONTROL_DESCRIPTION,
    )?;
    check_icon(&format!("{at}/icon"), c.icon.as_deref())?;
    check_span(at, c)?;
    if c.looks.len() > MAX_LOOKS {
        return Err(too_many(
            &format!("{at}/looks"),
            "looks on one control",
            MAX_LOOKS,
        ));
    }
    for look in c.looks.keys() {
        text::name(
            C,
            &format!("{at}/looks/{}", segment(look)),
            "A look name",
            look,
            MAX_LOOK_NAME,
        )?;
    }
    for (slot, assignment) in [("press", &c.press), ("longPress", &c.long_press)] {
        if let Some(a) = assignment {
            check_assignment(&format!("{at}/{slot}"), a)?;
        }
    }
    if c.feedback.len() > MAX_FEEDBACK_RULES {
        return Err(too_many(
            &format!("{at}/feedback"),
            "feedback rules on one control",
            MAX_FEEDBACK_RULES,
        ));
    }
    for (i, rule) in c.feedback.iter().enumerate() {
        if let Some(issue) = check_rule_shape(i, rule).into_iter().next() {
            return Err(Rejection::at(
                C,
                format!("{at}/feedback/{i}"),
                format!(
                    "A feedback rule is not valid: {}.",
                    plain_message(&issue.message)
                ),
            ));
        }
    }
    Ok(())
}

/// One cell unless on the grid, 1 to 8 each way on it.
fn check_span(at: &str, c: &ControlV1) -> Result<(), Rejection> {
    let refused = |why: &str| Err(Rejection::at(C, format!("{at}/span"), why.to_string()));
    match (c.pos, c.span) {
        (Pos::Grid { .. }, Some(span))
            if !(1..=MAX_SPAN).contains(&span.w) || !(1..=MAX_SPAN).contains(&span.h) =>
        {
            refused("A control spans 1 to 8 cells each way.")
        }
        (Pos::Top { .. } | Pos::Side { .. }, Some(_)) => {
            refused("Only a control on the grid spans several cells.")
        }
        _ => Ok(()),
    }
}

/// The grid ends at [`MAX_GRID`] columns and rows, and so does the device
/// requirement a file can state: a control may not reach past it.
fn within_grid(c: &ControlV1) -> bool {
    let span = c.span.unwrap_or(Span { w: 1, h: 1 });
    match c.pos {
        Pos::Grid { col, row } => {
            u16::from(col) + u16::from(span.w) <= u16::from(MAX_GRID)
                && u16::from(row) + u16::from(span.h) <= u16::from(MAX_GRID)
        }
        Pos::Top { .. } | Pos::Side { .. } => true,
    }
}

fn check_assignment(at: &str, a: &AssignmentV1) -> Result<(), Rejection> {
    text::name(
        C,
        &format!("{at}/provider"),
        "A provider",
        &a.provider,
        MAX_ID,
    )?;
    text::name(C, &format!("{at}/action"), "An action", &a.action, MAX_ID)?;
    let args = format!("{at}/args");
    if a.args.len() > MAX_ARG_KEYS {
        return Err(too_many(&args, "arguments in one action", MAX_ARG_KEYS));
    }
    if let Some(name) = a.args.keys().find(|k| !valid_param_name(k)) {
        return Err(Rejection::at(
            C,
            &args,
            format!("\"{}\" is not an argument name.", echo(name)),
        ));
    }
    let object = Value::Object(a.args.clone());
    if depth(&object) > MAX_ARG_DEPTH {
        return Err(Rejection::at(
            C,
            &args,
            format!("An action's arguments nest at most {MAX_ARG_DEPTH} levels deep."),
        ));
    }
    if object.to_string().len() > MAX_ARG_BYTES {
        return Err(Rejection::at(
            C,
            &args,
            format!(
                "An action's arguments take at most {} KiB.",
                MAX_ARG_BYTES / 1024
            ),
        ));
    }
    if longest_text(&object) > MAX_ARG_TEXT {
        return Err(Rejection::at(
            C,
            &args,
            format!("A text in an action's arguments is at most {MAX_ARG_TEXT} characters."),
        ));
    }
    for (arg, input) in &a.inputs {
        let at = format!("{at}/inputs/{}", segment(arg));
        if !valid_param_name(arg) || !text::input_id(input) {
            return Err(Rejection::at(
                C,
                at,
                "An input binding names an argument and an input id.",
            ));
        }
    }
    // Each names a distinct argument: counted before they are compared.
    if a.surface_refs.len() > MAX_ARG_KEYS {
        return Err(too_many(
            &format!("{at}/surfaceRefs"),
            "surface references in one action",
            MAX_ARG_KEYS,
        ));
    }
    for (i, arg) in a.surface_refs.iter().enumerate() {
        if !valid_param_name(arg) || a.surface_refs[..i].contains(arg) {
            return Err(Rejection::at(
                C,
                format!("{at}/surfaceRefs/{i}"),
                "A surface reference names an argument, once.",
            ));
        }
    }
    Ok(())
}

/// Arrays and objects nested in one another, the value itself included.
fn depth(v: &Value) -> usize {
    match v {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Characters of the longest text anywhere in `v`.
fn longest_text(v: &Value) -> usize {
    match v {
        Value::String(s) => s.chars().count(),
        Value::Array(items) => items.iter().map(longest_text).max().unwrap_or(0),
        Value::Object(map) => map.values().map(longest_text).max().unwrap_or(0),
        _ => 0,
    }
}

fn check_context_rule(at: &str, r: &ContextRuleV1) -> Result<(), Rejection> {
    let app_at = format!("{at}/application");
    match &r.application {
        AppRefV1::Application(app) => {
            if app.identities.len() > MAX_IDENTITIES {
                return Err(too_many(
                    &format!("{app_at}/identities"),
                    "identities of one application",
                    MAX_IDENTITIES,
                ));
            }
            app.validate().map_err(|why| {
                Rejection::at(
                    C,
                    &app_at,
                    format!("The application is not valid: {}.", plain_message(&why)),
                )
            })?;
            text::name(
                C,
                &format!("{app_at}/name"),
                "An application's name",
                &app.name,
                MAX_APPLICATION_NAME,
            )?;
        }
        AppRefV1::Input(input) => text::id(
            C,
            &format!("{app_at}/input"),
            "The input id",
            &input.input,
            text::input_id,
            "it is a lowercase letter, then up to 31 lowercase letters, digits and underscores",
        )?,
    }
    text::id(
        C,
        &format!("{at}/surface"),
        "The surface id",
        &r.surface,
        text::surface_id,
        "it is up to 64 lowercase letters, digits and dashes",
    )?;
    match &r.page {
        Some(page) => text::id(
            C,
            &format!("{at}/page"),
            "The page id",
            page,
            text::local_id,
            "it is 1 to 64 letters, digits, dots, colons, dashes and underscores",
        ),
        None => Ok(()),
    }
}
