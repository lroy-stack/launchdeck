//! Surface renderer: semantic visual state in, device-capability-aware frame out.
//!
//! ```text
//! page + control states  ->  semantic LEDs  ->  adapt(capabilities, time)  ->  SurfaceFrame
//! ```
//!
//! No protocol knowledge lives here. Degradation rules:
//! * positions the device does not have are dropped
//! * animation the device cannot do natively is synthesised from timed frames
//! * a non-RGB device receives luminance only

use std::f32::consts::TAU;

use crate::core::model::{Animation, Control, ControlPos, Page, Rgb, Visual};
use crate::devices::{DeviceCapabilities, Led, LedCell, SurfaceFrame};

const PULSE_PERIOD_MS: u64 = 1400;
const FLASH_PERIOD_MS: u64 = 700;

/// Brightness multiplier for a software pulse at time `t_ms` (0.25..=1.0).
pub fn pulse_factor(t_ms: u64) -> f32 {
    let phase = (t_ms % PULSE_PERIOD_MS) as f32 / PULSE_PERIOD_MS as f32;
    0.25 + 0.75 * (0.5 - 0.5 * (TAU * phase).cos())
}

/// Whether a software flash is in its "on" half at time `t_ms`.
pub fn flash_on(t_ms: u64) -> bool {
    (t_ms % FLASH_PERIOD_MS) < FLASH_PERIOD_MS / 2
}

/// Pick the visual for a control given its current state name.
pub fn resolve_visual(control: &Control, state: Option<&str>) -> Visual {
    state
        .and_then(|s| control.states.get(s))
        .copied()
        .unwrap_or(control.visual)
}

/// Translate one semantic LED into what this device can actually show.
pub fn adapt(visual: Visual, caps: &DeviceCapabilities, t_ms: u64) -> Led {
    let mut color = visual.color;
    let mut animation = Animation::None;
    match visual.animation {
        Animation::None => {}
        Animation::Pulse if caps.animations.pulse => animation = Animation::Pulse,
        Animation::Pulse => color = color.scaled(pulse_factor(t_ms)),
        Animation::Flash if caps.animations.flash => animation = Animation::Flash,
        Animation::Flash => {
            if !flash_on(t_ms) {
                color = Rgb::OFF;
            }
        }
    }
    if !caps.rgb {
        let l = color.luma();
        color = Rgb::new(l, l, l);
    }
    Led { color, animation }
}

