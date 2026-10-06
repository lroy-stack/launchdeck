//! What LaunchDeck sets on a unit it holds besides its frames, as bytes and
//! as the state release needs: the LED brightness (Mini MK3 plan section
//! 12.1) and scrolling text (section 12.3). Pure: the session decides when,
//! these say what to send.
//!
//! Brightness is read, then set: the unit answers the question before it
//! applies the setting, so the first answer since connect is the person's
//! own level, which release gives back if LaunchDeck changed it.
//!
//! Text scrolls over the frame, which is still drawn underneath. No message
//! says when a scroll ends: one that does not loop is taken as over after
//! `(characters × 6 + 9) / speed` seconds (a column of 6 per character is
//! an assumption).

use std::time::{Duration, Instant};

use super::protocol::{
    brightness, query, text, text_stop, to_7bit, Product, TextColour, BRIGHTNESS,
};
use super::session::REPLY_TIMEOUT;
use crate::devices::{DeviceFeatures, ScrollDirection, TextScroll};

/// The highest brightness level (MINI-PRM p22).
const BRIGHTNESS_MAX: u8 = 0x7F;

/// The fastest scroll the text command carries, pads per second (p20).
const TEXT_SPEED_MAX: u8 = 63;
/// Pad columns per character of scrolling text, and the columns the text
/// takes to come in and leave: an assumption, the font is undocumented.
const TEXT_COLUMNS_PER_CHAR: u64 = 6;
const TEXT_COLUMNS_EXTRA: u64 = 9;

/// The LED brightness LaunchDeck sets, and the unit's own.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Brightness {
    /// The level to set, 0-127; `None` leaves the unit's own.
    wanted: Option<u8>,
    /// The first level the unit reported since connect: what release
    /// gives back.
    saved: Option<u8>,
    /// LaunchDeck set a level the unit still shows.
    changed: bool,
    /// When the question still unanswered was sent.
    asked: Option<Instant>,
}

impl Brightness {
    /// The level wanted, and nothing the unit reported: for a new
    /// connection.
    pub(crate) fn wanted_only(&self) -> Self {
        Self {
            wanted: self.wanted,
            ..Self::default()
        }
    }

    /// The level to set from now on, in percent; `None` leaves the unit's
    /// own.
    pub(crate) fn want(&mut self, percent: Option<u8>) {
        self.wanted = percent.map(level);
    }

    /// Reads the unit's brightness, then sets the wanted level: the unit
    /// answers before it applies the setting. Nothing without a level.
    pub(crate) fn set(&mut self, product: Product, now: Instant) -> Vec<Vec<u8>> {
        let Some(level) = self.wanted else {
            return Vec::new();
        };
        self.changed = true;
        self.asked = Some(now);
        vec![query(product, BRIGHTNESS), brightness(product, level)]
    }

    /// Gives the unit its own level back now, if LaunchDeck changed it and
    /// knows it.
    pub(crate) fn unset(&mut self, product: Product) -> Vec<Vec<u8>> {
        match self.saved {
            Some(own) if self.changed => {
                self.changed = false;
                vec![brightness(product, own)]
            }
            _ => Vec::new(),
        }
    }

    /// The unit's brightness: the first answer since connect is its own.
    pub(crate) fn read(&mut self, level: u8) {
        self.asked = None;
        self.saved.get_or_insert(level.min(BRIGHTNESS_MAX));
    }

    /// What release gives back: the unit's own level, if LaunchDeck
    /// changed it.
    pub(crate) fn release(&self, product: Product) -> Option<Vec<u8>> {
        let own = self.saved.filter(|_| self.changed)?;
        Some(brightness(product, own))
    }

    /// Whether the last question just went unanswered for `REPLY_TIMEOUT`.
    pub(crate) fn unanswered(&mut self, now: Instant) -> bool {
        match self.asked {
            Some(asked) if now.saturating_duration_since(asked) >= REPLY_TIMEOUT => {
                self.asked = None;
                true
            }
            _ => false,
        }
    }

    /// In percent: the level set, otherwise the one read.
    pub(crate) fn percent(&self) -> Option<u8> {
        self.wanted.or(self.saved).map(percent)
    }
}

/// Text LaunchDeck is scrolling.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Scroll {
    /// When the running scroll is taken as over; `Some(None)` for one that
    /// loops.
    running: Option<Option<Instant>>,
}

