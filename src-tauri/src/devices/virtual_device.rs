//! `VirtualDevice`: a software control surface with the same device
//! interface as the hardware, built from a `VirtualModel`. Used for
//! development without hardware, CI, demos and tests. `VirtualLaunchpadX`
//! is the app's simulator; `novation::virtual_model` gives a Novation
//! model's. A model with the features records brightness, sleep and the
//! text it scrolls as the hardware adapter does: asleep it shows nothing,
//! and the press that wakes it, with its release, is not input.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceError, DeviceFeatures,
    DeviceIdentity, DeviceInput, DeviceKind, DeviceSetting, GridCaps, InputKind, InputSender, Led,
    LedCell, SurfaceDevice, SurfaceFrame, TextScroll,
};
use crate::core::model::ControlPos;

pub const VIRTUAL_ID: &str = "virtual-launchpad-x";

type ChangeCallback = Box<dyn Fn(&SurfaceFrame) + Send + Sync>;

/// What a virtual device is: its identity, what it can do, and how one of
/// its LEDs shows what a frame asks for.
pub(crate) struct VirtualModel {
    pub id: String,
    pub name: String,
    pub model: String,
    pub capabilities: DeviceCapabilities,
    pub features: DeviceFeatures,
    /// What an LED shows for the `Led` a frame gives it.
    pub display: fn(Led) -> Led,
}

struct State {
    id: String,
    capabilities: DeviceCapabilities,
    connected: bool,
    input: Option<InputSender>,
    /// What the LEDs show: the last frame, as `display` shows it.
    frame: SurfaceFrame,
    held: HashSet<ControlPos>,
    on_change: Option<ChangeCallback>,
    /// The LEDs are off until a press or a setting wakes them.
    asleep: bool,
    /// The control whose press woke the device, until its release.
    waking: Option<ControlPos>,
    /// Percent; `None` is the device's own level.
    brightness: Option<u8>,
    text: Option<TextScroll>,
}

impl State {
    /// What the LEDs show: nothing while asleep.
    fn shown(&self) -> SurfaceFrame {
        if self.asleep {
            SurfaceFrame::default()
        } else {
            self.frame.clone()
        }
    }

    fn notify(&self) {
        if let Some(cb) = &self.on_change {
            cb(&self.shown());
        }
    }

    fn sleep(&mut self) {
        if !self.asleep {
            self.asleep = true;
            self.notify();
        }
    }

    fn wake(&mut self) {
        if self.asleep {
            self.asleep = false;
            self.notify();
        }
    }

    fn sender(&self) -> Result<InputSender, DeviceError> {
        self.input
            .clone()
            .filter(|_| self.connected)
            .ok_or(DeviceError::NotConnected)
    }

    /// Full for a device whose pads send no velocity.
    fn velocity(&self, velocity: u8) -> u8 {
        if self.capabilities.velocity {
            velocity.clamp(1, 127)
        } else {
            127
        }
    }
}

/// Cheap clonable handle for UI/tests to press pads and read the frame.
#[derive(Clone)]
pub struct VirtualHandle(Arc<Mutex<State>>);

impl VirtualHandle {
    pub fn press(&self, pos: ControlPos, velocity: u8) -> Result<(), DeviceError> {
        let mut s = self.0.lock().unwrap();
        let tx = s.sender()?;
        if !s.capabilities.has_position(pos) {
            return Ok(());
        }
        if s.asleep {
            s.wake();
            s.waking = Some(pos);
            return Ok(());
        }
        s.held.insert(pos);
        let _ = tx.send(DeviceInput {
            device_id: s.id.clone(),
            pos,
            kind: InputKind::Pressed {
                velocity: s.velocity(velocity),
            },
        });
        Ok(())
    }

    pub fn release(&self, pos: ControlPos) -> Result<(), DeviceError> {
        let mut s = self.0.lock().unwrap();
        let tx = s.sender()?;
        if s.waking == Some(pos) {
            s.waking = None;
            return Ok(());
        }
        if s.held.remove(&pos) {
            let _ = tx.send(DeviceInput {
                device_id: s.id.clone(),
                pos,
                kind: InputKind::Released,
            });
        }
        Ok(())
    }

