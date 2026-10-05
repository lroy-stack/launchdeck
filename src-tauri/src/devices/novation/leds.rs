//! What the LEDs of a unit show, and what a frame needs sent to them.
//!
//! The cache keeps, per LED index, the state last sent in the device's own
//! units (`None`: unknown). A frame becomes a `Plan`: the LEDs whose wanted
//! state differs from the cache, in the frame's order, sent as one lighting
//! SysEx (`03h`, chunked at 81 colourspecs) per render. The cache learns a
//! plan only once every message of it was sent, so a failed send is planned
//! again. Clearing sends every position off whatever the cache knows;
//! invalidating forgets everything, so the next frame is sent whole.

use super::layout::led_index;
use super::protocol::{self, channel_lighting, to_7bit, ChannelLight, Colourspec, Product};
use crate::core::model::ControlPos;
use crate::devices::{Led, SurfaceFrame};

/// LED indices run up to 99, the logo.
const SLOTS: usize = 100;

/// A plan of at most this many LEDs, each of which channel notes can show,
/// goes out as notes (three bytes per LED) instead of one SysEx. 0 keeps
/// every plan in one `03h` message, as the Launchpad X has always been
/// driven; it changes only after a measurement on hardware (Mini MK3 plan,
/// section 8.4).
const NOTE_LIGHTING_MAX_CHANGES: usize = 0;

/// What one LED shows, in the device's units: 7-bit RGB, or palette entries
/// for the native animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "native pulse and flash arrive with package M6")
)]
enum LedState {
    Rgb([u8; 3]),
    Pulse(u8),
    /// Alternates between colour A and colour B.
    Flash {
        b: u8,
        a: u8,
    },
}

impl LedState {
    const OFF: LedState = LedState::Rgb([0, 0, 0]);

    /// The state a frame's LED asks for: its colour in 7-bit RGB. Animation
    /// is synthesised by the renderer for a model without native animation,
    /// so it arrives as colours; native pulse and flash (palette) come with
    /// package M6.
    fn wanted(led: Led) -> LedState {
        let c = led.color;
        LedState::Rgb([to_7bit(c.r), to_7bit(c.g), to_7bit(c.b)])
    }

    fn colourspec(self, index: u8) -> Colourspec {
        match self {
            LedState::Rgb([r, g, b]) => Colourspec::Rgb { index, r, g, b },
            LedState::Pulse(palette) => Colourspec::Pulse { index, palette },
            LedState::Flash { b, a } => Colourspec::Flash { index, b, a },
        }
    }

    /// The channel notes that show this state, when notes can: they carry
    /// palette entries only, so an RGB colour only when it is off. A flash
    /// sets colour A as a static colour, then flashes colour B over it.
    fn notes(self, index: u8) -> Option<Vec<[u8; 3]>> {
        match self {
            LedState::Rgb([0, 0, 0]) => {
                Some(vec![channel_lighting(ChannelLight::Static, index, 0)])
            }
            LedState::Rgb(_) => None,
            LedState::Pulse(palette) => {
                Some(vec![channel_lighting(ChannelLight::Pulse, index, palette)])
            }
            LedState::Flash { b, a } => Some(vec![
                channel_lighting(ChannelLight::Static, index, a),
                channel_lighting(ChannelLight::Flash, index, b),
            ]),
        }
    }
}

/// The LEDs one render changes, `(LED index, state)` in the frame's order.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    changes: Vec<(u8, LedState)>,
}

impl Plan {
    /// The messages that show this plan; none for an empty plan.
    pub(crate) fn messages(&self, product: Product) -> Vec<Vec<u8>> {
        self.as_notes(NOTE_LIGHTING_MAX_CHANGES)
            .unwrap_or_else(|| self.as_sysex(product))
    }

    fn as_sysex(&self, product: Product) -> Vec<Vec<u8>> {
        let specs: Vec<Colourspec> = self
            .changes
            .iter()
            .map(|&(index, state)| state.colourspec(index))
            .collect();
        protocol::lighting(product, &specs)
    }

    /// Channel notes for a plan of 1 to `max` LEDs that notes can show.
    fn as_notes(&self, max: usize) -> Option<Vec<Vec<u8>>> {
        if self.changes.is_empty() || self.changes.len() > max {
            return None;
        }
        let notes = self
            .changes
            .iter()
            .map(|&(index, state)| state.notes(index))
            .collect::<Option<Vec<_>>>()?;
        Some(notes.into_iter().flatten().map(Vec::from).collect())
    }
}

