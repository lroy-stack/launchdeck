//! Devices: drawing the active page on every connected device, the device
//! list (connect, disconnect, rescan for hardware), and what a device can
//! be given beyond frames (brightness, sleep, scrolling text).

use serde_json::json;

use super::{control_key, Inner, Runtime};
use crate::devices::manager::{DeviceChange, DeviceInfo};
use crate::devices::{DeviceError, DeviceSetting, TextScroll};

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

    /// Gives back every connected device: LaunchDeck is quitting. Bounded:
    /// a few messages per device, no waiting for answers.
    pub fn release_devices(&self) {
        self.lock().devices.release_all();
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

    /// Turns a connected device's LEDs off (`asleep`) or on again. Nothing
    /// puts a device to sleep by itself.
    pub fn set_device_sleep(&self, id: &str, asleep: bool) -> Result<(), String> {
        self.lock()
            .devices
            .apply(id, DeviceSetting::Sleep(asleep))
            .map_err(sentence)?;
        self.emit_state();
        Ok(())
    }

    /// Scrolls text across a connected device, or stops the scroll
    /// (`None`). The text is checked against LaunchDeck's limits first.
    pub fn scroll_device_text(&self, id: &str, text: Option<TextScroll>) -> Result<(), String> {
        if let Some(text) = &text {
            text.check()?;
        }
        self.lock()
            .devices
            .scroll_text(id, text.as_ref())
            .map_err(sentence)?;
        self.emit_state();
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
    use crate::core::model::Rgb;
    use crate::core::model::Workspace;
    use crate::devices::{
        AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceError, DeviceFeatures,
        DeviceIdentity, DeviceKind, DeviceSetting, InputSender, ModeState, ScrollDirection,
        SurfaceDevice, SurfaceFrame, TextScroll,
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
    #[derive(Default)]
    struct Record {
        settings: Vec<DeviceSetting>,
        texts: Vec<Option<TextScroll>>,
    }
    type Given = Arc<Mutex<Record>>;

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
            self.given.lock().unwrap().settings.push(setting);
            Ok(())
        }
        fn scroll_text(&mut self, text: Option<&TextScroll>) -> Result<(), DeviceError> {
            self.given.lock().unwrap().texts.push(text.cloned());
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

    #[tokio::test(flavor = "multi_thread")]
    async fn quitting_gives_back_every_connected_device() {
        let rt = Runtime::start(config(
            default_workspace(),
            None,
            Arc::new(States::default()),
        ))
        .await;
        featured(&rt);
        let connected = |rt: &Runtime| {
            rt.list_devices()
                .into_iter()
                .filter(|d| d.connected)
                .count()
        };
        assert!(connected(&rt) > 0);
        rt.release_devices();
        assert_eq!(connected(&rt), 0);
        rt.release_devices();
        assert_eq!(connected(&rt), 0);
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
        assert_eq!(
            given.lock().unwrap().settings,
            [],
            "no preference, nothing given"
        );

        assert_eq!(
            rt.set_device_brightness(Some(101)).unwrap_err(),
            "Brightness goes from 0 to 100 percent."
        );
        assert_eq!(rt.snapshot().workspace.preferences.device_brightness, None);

        rt.set_device_brightness(Some(50)).unwrap();
        assert_eq!(
            given.lock().unwrap().settings,
            [DeviceSetting::Brightness(Some(50))]
        );
        assert_eq!(
            rt.snapshot().workspace.preferences.device_brightness,
            Some(50)
        );
        assert_eq!(on_disk(dir.path()).preferences.device_brightness, Some(50));

        rt.set_device_brightness(None).unwrap();
        assert_eq!(
            given.lock().unwrap().settings.last(),
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
            given.lock().unwrap().settings,
            [DeviceSetting::Brightness(Some(30))]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_connected_device_is_put_to_sleep_and_woken_on_request_only() {
        let rt = Runtime::start(config(
            default_workspace(),
            None,
            Arc::new(States::default()),
        ))
        .await;
        let given = featured(&rt);
        rt.set_device_sleep(FEATURED, true).unwrap();
        rt.set_device_sleep(FEATURED, false).unwrap();
        assert_eq!(
            given.lock().unwrap().settings,
            [DeviceSetting::Sleep(true), DeviceSetting::Sleep(false)]
        );
        assert_eq!(
            rt.set_device_sleep("nope", true).unwrap_err(),
            "LaunchDeck does not know this device. Press Look again. Details: nope"
        );
        rt.disconnect_device(FEATURED).unwrap();
        assert_eq!(
            rt.set_device_sleep(FEATURED, true).unwrap_err(),
            "The device is not connected."
        );
        assert_eq!(given.lock().unwrap().settings.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn text_is_checked_before_it_is_scrolled_and_can_be_stopped() {
        let rt = Runtime::start(config(
            default_workspace(),
            None,
            Arc::new(States::default()),
        ))
        .await;
        let given = featured(&rt);
        let text = |text: &str| TextScroll {
            text: text.into(),
            color: Rgb::new(255, 255, 255),
            speed: 7,
            direction: ScrollDirection::Left,
            looping: true,
        };
        let too_long = "x".repeat(65);
        for refused in [too_long.as_str(), "é", ""] {
            assert_eq!(
                rt.scroll_device_text(FEATURED, Some(text(refused)))
                    .unwrap_err(),
                "Scrolling text takes up to 64 letters, digits and punctuation marks.",
                "{refused:?}"
            );
        }
        let slow = TextScroll {
            speed: 0,
            ..text("Hi")
        };
        assert_eq!(
            rt.scroll_device_text(FEATURED, Some(slow)).unwrap_err(),
            "Scrolling text moves at 1 to 63 pads per second."
        );
        assert!(
            given.lock().unwrap().texts.is_empty(),
            "nothing refused is sent"
        );

        rt.scroll_device_text(FEATURED, Some(text("Hello World")))
            .unwrap();
        rt.scroll_device_text(FEATURED, None).unwrap();
        assert_eq!(
            given.lock().unwrap().texts,
            [Some(text("Hello World")), None]
        );
        assert_eq!(
            rt.scroll_device_text("nope", None).unwrap_err(),
            "LaunchDeck does not know this device. Press Look again. Details: nope"
        );
    }
}