    /// What the LEDs show: all off while asleep.
    pub fn frame(&self) -> SurfaceFrame {
        self.0.lock().unwrap().shown()
    }

    /// The text the device is scrolling.
    pub fn text(&self) -> Option<TextScroll> {
        self.0.lock().unwrap().text.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.0.lock().unwrap().connected
    }

    pub fn set_on_change(&self, cb: impl Fn(&SurfaceFrame) + Send + Sync + 'static) {
        self.0.lock().unwrap().on_change = Some(Box::new(cb));
    }
}

/// A software device of one `VirtualModel`.
pub struct VirtualDevice {
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
    features: DeviceFeatures,
    display: fn(Led) -> Led,
    handle: VirtualHandle,
}

impl VirtualDevice {
    pub(crate) fn new(model: VirtualModel) -> (Self, VirtualHandle) {
        let handle = VirtualHandle(Arc::new(Mutex::new(State {
            id: model.id.clone(),
            capabilities: model.capabilities,
            connected: false,
            input: None,
            frame: SurfaceFrame::default(),
            held: HashSet::new(),
            on_change: None,
            asleep: false,
            waking: None,
            brightness: None,
            text: None,
        })));
        let device = Self {
            identity: DeviceIdentity {
                id: model.id,
                name: model.name,
                model: model.model,
                kind: DeviceKind::Virtual,
            },
            capabilities: model.capabilities,
            features: model.features,
            display: model.display,
            handle: handle.clone(),
        };
        (device, handle)
    }

    /// The frame as this device's LEDs show it.
    fn shown(&self, frame: &SurfaceFrame) -> SurfaceFrame {
        SurfaceFrame {
            leds: frame
                .leds
                .iter()
                .map(|cell| LedCell {
                    pos: cell.pos,
                    led: (self.display)(cell.led),
                })
                .collect(),
        }
    }
}

/// The app's simulator: a `VirtualDevice` of the Launchpad X.
pub struct VirtualLaunchpadX;

impl VirtualLaunchpadX {
    #[expect(
        clippy::new_ret_no_self,
        reason = "the simulator's constructor, as every caller knows it"
    )]
    pub fn new() -> (VirtualDevice, VirtualHandle) {
        VirtualDevice::new(VirtualModel {
            id: VIRTUAL_ID.into(),
            name: "Virtual Launchpad X".into(),
            model: "launchpad-x".into(),
            capabilities: launchpad_x_caps(),
            features: DeviceFeatures::default(),
            display: |led| led,
        })
    }
}

fn launchpad_x_caps() -> DeviceCapabilities {
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
        // The virtual surface animates natively in the UI.
        animations: AnimationCaps {
            pulse: true,
            flash: true,
        },
    }
}

impl SurfaceDevice for VirtualDevice {
    fn identity(&self) -> DeviceIdentity {
        self.identity.clone()
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.capabilities
    }

    fn features(&self) -> DeviceFeatures {
        self.features
    }

    fn connect(&mut self, input: InputSender) -> Result<(), DeviceError> {
        let mut s = self.handle.0.lock().unwrap();
        s.input = Some(input);
        s.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), DeviceError> {
        let mut s = self.handle.0.lock().unwrap();
        s.connected = false;
        s.input = None;
        s.held.clear();
        s.frame = SurfaceFrame::default();
        s.asleep = false;
        s.waking = None;
        s.text = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.handle.is_connected()
    }