/// What the unit is known to show, per LED index.
#[derive(Debug)]
pub(crate) struct LedCache {
    known: [Option<LedState>; SLOTS],
}

impl Default for LedCache {
    fn default() -> Self {
        Self {
            known: [None; SLOTS],
        }
    }
}

impl LedCache {
    /// Every LED of the frame whose wanted state differs from what the unit
    /// is known to show, in the frame's order. Positions without an LED are
    /// skipped.
    pub(crate) fn plan(&self, frame: &SurfaceFrame) -> Plan {
        let changes = frame
            .leds
            .iter()
            .filter_map(|cell| {
                let index = led_index(cell.pos)?;
                let wanted = LedState::wanted(cell.led);
                (self.shown(index) != Some(wanted)).then_some((index, wanted))
            })
            .collect();
        Plan { changes }
    }

    /// The clear frame: every one of these positions off, in their order,
    /// whatever the cache knows.
    pub(crate) fn clear(positions: &[ControlPos]) -> Plan {
        let changes = positions
            .iter()
            .filter_map(|&pos| led_index(pos))
            .map(|index| (index, LedState::OFF))
            .collect();
        Plan { changes }
    }

    /// Records what a plan showed, once every message of it was sent.
    pub(crate) fn commit(&mut self, plan: Plan) {
        for (index, state) in plan.changes {
            if let Some(slot) = self.known.get_mut(usize::from(index)) {
                *slot = Some(state);
            }
        }
    }

    /// Forgets what the unit shows: the next plan holds every LED of its
    /// frame.
    pub(crate) fn invalidate(&mut self) {
        self.known = [None; SLOTS];
    }

    fn shown(&self, index: u8) -> Option<LedState> {
        self.known.get(usize::from(index)).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Animation, Rgb};
    use crate::devices::novation::launchpad_x::PROFILE;
    use crate::devices::novation::testkit::hex;
    use crate::devices::LedCell;

    const X: Product = Product::X;

    fn cell(pos: ControlPos, color: Rgb) -> LedCell {
        LedCell {
            pos,
            led: Led {
                color,
                animation: Animation::None,
            },
        }
    }

    fn frame(cells: &[(ControlPos, Rgb)]) -> SurfaceFrame {
        SurfaceFrame {
            leds: cells.iter().map(|&(pos, color)| cell(pos, color)).collect(),
        }
    }

    fn positions() -> Vec<ControlPos> {
        PROFILE.capabilities.positions()
    }

    /// The cache after a clear, as on a freshly taken unit.
    fn cleared() -> LedCache {
        let mut cache = LedCache::default();
        cache.commit(LedCache::clear(&positions()));
        cache
    }

    #[test]
    fn a_plan_holds_what_changed_in_the_frames_order() {
        let cache = cleared();
        let f = frame(&[
            (ControlPos::Side { index: 3 }, Rgb::new(255, 0, 0)),
            (ControlPos::grid(0, 0), Rgb::OFF),
            (ControlPos::Top { index: 1 }, Rgb::new(0, 0, 255)),
        ]);
        assert_eq!(
            cache.plan(&f).messages(X),
            [hex("F0 00 20 29 02 0C 03 03 3B 7F 00 00 03 5C 00 00 7F F7")],
            "side 3 (CC 59) then top 1 (CC 92); the dark pad was dark"
        );
    }

    #[test]
    fn nothing_is_sent_twice_once_committed() {
        let mut cache = cleared();
        let f = frame(&[(ControlPos::grid(0, 0), Rgb::new(255, 0, 128))]);
        let plan = cache.plan(&f);
        assert_eq!(
            plan.messages(X),
            [hex("F0 00 20 29 02 0C 03 03 51 7F 00 40 F7")]
        );
        cache.commit(plan);
        assert_eq!(cache.plan(&f), Plan::default());
        assert!(Plan::default().messages(X).is_empty());
    }

    #[test]
    fn a_plan_not_committed_is_planned_again() {
        let cache = cleared();
        let f = frame(&[(ControlPos::grid(0, 0), Rgb::new(255, 0, 128))]);
        assert_eq!(cache.plan(&f), cache.plan(&f));
        assert!(!cache.plan(&f).changes.is_empty());
    }

