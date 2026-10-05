//! The LaunchDeck domain model.
//!
//! Everything here is hardware independent: no MIDI note numbers, no device
//! coordinates beyond the logical `ControlPos`. Device adapters translate.

use crate::context::rules::ContextSettings;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::actions::definition::Assignment;
pub use crate::state::feedback::FeedbackRule;

/// Current on-disk schema version. Bump together with a migration in
/// `persistence::migrate`.
///
/// 4: a control's `bindings` became `feedback` rules (see `state::feedback`).
pub const SCHEMA_VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------

/// 8-bit RGB colour, serialised as `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Scale brightness by `factor` in `0.0..=1.0`.
    pub fn scaled(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        let s = |c: u8| (f32::from(c) * f).round() as u8;
        Self::new(s(self.r), s(self.g), s(self.b))
    }

    /// Perceived luminance, `0..=255`.
    pub fn luma(self) -> u8 {
        let l =
            0.2126 * f32::from(self.r) + 0.7152 * f32::from(self.g) + 0.0722 * f32::from(self.b);
        l.round().clamp(0.0, 255.0) as u8
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid colour `{s}`")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Animation {
    #[default]
    None,
    Pulse,
    Flash,
}

/// What a control looks like in one state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Visual {
    pub color: Rgb,
    #[serde(default)]
    pub animation: Animation,
}

impl Visual {
    pub const fn solid(color: Rgb) -> Self {
        Self {
            color,
            animation: Animation::None,
        }
    }
    pub const fn pulse(color: Rgb) -> Self {
        Self {
            color,
            animation: Animation::Pulse,
        }
    }
    pub const fn flash(color: Rgb) -> Self {
        Self {
            color,
            animation: Animation::Flash,
        }
    }
    pub const OFF: Visual = Visual::solid(Rgb::OFF);
}

// ---------------------------------------------------------------------------
// Positions
// ---------------------------------------------------------------------------

/// Logical position of a control on a surface. Grid rows count from the top
/// (row 0 is the top row); `Top` and `Side` indices count left-to-right and
/// top-to-bottom respectively. `Logo` is an output-only LED.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ControlPos {
    Grid { col: u8, row: u8 },
    Top { index: u8 },
    Side { index: u8 },
    Logo,
}

impl ControlPos {
    pub const fn grid(col: u8, row: u8) -> Self {
        ControlPos::Grid { col, row }
    }

    /// Stable string key, e.g. `g.3.4`, `t.0`, `s.7`.
    pub fn key(&self) -> String {
        match self {
            ControlPos::Grid { col, row } => format!("g.{col}.{row}"),
            ControlPos::Top { index } => format!("t.{index}"),
            ControlPos::Side { index } => format!("s.{index}"),
            ControlPos::Logo => "logo".to_string(),
        }
    }
}

/// How many grid cells a control occupies. Larger targets are an accessibility
/// feature: every cell triggers the same control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub w: u8,
    pub h: u8,
}

impl Default for Span {
    fn default() -> Self {
        Span { w: 1, h: 1 }
    }
}

// ---------------------------------------------------------------------------
// Controls, pages, surfaces
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: String,
    pub pos: ControlPos,
    #[serde(default)]
    pub span: Span,
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// Editor-only glyph (emoji or short text). Never required on hardware.
    #[serde(default)]
    pub icon: Option<String>,
    /// The `idle` appearance.
    pub visual: Visual,
    /// Named appearances, the control's *looks* (`running`, `success`, ...).
    /// The saved field keeps its schema 1 name; the interface says "look",
    /// because "state" now also means an observed value.
    #[serde(default)]
    pub states: BTreeMap<String, Visual>,
    #[serde(default)]
    pub press: Option<Assignment>,
    #[serde(default)]
    pub long_press: Option<Assignment>,
    /// What decides which look is shown, as one ordered list. The first state
    /// rule that is true decides; an event rule shows its look for a while.
    /// See `state::feedback`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<FeedbackRule>,
    /// Schema 3 bindings that could not be converted to feedback rules, kept
    /// word for word so nothing is lost. Inert; the inspector only says they
    /// exist.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_bindings: Vec<serde_json::Value>,
}

impl Control {
    pub fn new(pos: ControlPos, label: &str, color: Rgb) -> Self {
        Self {
            id: pos.key(),
            pos,
            span: Span::default(),
            label: label.to_string(),
            description: String::new(),
            icon: None,
            visual: Visual::solid(color),
            states: BTreeMap::new(),
            press: None,
            long_press: None,
            feedback: Vec::new(),
            legacy_bindings: Vec::new(),
        }
    }