    fn render(&mut self, frame: &SurfaceFrame) -> Result<(), DeviceError> {
        let shown = self.shown(frame);
        let mut s = self.handle.0.lock().unwrap();
        if !s.connected {
            return Err(DeviceError::NotConnected);
        }
        if s.frame != shown {
            s.frame = shown;
            if !s.asleep {
                s.notify();
            }
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DeviceError> {
        self.render(&SurfaceFrame::default())
    }

    fn details(&self) -> DeviceDetails {
        let s = self.handle.0.lock().unwrap();
        DeviceDetails {
            brightness: s.brightness,
            asleep: s.asleep,
            scrolling: s.text.is_some(),
            ..DeviceDetails::default()
        }
    }

    /// A model without the feature refuses as `SurfaceDevice` does by
    /// default, connected or not.
    fn apply(&mut self, setting: DeviceSetting) -> Result<(), DeviceError> {
        let available = match setting {
            DeviceSetting::Brightness(_) => self.features.brightness,
            DeviceSetting::Sleep(_) => self.features.sleep,
        };
        if !available {
            return Err(DeviceError::Unavailable(format!("{setting:?}")));
        }
        let mut s = self.handle.0.lock().unwrap();
        if !s.connected {
            return Err(DeviceError::NotConnected);
        }
        match setting {
            DeviceSetting::Brightness(percent) => s.brightness = percent.map(|p| p.min(100)),
            DeviceSetting::Sleep(true) => s.sleep(),
            DeviceSetting::Sleep(false) => s.wake(),
        }
        Ok(())
    }

    fn scroll_text(&mut self, text: Option<&TextScroll>) -> Result<(), DeviceError> {
        if !self.features.scrolling_text {
            return Err(DeviceError::Unavailable("scrolling text".into()));
        }
        let mut s = self.handle.0.lock().unwrap();
        if !s.connected {
            return Err(DeviceError::NotConnected);
        }
        s.text = text.cloned();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Animation, Rgb};
    use crate::devices::novation::{virtual_model, LAUNCHPAD_MINI_MK3};
    use crate::devices::{DeviceDetails, DeviceSetting, Led, LedCell, ScrollDirection, TextScroll};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn frame_with(pos: ControlPos, color: Rgb, animation: Animation) -> SurfaceFrame {
        SurfaceFrame {
            leds: vec![LedCell {
                pos,
                led: Led { color, animation },
            }],
        }
    }

    #[test]
    fn press_release_flow_through_the_same_interface() {
        let (mut dev, handle) = VirtualLaunchpadX::new();
        let (tx, mut rx) = unbounded_channel();
        assert!(
            handle.press(ControlPos::grid(0, 0), 100).is_err(),
            "not connected yet"
        );
        dev.connect(tx).unwrap();
        handle.press(ControlPos::grid(3, 4), 90).unwrap();
        handle.release(ControlPos::grid(3, 4)).unwrap();
        let down = rx.try_recv().unwrap();
        assert_eq!(down.pos, ControlPos::grid(3, 4));
        assert_eq!(down.kind, InputKind::Pressed { velocity: 90 });
        assert_eq!(rx.try_recv().unwrap().kind, InputKind::Released);
        // Releasing something that was never pressed is a no-op.
        handle.release(ControlPos::grid(1, 1)).unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn colors_and_animations_are_stored_and_reported_once() {
        let (mut dev, handle) = VirtualLaunchpadX::new();
        let (tx, _rx) = unbounded_channel();
        dev.connect(tx).unwrap();
        let calls = Arc::new(Mutex::new(0u32));
        let c = calls.clone();
        handle.set_on_change(move |_| *c.lock().unwrap() += 1);
        let f = frame_with(
            ControlPos::grid(0, 0),
            Rgb::new(0, 255, 0),
            Animation::Pulse,
        );
        dev.render(&f).unwrap();
        dev.render(&f).unwrap();
        assert_eq!(*calls.lock().unwrap(), 1, "identical frames do not notify");
        assert_eq!(
            handle
                .frame()
                .get(ControlPos::grid(0, 0))
                .unwrap()
                .animation,
            Animation::Pulse
        );
        dev.clear().unwrap();
        assert!(handle.frame().leds.is_empty());
    }

    #[test]
    fn disconnect_resets_state_and_blocks_rendering() {
        let (mut dev, handle) = VirtualLaunchpadX::new();
        let (tx, _rx) = unbounded_channel();
        dev.connect(tx).unwrap();
        dev.disconnect().unwrap();
        assert!(!handle.is_connected());
        assert!(matches!(
            dev.render(&SurfaceFrame::default()),
            Err(DeviceError::NotConnected)
        ));
    }

    #[test]
    fn advertises_a_full_launchpad_x_capability_set() {
        let (dev, _h) = VirtualLaunchpadX::new();
        let caps = dev.capabilities();
        assert_eq!(caps.positions().len(), 81);
        assert!(caps.rgb && caps.animations.pulse && caps.animations.flash);
    }

    fn mini() -> (VirtualDevice, VirtualHandle, UnboundedReceiver<DeviceInput>) {
        let (mut dev, handle) = VirtualDevice::new(virtual_model(&LAUNCHPAD_MINI_MK3));
        let (tx, rx) = unbounded_channel();
        dev.connect(tx).unwrap();
        (dev, handle, rx)
    }

    fn scroll(text: &str) -> TextScroll {
        TextScroll {
            text: text.into(),
            color: Rgb::new(255, 255, 255),
            speed: 7,
            direction: ScrollDirection::Left,
            looping: true,
        }
    }

    #[test]
    fn the_virtual_x_is_the_simulator_it_always_was() {
        let (mut dev, handle) = VirtualLaunchpadX::new();
        assert_eq!(
            dev.identity(),
            DeviceIdentity {
                id: VIRTUAL_ID.into(),
                name: "Virtual Launchpad X".into(),
                model: "launchpad-x".into(),
                kind: DeviceKind::Virtual,
            }
        );
        assert!(dev.capabilities().velocity);
        assert_eq!(dev.features(), DeviceFeatures::default());
        let (tx, mut rx) = unbounded_channel();
        dev.connect(tx).unwrap();
        // Animations stay as they are: the UI draws them.
        let pulse = frame_with(
            ControlPos::grid(0, 0),
            Rgb::new(40, 110, 255),
            Animation::Pulse,
        );
        dev.render(&pulse).unwrap();
        assert_eq!(handle.frame(), pulse);
        handle.press(ControlPos::grid(1, 1), 0).unwrap();
        let down = rx.try_recv().unwrap();
        assert_eq!(down.device_id, VIRTUAL_ID);
        assert_eq!(down.kind, InputKind::Pressed { velocity: 1 });
    }

    #[test]
    fn the_virtual_x_refuses_settings_and_text() {
        let (mut dev, handle) = VirtualLaunchpadX::new();
        let (tx, _rx) = unbounded_channel();
        dev.connect(tx).unwrap();
        for setting in [
            DeviceSetting::Brightness(Some(50)),
            DeviceSetting::Sleep(true),
        ] {
            assert!(matches!(
                dev.apply(setting),
                Err(DeviceError::Unavailable(_))
            ));
        }
        assert!(matches!(
            dev.scroll_text(Some(&scroll("Hi"))),
            Err(DeviceError::Unavailable(_))
        ));
        assert_eq!(dev.details(), DeviceDetails::default());
        assert_eq!(handle.text(), None);
    }

    #[test]
    fn the_virtual_mini_reports_its_id_and_full_presses() {
        let (dev, handle, mut rx) = mini();
        assert_eq!(dev.identity().id, "virtual-launchpad-mini-mk3");
        assert_eq!(dev.identity().kind, DeviceKind::Virtual);
        handle.press(ControlPos::grid(2, 5), 40).unwrap();
        handle.release(ControlPos::grid(2, 5)).unwrap();
        let down = rx.try_recv().unwrap();
        assert_eq!(down.device_id, "virtual-launchpad-mini-mk3");
        assert_eq!(down.kind, InputKind::Pressed { velocity: 127 });
        assert_eq!(rx.try_recv().unwrap().kind, InputKind::Released);
    }

    #[test]
    fn the_virtual_mini_shows_a_pulse_in_the_palette_and_a_static_colour_exactly() {
        let (mut dev, handle, _rx) = mini();
        let blue = Rgb::new(40, 110, 255);
        dev.render(&frame_with(ControlPos::grid(0, 0), blue, Animation::Pulse))
            .unwrap();
        let shown = handle.frame().get(ControlPos::grid(0, 0)).unwrap();
        assert_eq!(shown.animation, Animation::Pulse);
        assert_ne!(shown.color, blue, "no palette entry is exactly this blue");
        dev.render(&frame_with(ControlPos::grid(0, 0), blue, Animation::None))
            .unwrap();
        assert_eq!(
            handle.frame().get(ControlPos::grid(0, 0)),
            Some(Led {
                color: blue,
                animation: Animation::None
            })
        );
    }

    #[test]
    fn asleep_the_virtual_mini_shows_nothing_and_the_waking_press_is_not_input() {
        let (mut dev, handle, mut rx) = mini();
        let shown = Arc::new(Mutex::new(Vec::new()));
        let seen = shown.clone();
        handle.set_on_change(move |frame| seen.lock().unwrap().push(frame.clone()));
        let red = frame_with(ControlPos::grid(0, 0), Rgb::new(255, 0, 0), Animation::None);
        let green = frame_with(ControlPos::grid(1, 1), Rgb::new(0, 255, 0), Animation::None);
        dev.render(&red).unwrap();
        dev.apply(DeviceSetting::Sleep(true)).unwrap();
        assert!(dev.details().asleep);
        assert_eq!(handle.frame(), SurfaceFrame::default());
        dev.render(&green).unwrap();
        assert_eq!(handle.frame(), SurfaceFrame::default(), "drawn, not shown");

        handle.press(ControlPos::grid(3, 3), 127).unwrap();
        handle.release(ControlPos::grid(3, 3)).unwrap();
        assert!(rx.try_recv().is_err(), "the waking press and its release");
        assert!(!dev.details().asleep);
        assert_eq!(handle.frame(), green);
        assert_eq!(
            *shown.lock().unwrap(),
            [red, SurfaceFrame::default(), green.clone()]
        );

        handle.press(ControlPos::grid(3, 3), 127).unwrap();
        assert_eq!(rx.try_recv().unwrap().pos, ControlPos::grid(3, 3));
    }

    #[test]
    fn the_virtual_mini_wakes_when_told_to() {
        let (mut dev, handle, mut rx) = mini();
        let red = frame_with(ControlPos::grid(0, 0), Rgb::new(255, 0, 0), Animation::None);
        dev.render(&red).unwrap();
        dev.apply(DeviceSetting::Sleep(true)).unwrap();
        dev.apply(DeviceSetting::Sleep(false)).unwrap();
        assert_eq!(handle.frame(), red);
        handle.press(ControlPos::grid(3, 3), 127).unwrap();
        assert!(rx.try_recv().is_ok(), "awake, a press is input");
    }

    #[test]
    fn the_virtual_mini_records_brightness_and_text_in_its_details() {
        let (mut dev, handle, _rx) = mini();
        assert_eq!(dev.details(), DeviceDetails::default());
        dev.apply(DeviceSetting::Brightness(Some(40))).unwrap();
        dev.scroll_text(Some(&scroll("Hello"))).unwrap();
        assert_eq!(
            dev.details(),
            DeviceDetails {
                brightness: Some(40),
                scrolling: true,
                ..DeviceDetails::default()
            }
        );
        assert_eq!(handle.text(), Some(scroll("Hello")));

        dev.scroll_text(None).unwrap();
        dev.apply(DeviceSetting::Brightness(None)).unwrap();
        assert_eq!(dev.details(), DeviceDetails::default());
        assert_eq!(handle.text(), None);
    }

    #[test]
    fn the_virtual_mini_forgets_sleep_and_text_on_disconnect() {
        let (mut dev, handle, _rx) = mini();
        dev.apply(DeviceSetting::Sleep(true)).unwrap();
        dev.scroll_text(Some(&scroll("Bye"))).unwrap();
        dev.disconnect().unwrap();
        assert!(!dev.details().asleep && !dev.details().scrolling);
        assert_eq!(handle.text(), None);
        assert!(matches!(
            dev.apply(DeviceSetting::Sleep(true)),
            Err(DeviceError::NotConnected)
        ));
        assert!(matches!(
            dev.scroll_text(None),
            Err(DeviceError::NotConnected)
        ));
    }
}