/// Compose a full frame for one device.
///
/// `state_of` returns the current state name for a control id on this page.
pub fn compose(
    page: Option<&Page>,
    state_of: &dyn Fn(&str) -> Option<String>,
    caps: &DeviceCapabilities,
    t_ms: u64,
) -> SurfaceFrame {
    let positions = caps.positions();
    let mut leds: Vec<LedCell> = positions
        .iter()
        .map(|&pos| LedCell { pos, led: Led::OFF })
        .collect();
    let Some(page) = page else {
        return SurfaceFrame { leds };
    };

    let mut set = |pos: ControlPos, led: Led| {
        if let Some(cell) = leds.iter_mut().find(|c| c.pos == pos) {
            cell.led = led;
        }
    };

    for control in &page.controls {
        let state = state_of(&control.id);
        let led = adapt(resolve_visual(control, state.as_deref()), caps, t_ms);
        match control.pos {
            ControlPos::Grid { .. } => {
                for (col, row) in control.cells() {
                    set(ControlPos::Grid { col, row }, led);
                }
            }
            other => set(other, led),
        }
    }
    SurfaceFrame { leds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::Span;
    use crate::devices::{AnimationCaps, AuxCaps, GridCaps};

    const GREEN: Rgb = Rgb::new(0, 200, 0);

    fn full_caps() -> DeviceCapabilities {
        DeviceCapabilities {
            grid: Some(GridCaps {
                width: 8,
                height: 8,
            }),
            aux: AuxCaps {
                top: 8,
                side: 8,
                logo: true,
            },
            rgb: true,
            velocity: true,
            pressure: false,
            animations: AnimationCaps {
                pulse: true,
                flash: true,
            },
        }
    }

    fn page_with(control: Control) -> Page {
        Page {
            id: "p".into(),
            name: "P".into(),
            controls: vec![control],
        }
    }

    fn pulsing_control() -> Control {
        let mut c = Control::new(ControlPos::grid(1, 1), "Run", Rgb::new(0, 0, 255));
        c.states.insert("running".into(), Visual::pulse(GREEN));
        c.states
            .insert("error".into(), Visual::flash(Rgb::new(255, 0, 0)));
        c
    }

    #[test]
    fn state_selects_the_visual_and_falls_back_to_idle() {
        let c = pulsing_control();
        assert_eq!(resolve_visual(&c, None), c.visual);
        assert_eq!(resolve_visual(&c, Some("running")), Visual::pulse(GREEN));
        assert_eq!(resolve_visual(&c, Some("unknown")), c.visual);
    }

    #[test]
    fn native_animation_is_passed_through() {
        let led = adapt(Visual::pulse(GREEN), &full_caps(), 123);
        assert_eq!(
            led,
            Led {
                color: GREEN,
                animation: Animation::Pulse
            }
        );
    }

    #[test]
    fn missing_pulse_is_synthesised_from_timed_frames() {
        let mut caps = full_caps();
        caps.animations = AnimationCaps {
            pulse: false,
            flash: false,
        };
        let brightest = adapt(Visual::pulse(GREEN), &caps, PULSE_PERIOD_MS / 2);
        let dimmest = adapt(Visual::pulse(GREEN), &caps, 0);
        assert_eq!(brightest.animation, Animation::None);
        assert_eq!(brightest.color, GREEN);
        assert!(
            dimmest.color.g < brightest.color.g,
            "pulse must vary over time"
        );
        assert!(dimmest.color.g > 0, "pulse never goes fully dark");
    }

    #[test]
    fn missing_flash_alternates_between_colour_and_off() {
        let mut caps = full_caps();
        caps.animations = AnimationCaps {
            pulse: false,
            flash: false,
        };
        let on = adapt(Visual::flash(GREEN), &caps, 0);
        let off = adapt(Visual::flash(GREEN), &caps, FLASH_PERIOD_MS / 2);
        assert_eq!(on.color, GREEN);
        assert_eq!(off.color, Rgb::OFF);
    }

    #[test]
    fn non_rgb_devices_get_luminance_only() {
        let mut caps = full_caps();
        caps.rgb = false;
        let led = adapt(Visual::solid(Rgb::new(255, 0, 0)), &caps, 0);
        assert_eq!(led.color.r, led.color.g);
        assert_eq!(led.color.g, led.color.b);
        assert!(led.color.r > 0);
    }

    #[test]
    fn small_grids_clip_controls_instead_of_failing() {
        let mut caps = full_caps();
        caps.grid = Some(GridCaps {
            width: 4,
            height: 4,
        });
        caps.aux = AuxCaps::default();
        let mut inside = Control::new(ControlPos::grid(1, 1), "In", GREEN);
        inside.span = Span { w: 1, h: 1 };
        let outside = Control::new(ControlPos::grid(6, 6), "Out", GREEN);
        let page = Page {
            id: "p".into(),
            name: "P".into(),
            controls: vec![inside, outside],
        };
        let frame = compose(Some(&page), &|_| None, &caps, 0);
        assert_eq!(frame.leds.len(), 16);
        assert_eq!(frame.get(ControlPos::grid(1, 1)).unwrap().color, GREEN);
        assert!(frame.get(ControlPos::grid(6, 6)).is_none());
    }

    #[test]
    fn frames_cover_every_device_position_and_span_cells_light_together() {
        let mut big = Control::new(ControlPos::grid(0, 0), "Big", GREEN);
        big.span = Span { w: 2, h: 2 };
        let frame = compose(Some(&page_with(big)), &|_| None, &full_caps(), 0);
        assert_eq!(frame.leds.len(), 81);
        for (col, row) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(frame.get(ControlPos::grid(col, row)).unwrap().color, GREEN);
        }
        assert_eq!(frame.get(ControlPos::grid(2, 0)).unwrap(), Led::OFF);
    }

    #[test]
    fn compose_applies_control_state() {
        let page = page_with(pulsing_control());
        let state = |id: &str| (id == "g.1.1").then(|| "running".to_string());
        let frame = compose(Some(&page), &state, &full_caps(), 0);
        assert_eq!(
            frame.get(ControlPos::grid(1, 1)).unwrap(),
            Led {
                color: GREEN,
                animation: Animation::Pulse
            }
        );
    }
}