    /// All grid cells covered by this control (empty for non-grid controls).
    pub fn cells(&self) -> Vec<(u8, u8)> {
        match self.pos {
            ControlPos::Grid { col, row } => {
                let mut out = Vec::new();
                for dy in 0..self.span.h.max(1) {
                    for dx in 0..self.span.w.max(1) {
                        out.push((col.saturating_add(dx), row.saturating_add(dy)));
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub name: String,
    pub controls: Vec<Control>,
}

impl Page {
    pub fn control(&self, id: &str) -> Option<&Control> {
        self.controls.iter().find(|c| c.id == id)
    }

    /// Find the control that owns a physical position (span aware).
    pub fn control_at(&self, pos: ControlPos) -> Option<&Control> {
        match pos {
            ControlPos::Grid { col, row } => self.controls.iter().find(|c| {
                matches!(c.pos, ControlPos::Grid { .. }) && c.cells().contains(&(col, row))
            }),
            other => self.controls.iter().find(|c| c.pos == other),
        }
    }
}

/// An early hint for choosing a surface by application. Not used: Auto Context
/// has ordered rules in the workspace's `context`. Kept so that workspaces
/// written with it still load unchanged; nothing creates one any more.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextHint {
    #[serde(default)]
    pub foreground_apps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Surface {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// `ai`, `development`, `productivity`, `smart-home`, `accessibility`, `custom`.
    pub category: String,
    pub pages: Vec<Page>,
    /// Read from old files and written back only where one was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextHint>,
    /// The Surface file or Pack this surface was imported from. Absent on
    /// every surface made here, so a workspace without imports is written
    /// exactly as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<ImportOrigin>,
}

/// Where an imported surface came from: which file, which import of it, and
/// the digests that tell whether it changed since. Local only: never
/// exported, and it holds no time, path or anything about this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOrigin {
    /// `launchdeck.surface` or `launchdeck.pack`.
    pub format: String,
    pub artifact_id: String,
    pub artifact_name: String,
    pub artifact_version: String,
    /// Which import of the artifact, from 1: importing it again as new makes
    /// another instance; replacing one keeps its number.
    pub instance: u32,
    /// The surface's id in the file.
    pub source_surface_id: String,
    /// `integrity.content` of the file.
    pub content_digest: String,
    /// `sha256:` and the digest of the surface as it was written here, in
    /// its canonical file form: a surface whose digest is another now may
    /// have been changed since it was imported.
    pub surface_digest: String,
}

impl Surface {
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }
    pub fn home_page(&self) -> Option<&Page> {
        self.pages.first()
    }
}

// ---------------------------------------------------------------------------
// Workspace (persisted)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default)]
    pub onboarding_done: bool,
    /// Port of the localhost-only WebSocket event interface.
    #[serde(default = "default_ws_port")]
    pub external_events_port: u16,
    #[serde(default = "default_true")]
    pub external_events_enabled: bool,
}

fn default_ws_port() -> u16 {
    47821
}
fn default_true() -> bool {
    true
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            onboarding_done: false,
            external_events_port: default_ws_port(),
            external_events_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub schema_version: u32,
    pub surfaces: Vec<Surface>,
    #[serde(default)]
    pub last_active_surface: Option<String>,
    #[serde(default)]
    pub preferences: Preferences,
    /// Auto Context: whether it is on, its rules and its fallback. Absent in
    /// a workspace that never used it, which is then saved exactly as before.
    #[serde(default, skip_serializing_if = "ContextSettings::is_default")]
    pub context: ContextSettings,
}

impl Workspace {
    pub fn surface(&self, id: &str) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.id == id)
    }
    pub fn surface_mut(&mut self, id: &str) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_hex_roundtrip() {
        let c = Rgb::from_hex("#3b82f6").unwrap();
        assert_eq!(c, Rgb::new(0x3b, 0x82, 0xf6));
        assert_eq!(c.to_hex(), "#3b82f6");
        assert!(Rgb::from_hex("3b82f6").is_none());
        assert!(Rgb::from_hex("#zzzzzz").is_none());
    }

    #[test]
    fn controls_store_assignments_not_executable_shapes() {
        let mut c = Control::new(ControlPos::grid(0, 0), "Open", Rgb::new(1, 2, 3));
        c.press = Some(Assignment::new(
            "system",
            "openUrl",
            serde_json::json!({ "url": "https://github.com" }),
        ));
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(
            json["press"],
            serde_json::json!({
                "provider": "system",
                "action": "openUrl",
                "args": { "url": "https://github.com" }
            })
        );
        assert!(json["longPress"].is_null());
        let back: Control = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn spanning_control_owns_all_cells() {
        let mut page = Page {
            id: "p".into(),
            name: "P".into(),
            controls: vec![],
        };
        let mut c = Control::new(ControlPos::grid(2, 2), "Big", Rgb::new(1, 2, 3));
        c.span = Span { w: 2, h: 2 };
        page.controls.push(c);
        for (col, row) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
            assert_eq!(
                page.control_at(ControlPos::grid(col, row)).unwrap().label,
                "Big"
            );
        }
        assert!(page.control_at(ControlPos::grid(4, 2)).is_none());
    }
}
