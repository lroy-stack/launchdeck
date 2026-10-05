//! `VirtualDevice`: a software control surface with the same device
//! interface as the hardware, built from a `VirtualModel`. Used for
//! development without hardware, CI, demos and tests. `VirtualLaunchpadX`
//! is the app's simulator.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceError, DeviceFeatures, DeviceIdentity,
    DeviceInput, DeviceKind, GridCaps, InputKind, InputSender, Led, LedCell, SurfaceDevice,
    SurfaceFrame,
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
}

impl State {
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
        if s.held.remove(&pos) {
            let _ = tx.send(DeviceInput {
                device_id: s.id.clone(),
                pos,
                kind: InputKind::Released,
            });
        }
        Ok(())
    }

    pub fn frame(&self) -> SurfaceFrame {
        self.0.lock().unwrap().frame.clone()
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
            if let Some(cb) = &s.on_change {
                cb(&s.frame);
            }
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DeviceError> {
        self.render(&SurfaceFrame::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Animation, Rgb};
    use crate::devices::{Led, LedCell};
    use tokio::sync::mpsc::unbounded_channel;

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
}
