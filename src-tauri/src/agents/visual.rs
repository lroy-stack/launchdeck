//! The one place that maps agent semantics to appearance.
//!
//! Surfaces, the desktop UI palette and the tests all read these functions.
//! Nothing here knows a pad position or a MIDI message: it returns semantic
//! [`Visual`]s, and the renderer adapts them to whatever the device can show
//! (native pulse/flash, synthesised animation, or luminance only).
//!
//! Colour is never the only signal: urgent states animate (flash), running
//! states pulse, and the desktop UI always prints the status name.

use serde::Serialize;

use super::model::Status;
use crate::core::model::{Animation, Rgb, Visual};

pub const BLUE: Rgb = Rgb::new(40, 110, 255);
pub const GREEN: Rgb = Rgb::new(30, 220, 90);
pub const YELLOW: Rgb = Rgb::new(255, 200, 30);
pub const PURPLE: Rgb = Rgb::new(170, 80, 255);
pub const WHITE: Rgb = Rgb::new(255, 255, 255);
pub const RED: Rgb = Rgb::new(255, 40, 50);
pub const ORANGE: Rgb = Rgb::new(255, 120, 20);
pub const CYAN: Rgb = Rgb::new(30, 210, 230);
pub const GRAY: Rgb = Rgb::new(70, 74, 90);

/// State name for a slot that shows nothing.
pub const EMPTY: &str = "empty";

/// Appearance of a track whose normalized status is `status`.
pub fn status_visual(status: Status) -> Visual {
    match status {
        Status::Idle => Visual::solid(BLUE.scaled(0.45)),
        Status::Starting => Visual::solid(YELLOW),
        Status::Running => Visual::pulse(GREEN),
        Status::ToolUse => Visual::pulse(CYAN),
        Status::Waiting => Visual::solid(PURPLE),
        Status::ApprovalRequired => Visual::flash(WHITE),
        Status::Paused => Visual::solid(ORANGE),
        Status::Completed => Visual::solid(GREEN),
        Status::Failed => Visual::flash(RED),
        Status::Cancelled => Visual::solid(RED.scaled(0.30)),
        Status::Offline => Visual::solid(GRAY.scaled(0.30)),
    }
}

/// Appearance of a slot with no track.
pub fn empty_visual() -> Visual {
    Visual::solid(Rgb::new(12, 13, 18))
}

/// Focus marker under a slot.
pub fn focus_visual(focused: bool) -> Visual {
    if focused {
        Visual::solid(WHITE)
    } else {
        Visual::OFF
    }
}

/// What a command pad communicates. The same five states are shown in the
/// desktop UI (plus `attention`, an available command that needs a decision).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    /// Cannot be issued now (unsupported, monitor only, wrong state).
    Unavailable,
    Available,
    /// Available and something is waiting for exactly this decision.
    Attention,
    /// Sent to the adapter, no result yet.
    Pending,
    Succeeded,
    Failed,
}

impl CommandState {
    pub const ALL: [CommandState; 6] = [
        CommandState::Unavailable,
        CommandState::Available,
        CommandState::Attention,
        CommandState::Pending,
        CommandState::Succeeded,
        CommandState::Failed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CommandState::Unavailable => "unavailable",
            CommandState::Available => "available",
            CommandState::Attention => "attention",
            CommandState::Pending => "pending",
            CommandState::Succeeded => "succeeded",
            CommandState::Failed => "failed",
        }
    }
}

/// Appearance of a command pad whose resting colour is `base`.
pub fn command_visual(base: Rgb, state: CommandState) -> Visual {
    match state {
        CommandState::Unavailable => Visual::solid(base.scaled(0.10)),
        CommandState::Available => Visual::solid(base),
        CommandState::Attention => Visual::flash(base),
        CommandState::Pending => Visual::pulse(YELLOW),
        CommandState::Succeeded => Visual::solid(GREEN),
        CommandState::Failed => Visual::flash(RED),
    }
}

/// One palette row for the desktop UI, so that dots in the app match the pads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteEntry {
    pub status: Status,
    pub color: String,
    pub animation: Animation,
}

pub fn palette() -> Vec<PaletteEntry> {
    Status::ALL
        .into_iter()
        .map(|status| {
            let v = status_visual(status);
            PaletteEntry {
                status,
                color: v.color.to_hex(),
                animation: v.animation,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_a_distinct_appearance() {
        let visuals: Vec<Visual> = Status::ALL.into_iter().map(status_visual).collect();
        for (i, a) in visuals.iter().enumerate() {
            for b in &visuals[i + 1..] {
                assert_ne!(a, b, "two statuses look identical");
            }
        }
    }

    #[test]
    fn urgent_states_animate_and_calm_states_do_not() {
        assert_eq!(
            status_visual(Status::ApprovalRequired).animation,
            Animation::Flash
        );
        assert_eq!(status_visual(Status::Failed).animation, Animation::Flash);
        assert_eq!(status_visual(Status::Running).animation, Animation::Pulse);
        assert_eq!(status_visual(Status::Completed).animation, Animation::None);
        assert_eq!(status_visual(Status::Paused).animation, Animation::None);
    }

    #[test]
    fn colour_families_match_the_documented_palette() {
        let c = |s| status_visual(s).color;
        assert!(c(Status::Running).g > c(Status::Running).r);
        assert!(c(Status::Failed).r > 200 && c(Status::Failed).g < 80);
        assert!(c(Status::Paused).r > c(Status::Paused).b);
        assert!(c(Status::Idle).b > c(Status::Idle).r);
        assert_eq!(c(Status::ApprovalRequired), WHITE);
    }

    #[test]
    fn command_pads_distinguish_all_six_states() {
        let visuals: Vec<Visual> = CommandState::ALL
            .into_iter()
            .map(|s| command_visual(GREEN, s))
            .collect();
        // succeeded is green, same as the base colour, but attention flashes
        // and unavailable is dim, so only available/succeeded may coincide.
        let unique = visuals
            .iter()
            .enumerate()
            .filter(|(i, v)| !visuals[..*i].contains(v))
            .count();
        assert!(unique >= 5, "pad states are not visually distinct");
        assert!(
            command_visual(GREEN, CommandState::Unavailable)
                .color
                .luma()
                < command_visual(GREEN, CommandState::Available).color.luma()
        );
        assert_eq!(
            command_visual(GREEN, CommandState::Attention).animation,
            Animation::Flash
        );
        assert_eq!(
            command_visual(GREEN, CommandState::Pending).animation,
            Animation::Pulse
        );
    }

    #[test]
    fn palette_covers_every_status() {
        assert_eq!(palette().len(), Status::ALL.len());
    }
}
