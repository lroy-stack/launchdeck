//! The LaunchDeck domain model.
//!
//! Everything here is hardware independent: no MIDI note numbers, no device
//! coordinates beyond the logical `ControlPos`. Device adapters translate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::actions::definition::Assignment;

/// Current on-disk schema version. Bump together with a migration in
/// `persistence::migrate`.
pub const SCHEMA_VERSION: u32 = 3;

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
// Bindings (external state -> control state)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Top-level payload field.
    pub field: String,
    pub equals: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRule {
    pub event: String,
    #[serde(default)]
    pub when: Vec<Condition>,
    /// State name to set. `$payload.<field>` reads a string from the payload.
    pub set_state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// Event source this binding listens to (`demo`, `ci`, ...).
    pub source: String,
    pub rules: Vec<BindingRule>,
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
    /// Named state appearances (`running`, `success`, ...).
    #[serde(default)]
    pub states: BTreeMap<String, Visual>,
    #[serde(default)]
    pub press: Option<Assignment>,
    #[serde(default)]
    pub long_press: Option<Assignment>,
    /// External/plugin state that drives this control's appearance. A control
    /// can listen to several sources (for example the demo plugin and a real
    /// agent on the WebSocket interface).
    #[serde(default)]
    pub bindings: Vec<Binding>,
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
            bindings: Vec::new(),
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

/// Hints the (future) context engine can use to pick this surface.
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
    #[serde(default)]
    pub context: Option<ContextHint>,
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
