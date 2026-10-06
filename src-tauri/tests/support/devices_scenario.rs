//! The device list as Settings > Devices reads it (`DeviceInfo`): the
//! simulator as the device manager lists it once connected, and, built by
//! hand because no unit is attached here, a connected Launchpad Mini MK3 with
//! its details, features and a notice, and a Launchpad model LaunchDeck does
//! not drive. Shared by the `dump_devices` example (which writes
//! `examples/devices.json`) and the contract test.

use launchdeck_lib::devices::manager::{DeviceInfo, DeviceManager, DeviceStatus};
use launchdeck_lib::devices::virtual_device::{VirtualLaunchpadX, VIRTUAL_ID};
use launchdeck_lib::devices::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceFeatures, DeviceIdentity,
    DeviceKind, GridCaps, ModeState,
};
use serde_json::Value;

/// The sentence of plan section 12.4 for a unit whose firmware did not
/// answer the brightness readback.
pub const NOTICE: &str =
    "Firmware version 0531 does not support brightness. Novation Components can update it.";

pub fn capture() -> Value {
    serde_json::to_value(devices()).expect("serialise")
}

fn devices() -> Vec<DeviceInfo> {
    vec![simulator(), mini(), unsupported()]
}

fn simulator() -> DeviceInfo {
    let (input, _) = tokio::sync::mpsc::unbounded_channel();
    let mut manager = DeviceManager::new(input);
    let (device, _handle) = VirtualLaunchpadX::new();
    manager.register(Box::new(device));
    manager.connect(VIRTUAL_ID).expect("the simulator connects");
    manager.list().remove(0)
}

fn mini() -> DeviceInfo {
    let details = DeviceDetails {
        firmware: Some("0531".into()),
        mode: ModeState::Confirmed,
        brightness: None,
        asleep: false,
        scrolling: false,
        notice: Some(NOTICE.into()),
    };
    DeviceInfo {
        identity: DeviceIdentity {
            id: "launchpad-mini-mk3:1".into(),
            name: "Launchpad Mini MK3".into(),
            model: "launchpad-mini-mk3".into(),
            kind: DeviceKind::Hardware,
        },
        capabilities: DeviceCapabilities {
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
            velocity: false,
            pressure: false,
            animations: AnimationCaps {
                pulse: true,
                flash: true,
            },
        },
        connected: true,
        status: DeviceStatus::Connected,
        problem: None,
        notice: details.notice.clone(),
        details,
        features: DeviceFeatures {
            host_mode: true,
            palette: true,
            batched_lighting: true,
            flash_period_ms: Some(500),
            pulse_period_ms: Some(1000),
            brightness: true,
            sleep: true,
            scrolling_text: true,
            firmware_version: true,
        },
    }
}

fn unsupported() -> DeviceInfo {
    DeviceInfo {
        identity: DeviceIdentity {
            id: "unsupported:launchpad-pro-mk3".into(),
            name: "Launchpad Pro MK3".into(),
            model: "unsupported".into(),
            kind: DeviceKind::Hardware,
        },
        capabilities: DeviceCapabilities {
            grid: None,
            aux: AuxCaps::default(),
            rgb: false,
            velocity: false,
            pressure: false,
            animations: AnimationCaps::default(),
        },
        connected: false,
        status: DeviceStatus::Unsupported,
        problem: None,
        notice: None,
        details: DeviceDetails::default(),
        features: DeviceFeatures::default(),
    }
}
