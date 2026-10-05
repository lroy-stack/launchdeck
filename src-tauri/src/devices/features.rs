//! What a device can do beyond drawing frames, the settings and the text
//! LaunchDeck can give it, and what LaunchDeck observed on a connected
//! device: how far it got with taking the device, the firmware it reported,
//! and a sentence when it is connected with a limitation. Generic: a device
//! that has none of it reports the defaults. The renderer never reads it.

use serde::{Deserialize, Serialize};

use crate::core::model::Rgb;

/// What a device can do beyond drawing frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceFeatures {
    /// LaunchDeck switches it into a mode it owns (Programmer Mode) and back.
    pub host_mode: bool,
    /// Lights by index into a fixed colour table (used for native animation).
    pub palette: bool,
    /// Many LEDs in one message.
    pub batched_lighting: bool,
    /// The period of its native flash, when the device fixes it.
    pub flash_period_ms: Option<u16>,
    /// The period of its native pulse, when the device fixes it.
    pub pulse_period_ms: Option<u16>,
    pub brightness: bool,
    pub sleep: bool,
    pub scrolling_text: bool,
    /// The device reports a firmware version.
    pub firmware_version: bool,
}

/// A setting LaunchDeck gives a connected device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSetting {
    /// LED brightness in percent; `None` gives the device its own level back.
    Brightness(Option<u8>),
    /// LEDs off (`true`) or on again.
    Sleep(bool),
}

/// Text the device scrolls across its pads by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextScroll {
    pub text: String,
    pub color: Rgb,
    /// Pads per second.
    pub speed: u8,
    pub direction: ScrollDirection,
    pub looping: bool,
}

/// The most characters LaunchDeck scrolls. LaunchDeck's limit: the
/// device's own is undocumented.
const TEXT_MAX_CHARS: usize = 64;
/// The fastest scroll, in pads per second (MINI-PRM p20).
const TEXT_SPEED_MAX: u8 = 63;

const TEXT_PROBLEM: &str = "Scrolling text takes up to 64 letters, digits and punctuation marks.";
const SPEED_PROBLEM: &str = "Scrolling text moves at 1 to 63 pads per second.";

impl TextScroll {
    /// Whether LaunchDeck sends this text: 1 to `TEXT_MAX_CHARS` printable
    /// ASCII characters (`20h`-`7Eh`, which the device's font is
    /// documented for) at 1 to `TEXT_SPEED_MAX` pads per second; the
    /// sentence for the person otherwise.
    pub fn check(&self) -> Result<(), &'static str> {
        let text = self.text.as_bytes();
        if text.is_empty()
            || text.len() > TEXT_MAX_CHARS
            || !text.iter().all(|c| (0x20..=0x7E).contains(c))
        {
            return Err(TEXT_PROBLEM);
        }
        if !(1..=TEXT_SPEED_MAX).contains(&self.speed) {
            return Err(SPEED_PROBLEM);
        }
        Ok(())
    }
}

/// Which way scrolling text moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollDirection {
    /// From right to left, as text is read.
    #[default]
    Left,
    /// From left to right.
    Right,
}

/// Whether LaunchDeck holds the mode it needs on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModeState {
    /// The device has no such mode, or LaunchDeck does not hold it.
    #[default]
    None,
    /// Being identified and switched over.
    Acquiring,
    /// The device said it is in the mode.
    Confirmed,
    /// Switched over, but the device did not say so.
    Unconfirmed,
}

/// What LaunchDeck observed on a connected device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDetails {
    /// The firmware version as the device shows it, e.g. `0531`.
    pub firmware: Option<String>,
    pub mode: ModeState,
    /// Percent, when LaunchDeck set or read it.
    pub brightness: Option<u8>,
    pub asleep: bool,
    pub scrolling: bool,
    /// Connected, with a limitation, in a sentence.
    pub notice: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn features_serialize_in_camel_case_and_read_with_defaults() {
        assert_eq!(
            serde_json::to_value(DeviceFeatures::default()).unwrap(),
            json!({
                "hostMode": false,
                "palette": false,
                "batchedLighting": false,
                "flashPeriodMs": null,
                "pulsePeriodMs": null,
                "brightness": false,
                "sleep": false,
                "scrollingText": false,
                "firmwareVersion": false,
            })
        );
        let read: DeviceFeatures =
            serde_json::from_value(json!({ "sleep": true, "flashPeriodMs": 500 })).unwrap();
        assert_eq!(
            read,
            DeviceFeatures {
                sleep: true,
                flash_period_ms: Some(500),
                ..DeviceFeatures::default()
            }
        );
    }

    #[test]
    fn text_scrolls_serialize_in_camel_case() {
        let scroll = TextScroll {
            text: "Hello World".into(),
            color: Rgb::new(255, 255, 255),
            speed: 7,
            direction: ScrollDirection::Right,
            looping: true,
        };
        let value = serde_json::to_value(&scroll).unwrap();
        assert_eq!(
            value,
            json!({
                "text": "Hello World",
                "color": "#ffffff",
                "speed": 7,
                "direction": "right",
                "looping": true,
            })
        );
        assert_eq!(serde_json::from_value::<TextScroll>(value).unwrap(), scroll);
        assert_eq!(ScrollDirection::default(), ScrollDirection::Left);
    }

    #[test]
    fn text_is_checked_against_launchdecks_limits() {
        let scroll = |text: &str, speed: u8| TextScroll {
            text: text.into(),
            color: Rgb::new(255, 255, 255),
            speed,
            direction: ScrollDirection::Left,
            looping: false,
        };
        let longest = "x".repeat(TEXT_MAX_CHARS);
        for ok in [
            scroll(" ~Hello, World!", 1),
            scroll(&longest, TEXT_SPEED_MAX),
        ] {
            assert_eq!(ok.check(), Ok(()), "{:?}", ok.text);
        }
        let too_long = "x".repeat(TEXT_MAX_CHARS + 1);
        for text in ["", too_long.as_str(), "é", "café", "tab\there", "\u{7F}"] {
            assert_eq!(
                scroll(text, 7).check(),
                Err("Scrolling text takes up to 64 letters, digits and punctuation marks."),
                "{text:?}"
            );
        }
        for speed in [0, TEXT_SPEED_MAX + 1, 255] {
            assert_eq!(
                scroll("Hi", speed).check(),
                Err("Scrolling text moves at 1 to 63 pads per second."),
                "{speed}"
            );
        }
    }

    #[test]
    fn details_serialize_in_camel_case_with_every_field() {
        assert_eq!(
            serde_json::to_value(DeviceDetails::default()).unwrap(),
            json!({
                "firmware": null,
                "mode": "none",
                "brightness": null,
                "asleep": false,
                "scrolling": false,
                "notice": null,
            })
        );
        let details = DeviceDetails {
            firmware: Some("0531".into()),
            mode: ModeState::Unconfirmed,
            notice: Some("Connected, but not quite.".into()),
            ..DeviceDetails::default()
        };
        let value = serde_json::to_value(&details).unwrap();
        assert_eq!(value["firmware"], "0531");
        assert_eq!(value["mode"], "unconfirmed");
        assert_eq!(value["notice"], "Connected, but not quite.");
        assert_eq!(
            serde_json::from_value::<DeviceDetails>(value).unwrap(),
            details
        );
        for (mode, name) in [
            (ModeState::Acquiring, "acquiring"),
            (ModeState::Confirmed, "confirmed"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), name);
        }
    }
}
