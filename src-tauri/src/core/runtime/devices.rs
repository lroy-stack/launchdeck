//! Devices: drawing the active page on every connected device, the device
//! list (connect, disconnect, rescan for hardware), and what a device can
//! be given beyond frames (brightness).

use serde_json::json;

use super::{control_key, Inner, Runtime};
use crate::devices::manager::{DeviceChange, DeviceInfo};
use crate::devices::DeviceError;

/// The highest brightness, in percent.
const BRIGHTNESS_MAX: u8 = 100;
use crate::events::types;
use crate::renderer;

impl Runtime {
    pub fn render_tick(&self) {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let Inner {
                workspace,
                active_surface,
                active_page,
                overlay,
                derived,
                devices,
                ..
            } = &mut *inner;
            let page = workspace
                .surface(active_surface)
                .and_then(|s| s.page(active_page));
            for (id, caps) in devices.connected_capabilities() {
                // A temporary look over the one the state rules give.
                let state_of = |cid: &str| {
                    let key = control_key(active_surface, active_page, cid);
                    overlay
                        .get(&key)
                        .map(|e| e.name.clone())
                        .or_else(|| derived.get(&key).cloned())
                };
                let frame = renderer::compose(page, &state_of, &caps, t_ms);
                if let Some(change) = devices.render(&id, &frame) {
                    changes.push(change);
                }
            }
        }
        self.announce(changes);
    }

    fn announce(&self, changes: Vec<DeviceChange>) {
        if changes.is_empty() {
            return;
        }
        for change in changes {
            match change {
                DeviceChange::Connected(id) => self.publish(
                    "runtime",
                    types::DEVICE_CONNECTED,
                    json!({ "deviceId": id }),
                ),
                DeviceChange::Disconnected(id) => self.publish(
                    "runtime",
                    types::DEVICE_DISCONNECTED,
                    json!({ "deviceId": id }),
                ),
                // Only the device list changed: the state below carries it.
                DeviceChange::Updated(_) => {}
            }
        }
        self.emit_state();
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.lock().devices.list()
    }

    pub fn connect_device(&self, id: &str) -> Result<(), String> {
        let change = self.lock().devices.connect(id).map_err(sentence)?;
        self.announce(vec![change]);
        Ok(())
    }

    pub fn disconnect_device(&self, id: &str) -> Result<(), String> {
        let change = self.lock().devices.disconnect(id).map_err(sentence)?;
        self.announce(vec![change]);
        Ok(())
    }

    /// The LED brightness in percent of every device that has it, or each
    /// device's own level (`None`): saved as a preference and given to the
    /// connected devices now and to every device when it connects.
    pub fn set_device_brightness(&self, percent: Option<u8>) -> Result<(), String> {
        if percent.is_some_and(|p| p > BRIGHTNESS_MAX) {
            return Err(format!(
                "Brightness goes from 0 to {BRIGHTNESS_MAX} percent."
            ));
        }
        {
            let mut inner = self.lock();
            inner.workspace.preferences.device_brightness = percent;
            self.persist(&inner);
            inner.devices.set_preferred_brightness(percent);
        }
        self.emit_workspace();
        Ok(())
    }

    /// Look for Launchpad X hardware; connect units that are new or came back.
    /// A connect that fails is recorded by the device manager as the unit's
    /// problem.
    pub fn rescan(&self) {
        self.scan_hardware(false);
    }

    /// The person pressed Look again: also retry units whose connect failed
    /// or that turned LaunchDeck down.
    pub fn look_again(&self) {
        self.scan_hardware(true);
    }

    fn scan_hardware(&self, retry_failed: bool) {
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let rescan = inner.devices.rescan_hardware(retry_failed);
            changes.extend(rescan.changes);
            for id in rescan.to_connect {
                if let Ok(change) = inner.devices.connect(&id) {
                    changes.push(change);
                }
            }
        }
        // A newly listed, unplugged or failed device also changes the list.
        self.announce(changes);
        self.emit_state();
    }
}

