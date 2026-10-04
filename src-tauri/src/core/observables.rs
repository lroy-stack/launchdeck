//! What the app itself lets a control observe.
//!
//! Two native providers that are not plugins and offer no actions:
//!
//! * `runtime` ("LaunchDeck"): which surface and page are on screen, and the
//!   events of running actions, navigation and devices;
//! * `device` ("Devices"): a control was pressed.
//!
//! Only things with a real current value are states. *Which surface is on
//! screen* is one; *a button was pressed* is not, and stays an event.

use crate::actions::definition::{ProviderInfo, ProviderKind, ProviderStatus};
use crate::events::types;
use crate::state::catalog::Source;
use crate::state::definition::ObservableDefinition;
use crate::state::StateAddress;

/// The bus source of action, navigation and device-connection events.
pub const RUNTIME: &str = "runtime";
/// The bus source of control presses.
pub const DEVICE: &str = "device";

/// The surface on screen now (its id).
pub const SURFACE_ACTIVE: &str = "surface.active";
/// The page on screen now (its id, within the active surface).
pub const PAGE_ACTIVE: &str = "page.active";

pub fn surface_active() -> StateAddress {
    StateAddress::new(RUNTIME, SURFACE_ACTIVE)
}

pub fn page_active() -> StateAddress {
    StateAddress::new(RUNTIME, PAGE_ACTIVE)
}

/// The states the runtime reports to the hub. Always running.
pub fn states() -> Vec<ObservableDefinition> {
    vec![
        ObservableDefinition::state(
            RUNTIME,
            SURFACE_ACTIVE,
            "Active surface",
            "The id of the surface on screen now.",
        )
        .category("LaunchDeck")
        .keywords(&["surface", "navigation", "tab", "current"])
        .text(),
        ObservableDefinition::state(
            RUNTIME,
            PAGE_ACTIVE,
            "Active page",
            "The id of the page on screen now, within the active surface.",
        )
        .category("LaunchDeck")
        .keywords(&["page", "navigation", "current"])
        .text(),
    ]
}

fn runtime_event(kind: &str, name: &str, description: &str) -> ObservableDefinition {
    ObservableDefinition::event(RUNTIME, kind, name, description).category("LaunchDeck")
}

/// What the runtime announces on the bus under the source `runtime`.
pub fn runtime_events() -> Vec<ObservableDefinition> {
    let action = |kind: &str, name: &str, description: &str| {
        runtime_event(kind, name, description)
            .keywords(&["action", "run"])
            .field("action", "Action")
            .field("controlId", "Control")
            .field("surfaceId", "Surface")
            .field("pageId", "Page")
    };
    vec![
        action(
            types::ACTION_STARTED,
            "Action started",
            "A control began running its action.",
        ),
        action(
            types::ACTION_COMPLETED,
            "Action completed",
            "A control's action finished without an error.",
        ),
        action(
            types::ACTION_FAILED,
            "Action failed",
            "A control's action ended with an error.",
        )
        .field("error", "Error"),
        runtime_event(
            types::SURFACE_CHANGED,
            "Surface changed",
            "Another surface came on screen.",
        )
        .keywords(&["navigation"])
        .field("surfaceId", "Surface")
        .field("pageId", "Page"),
        runtime_event(
            types::PAGE_CHANGED,
            "Page changed",
            "Another page came on screen.",
        )
        .keywords(&["navigation"])
        .field("surfaceId", "Surface")
        .field("pageId", "Page"),
        runtime_event(
            types::DEVICE_CONNECTED,
            "Device connected",
            "A controller was connected.",
        )
        .keywords(&["hardware", "launchpad"])
        .field("deviceId", "Device"),
        runtime_event(
            types::DEVICE_DISCONNECTED,
            "Device disconnected",
            "A controller was disconnected.",
        )
        .keywords(&["hardware", "launchpad"])
        .field("deviceId", "Device"),
    ]
}

/// What a connected device announces on the bus under the source `device`.
pub fn device_events() -> Vec<ObservableDefinition> {
    vec![ObservableDefinition::event(
        DEVICE,
        types::CONTROL_PRESSED,
        "Control pressed",
        "A control was pressed on a connected device.",
    )
    .category("Devices")
    .keywords(&["press", "button", "pad"])
    .field("deviceId", "Device")
    .field("surfaceId", "Surface")
    .field("pageId", "Page")
    .field("controlId", "Control")
    .field("label", "Label")
    .field_number("velocity", "Velocity")]
}

/// The native providers, as the Observable Catalog lists them.
pub fn sources() -> Vec<Source> {
    let info = |id: &str, name: &str, description: &str| ProviderInfo {
        id: id.to_string(),
        name: name.to_string(),
        kind: ProviderKind::Builtin,
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: description.to_string(),
        status: ProviderStatus::Ready,
        reason: None,
    };
    let mut runtime = states();
    runtime.extend(runtime_events());
    vec![
        Source {
            info: info(
                RUNTIME,
                "LaunchDeck",
                "The app itself: navigation, running actions and devices.",
            ),
            observables: runtime,
        },
        Source {
            info: info(DEVICE, "Devices", "Controls pressed on a connected device."),
            observables: device_events(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_definition_is_valid_and_belongs_to_its_provider() {
        for source in sources() {
            assert!(!source.observables.is_empty());
            for d in &source.observables {
                d.check().unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(d.provider, source.info.id);
            }
        }
    }

    #[test]
    fn every_event_the_runtime_publishes_is_listed() {
        let listed: Vec<(String, String)> = sources()
            .into_iter()
            .flat_map(|s| s.observables)
            .filter(|d| !d.is_state())
            .map(|d| (d.provider, d.local))
            .collect();
        for (source, kind) in [
            (RUNTIME, types::ACTION_STARTED),
            (RUNTIME, types::ACTION_COMPLETED),
            (RUNTIME, types::ACTION_FAILED),
            (RUNTIME, types::SURFACE_CHANGED),
            (RUNTIME, types::PAGE_CHANGED),
            (RUNTIME, types::DEVICE_CONNECTED),
            (RUNTIME, types::DEVICE_DISCONNECTED),
            (DEVICE, types::CONTROL_PRESSED),
        ] {
            assert!(
                listed.contains(&(source.to_string(), kind.to_string())),
                "{source} {kind} is published but not listed"
            );
        }
    }

    #[test]
    fn only_what_has_a_current_value_is_a_state() {
        let states = states();
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|d| d.is_state() && d.params.is_empty()));
    }
}