    #[test]
    fn a_colour_change_the_unit_cannot_show_is_not_sent() {
        let mut cache = cleared();
        let red = frame(&[(ControlPos::grid(0, 0), Rgb::new(255, 0, 0))]);
        cache.commit(cache.plan(&red));
        let almost = frame(&[(ControlPos::grid(0, 0), Rgb::new(254, 0, 1))]);
        assert_eq!(cache.plan(&almost), Plan::default(), "both are 7F 00 00");
    }

    #[test]
    fn the_clear_frame_is_every_position_off_in_position_order() {
        let mut cache = cleared();
        let lit = frame(&[(ControlPos::Logo, Rgb::new(9, 9, 9))]);
        cache.commit(cache.plan(&lit));
        let clear = LedCache::clear(&positions()).messages(X);
        assert_eq!(clear.len(), 1);
        assert_eq!(clear[0].len(), 413, "81 RGB colourspecs");
        assert_eq!(
            &clear[0][7..12],
            [0x03, 81, 0, 0, 0],
            "the top left pad first"
        );
        assert_eq!(
            &clear[0][413 - 6..],
            [0x03, 99, 0, 0, 0, 0xF7],
            "the logo last"
        );
        cache.commit(LedCache::clear(&positions()));
        assert_eq!(
            cache.plan(&frame(&[(ControlPos::Logo, Rgb::OFF)])),
            Plan::default()
        );
    }

    #[test]
    fn after_invalidating_a_frame_is_sent_whole_without_a_clear() {
        let mut cache = cleared();
        let all: Vec<(ControlPos, Rgb)> = positions().into_iter().map(|p| (p, Rgb::OFF)).collect();
        assert_eq!(cache.plan(&frame(&all)), Plan::default());
        cache.invalidate();
        let messages = cache.plan(&frame(&all)).messages(X);
        assert_eq!(messages, LedCache::clear(&positions()).messages(X));
    }

    #[test]
    fn plans_are_chunked_at_81_colourspecs() {
        let cache = LedCache::default();
        let cells: Vec<(ControlPos, Rgb)> = positions()
            .into_iter()
            .cycle()
            .take(100)
            .map(|p| (p, Rgb::new(1, 2, 3)))
            .collect();
        let messages = cache.plan(&frame(&cells)).messages(X);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].len(), 7 + 81 * 5 + 1);
        assert_eq!(messages[1].len(), 7 + 19 * 5 + 1);
    }

    #[test]
    fn positions_without_an_led_are_skipped() {
        let f = frame(&[(ControlPos::grid(8, 0), Rgb::new(255, 0, 0))]);
        assert_eq!(LedCache::default().plan(&f), Plan::default());
    }

    #[test]
    fn native_animations_are_palette_colourspecs() {
        let plan = Plan {
            changes: vec![
                (11, LedState::Pulse(5)),
                (12, LedState::Flash { b: 21, a: 0 }),
            ],
        };
        assert_eq!(
            plan.messages(Product::MINI_MK3),
            [hex("F0 00 20 29 02 0D 03 02 0B 05 01 0C 15 00 F7")]
        );
    }

    #[test]
    fn notes_are_used_only_below_the_limit_and_for_palette_states() {
        let plan = Plan {
            changes: vec![
                (11, LedState::Pulse(5)),
                (12, LedState::Flash { b: 21, a: 0 }),
                (13, LedState::OFF),
            ],
        };
        assert_eq!(
            plan.as_notes(3),
            Some(vec![
                hex("92 0B 05"),
                hex("90 0C 00"),
                hex("91 0C 15"),
                hex("90 0D 00")
            ])
        );
        assert_eq!(plan.as_notes(2), None, "too many");
        let rgb = Plan {
            changes: vec![(11, LedState::Rgb([1, 2, 3]))],
        };
        assert_eq!(rgb.as_notes(8), None, "RGB needs SysEx");
        assert_eq!(Plan::default().as_notes(8), None);
        assert_eq!(
            NOTE_LIGHTING_MAX_CHANGES, 0,
            "one SysEx per render until measured"
        );
        assert_eq!(
            plan.messages(Product::MINI_MK3).len(),
            1,
            "so even a small palette plan is one SysEx"
        );
    }
}
