//! What LaunchDeck observed on a connected device beyond the frames it
//! draws: how far it got with taking the device, the firmware it reported,
//! and a sentence when it is connected with a limitation. Generic: a device
//! that has none of it reports the defaults.

use serde::{Deserialize, Serialize};

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