impl Scroll {
    /// Starts this scroll: `07 <loop> <speed> 01 <r> <g> <b> <ascii>`, the
    /// speed `80h - speed` to the right.
    pub(crate) fn start(&mut self, product: Product, now: Instant, scroll: &TextScroll) -> Vec<u8> {
        let speed = scroll.speed.clamp(1, TEXT_SPEED_MAX);
        let ascii = scroll.text.as_bytes();
        let over = (!scroll.looping).then(|| now + scroll_time(ascii.len(), speed));
        self.running = Some(over);
        let signed = match scroll.direction {
            ScrollDirection::Left => speed as i8,
            ScrollDirection::Right => -(speed as i8),
        };
        let c = scroll.color;
        let colour = TextColour::Rgb(to_7bit(c.r), to_7bit(c.g), to_7bit(c.b));
        text(product, scroll.looping, signed, colour, ascii)
    }

    /// Stops any scroll: the text command alone.
    pub(crate) fn stop(&mut self, product: Product) -> Vec<u8> {
        self.running = None;
        text_stop(product)
    }

    pub(crate) fn running(&self) -> bool {
        self.running.is_some()
    }

    /// The unit was taken again: whatever ran there is not LaunchDeck's.
    pub(crate) fn forget(&mut self) {
        self.running = None;
    }

    /// Whether a scroll that does not loop just ran its estimated time.
    pub(crate) fn ran_out(&mut self, now: Instant) -> bool {
        match self.running {
            Some(Some(over)) if now >= over => {
                self.running = None;
                true
            }
            _ => false,
        }
    }
}

/// Features of a profile that a unit's firmware turned out not to answer
/// for: a documented readback left unanswered while others were answered.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Unsupported {
    pub(crate) brightness: bool,
    pub(crate) sleep: bool,
}

impl Unsupported {
    /// The features, less these.
    pub(crate) fn remove_from(&self, mut features: DeviceFeatures) -> DeviceFeatures {
        features.brightness &= !self.brightness;
        features.sleep &= !self.sleep;
        features
    }

    /// The person's notice for a unit with this firmware, if any feature
    /// is missing.
    pub(crate) fn notice(&self, firmware: &str) -> Option<String> {
        let missing: Vec<&str> = [("brightness", self.brightness), ("sleep", self.sleep)]
            .into_iter()
            .filter_map(|(name, missing)| missing.then_some(name))
            .collect();
        (!missing.is_empty()).then(|| {
            format!(
                "Firmware version {firmware} does not support {}. Novation Components can update it.",
                missing.join(" or ")
            )
        })
    }
}

/// How long text takes to scroll by once, at `speed` pads per second.
fn scroll_time(chars: usize, speed: u8) -> Duration {
    let columns = chars as u64 * TEXT_COLUMNS_PER_CHAR + TEXT_COLUMNS_EXTRA;
    Duration::from_millis(columns * 1000 / u64::from(speed.max(1)))
}

/// Percent 0-100 to a brightness level 0-127, rounded: 50 is `40h`.
fn level(percent: u8) -> u8 {
    let level = (u16::from(percent.min(100)) * u16::from(BRIGHTNESS_MAX) + 50) / 100;
    level as u8
}

/// A brightness level 0-127 to percent, rounded.
fn percent(level: u8) -> u8 {
    let max = u16::from(BRIGHTNESS_MAX);
    ((u16::from(level.min(BRIGHTNESS_MAX)) * 100 + max / 2) / max) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_percent_and_levels_round_to_each_other() {
        assert_eq!([0, 25, 50, 100, 255].map(level), [0, 32, 64, 127, 127]);
        assert_eq!([0, 32, 64, 127, 255].map(percent), [0, 25, 50, 100, 100]);
        for p in 0..=100 {
            assert_eq!(percent(level(p)), p, "{p} %");
        }
    }

    #[test]
    fn the_firmware_notice_names_every_missing_feature() {
        let none = Unsupported::default();
        assert_eq!(none.notice("0531"), None);
        let both = Unsupported {
            brightness: true,
            sleep: true,
        };
        assert_eq!(
            both.notice("0531").as_deref(),
            Some("Firmware version 0531 does not support brightness or sleep. Novation Components can update it.")
        );
        let features = both.remove_from(DeviceFeatures {
            brightness: true,
            sleep: true,
            scrolling_text: true,
            ..DeviceFeatures::default()
        });
        assert!(!features.brightness && !features.sleep && features.scrolling_text);
    }

    #[test]
    fn a_scroll_takes_six_columns_a_character_and_nine_more() {
        assert_eq!(scroll_time(2, 7), Duration::from_secs(3));
        assert_eq!(scroll_time(11, 15), Duration::from_millis(5000));
    }
}