/// The sentence for the person, then what the system said, until command
/// errors carry their detail separately.
fn sentence(e: DeviceError) -> String {
    match e.detail() {
        Some(detail) => format!("{e} Details: {detail}"),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::Value;

    use super::super::{ui, Runtime, RuntimeConfig, UiEmitter};
    use crate::core::model::Workspace;
    use crate::devices::{
        AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceError, DeviceFeatures,
        DeviceIdentity, DeviceKind, DeviceSetting, InputSender, ModeState, SurfaceDevice,
        SurfaceFrame,
    };
    use crate::events::types;
    use crate::persistence::Store;
    use crate::plugins::keyboard::{Combo, Keyboard};
    use crate::plugins::ActionError;
    use crate::surfaces::default_workspace;

    const ID: &str = "noticing:1";
    const FEATURED: &str = "featured:1";

    /// A device whose details the test changes.
    struct Noticing {
        details: Arc<Mutex<DeviceDetails>>,
        connected: bool,
    }

    impl SurfaceDevice for Noticing {
        fn identity(&self) -> DeviceIdentity {
            DeviceIdentity {
                id: ID.into(),
                name: "Noticing".into(),
                model: "noticing".into(),
                kind: DeviceKind::Hardware,
            }
        }
        fn capabilities(&self) -> DeviceCapabilities {
            DeviceCapabilities {
                grid: None,
                aux: AuxCaps::default(),
                rgb: true,
                velocity: false,
                pressure: false,
                animations: AnimationCaps::default(),
            }
        }
        fn connect(&mut self, _input: InputSender) -> Result<(), DeviceError> {
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), DeviceError> {
            self.connected = false;
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
        fn render(&mut self, _frame: &SurfaceFrame) -> Result<(), DeviceError> {
            Ok(())
        }
        fn clear(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
        fn details(&self) -> DeviceDetails {
            self.details.lock().unwrap().clone()
        }
    }

    /// What a device with every feature was given, in order.
    type Given = Arc<Mutex<Vec<DeviceSetting>>>;

    /// A device with every feature that records what it is given.
    struct Featured {
        given: Given,
        connected: bool,
    }

    impl SurfaceDevice for Featured {
        fn identity(&self) -> DeviceIdentity {
            DeviceIdentity {
                id: FEATURED.into(),
                name: "Featured".into(),
                model: "featured".into(),
                kind: DeviceKind::Hardware,
            }
        }
        fn capabilities(&self) -> DeviceCapabilities {
            DeviceCapabilities {
                grid: None,
                aux: AuxCaps::default(),
                rgb: true,
                velocity: false,
                pressure: false,
                animations: AnimationCaps::default(),
            }
        }
        fn connect(&mut self, _input: InputSender) -> Result<(), DeviceError> {
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), DeviceError> {
            self.connected = false;
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
        fn render(&mut self, _frame: &SurfaceFrame) -> Result<(), DeviceError> {
            Ok(())
        }
        fn clear(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
        fn features(&self) -> DeviceFeatures {
            DeviceFeatures {
                brightness: true,
                sleep: true,
                scrolling_text: true,
                ..DeviceFeatures::default()
            }
        }
        fn apply(&mut self, setting: DeviceSetting) -> Result<(), DeviceError> {
            self.given.lock().unwrap().push(setting);
            Ok(())
        }
    }

    /// Registers and connects a `Featured` device; what it is given.
    fn featured(rt: &Runtime) -> Given {
        let given = Given::default();
        rt.lock().devices.register(Box::new(Featured {
            given: given.clone(),
            connected: false,
        }));
        rt.connect_device(FEATURED).unwrap();
        given
    }

    fn config(
        workspace: Workspace,
        store: Option<Store>,
        emitter: Arc<dyn UiEmitter>,
    ) -> RuntimeConfig {
        RuntimeConfig {
            workspace,
            store,
            emitter,
            keyboard: Arc::new(NoKeyboard),
            scan_hardware: false,
            serve_external: false,
            demo_pace: 1.0,
            plugin_dirs: vec![],
            secrets: None,
            observer: None,
        }
    }

    /// Every state the runtime sends the UI.
    #[derive(Default)]
    struct States(Mutex<Vec<Value>>);

    impl UiEmitter for States {
        fn emit(&self, event: &str, payload: Value) {
            if event == ui::STATE {
                self.0.lock().unwrap().push(payload);
            }
        }
    }

    impl States {
        fn mode_of(&self, id: &str) -> Vec<Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .flat_map(|s| s["devices"].as_array().cloned().unwrap_or_default())
                .filter(|d| d["identity"]["id"] == id)
                .map(|d| d["details"]["mode"].clone())
                .collect()
        }
    }

    struct NoKeyboard;
    impl Keyboard for NoKeyboard {
        fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn changed_details_send_the_state_again_and_publish_no_event() {
        let states = Arc::new(States::default());
        let rt = Runtime::start(config(default_workspace(), None, states.clone())).await;
        let details = Arc::new(Mutex::new(DeviceDetails::default()));
        rt.lock().devices.register(Box::new(Noticing {
            details: details.clone(),
            connected: false,
        }));
        rt.connect_device(ID).unwrap();
        assert_eq!(states.mode_of(ID).last(), Some(&Value::from("none")));

        *details.lock().unwrap() = DeviceDetails {
            mode: ModeState::Confirmed,
            ..DeviceDetails::default()
        };
        // The render loop draws every 40 ms; the next tick sees the change.
        for _ in 0..100 {
            if states.mode_of(ID).last() == Some(&Value::from("confirmed")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(states.mode_of(ID).last(), Some(&Value::from("confirmed")));
        let events: Vec<String> = rt
            .snapshot()
            .recent_events
            .into_iter()
            .filter(|e| e.payload["deviceId"] == ID)
            .map(|e| e.kind)
            .collect();
        assert_eq!(events, [types::DEVICE_CONNECTED], "no event of its own");
    }

    fn on_disk(dir: &std::path::Path) -> Workspace {
        Store::new(dir).load_or_default(|| panic!("the saved file must load"))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn brightness_is_checked_saved_and_given_to_every_device_with_the_feature() {
        let dir = tempfile::tempdir().unwrap();
        let rt = Runtime::start(config(
            default_workspace(),
            Some(Store::new(dir.path())),
            Arc::new(States::default()),
        ))
        .await;
        let given = featured(&rt);
        assert_eq!(*given.lock().unwrap(), [], "no preference, nothing given");

        assert_eq!(
            rt.set_device_brightness(Some(101)).unwrap_err(),
            "Brightness goes from 0 to 100 percent."
        );
        assert_eq!(rt.snapshot().workspace.preferences.device_brightness, None);

        rt.set_device_brightness(Some(50)).unwrap();
        assert_eq!(
            *given.lock().unwrap(),
            [DeviceSetting::Brightness(Some(50))]
        );
        assert_eq!(
            rt.snapshot().workspace.preferences.device_brightness,
            Some(50)
        );
        assert_eq!(on_disk(dir.path()).preferences.device_brightness, Some(50));

        rt.set_device_brightness(None).unwrap();
        assert_eq!(
            given.lock().unwrap().last(),
            Some(&DeviceSetting::Brightness(None))
        );
        assert_eq!(on_disk(dir.path()).preferences.device_brightness, None);
        let saved = std::fs::read_to_string(dir.path().join("workspace.json")).unwrap();
        assert!(!saved.contains("deviceBrightness"), "absent when unset");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_saved_brightness_is_given_to_a_device_when_it_connects() {
        let mut workspace = default_workspace();
        workspace.preferences.device_brightness = Some(30);
        let rt = Runtime::start(config(workspace, None, Arc::new(States::default()))).await;
        let given = featured(&rt);
        assert_eq!(
            *given.lock().unwrap(),
            [DeviceSetting::Brightness(Some(30))]
        );
    }
}
