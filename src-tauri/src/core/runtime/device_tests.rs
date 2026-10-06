//! A Launchpad Mini MK3, a Launchpad X and a virtual Mini MK3 driven through
//! the real runtime (LAUNCHPAD_MINI_MK3_PLAN.md section 14.4).
//!
//! Everything but the units is the app's: the runtime, its render loop, the
//! renderer, actions, state feedback, Auto Context (with a scripted
//! observer) and the device manager, which finds the units through the real
//! `NovationHardware` on fake ports named as macOS names them. Behind the
//! ports is a `SimLaunchpad`, a unit written from the manuals. The render
//! loop runs, so every check waits for what it expects, two seconds at most.
//! The times printed are host-side over the fake transport, not device
//! latency. NOT HARDWARE VERIFIED: no Launchpad was attached.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::broadcast::{error::RecvError, Receiver};

use super::{control_key, NullEmitter, Runtime, RuntimeConfig};
use crate::actions::builtin::{NAVIGATION, OPEN_SURFACE};
use crate::context::identity::X11;
use crate::context::observer::scripted::{Script, ScriptedObserver};
use crate::context::{
    ApplicationIdentity, ContextRule, ContextSettings, Fallback, ObserverStatus, Report, Seen,
};
use crate::core::model::{
    Animation, Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual, Workspace,
    SCHEMA_VERSION,
};
use crate::devices::manager::DeviceStatus;
use crate::devices::testkit::{
    nearest, palette_rgb, port, to_7bit, virtual_model, FakeMidi, SimLaunchpad, SimLed,
    LAUNCHPAD_MINI_MK3,
};
use crate::devices::virtual_device::VirtualDevice;
use crate::devices::{DeviceCapabilities, Led, SurfaceFrame};
use crate::events::{types, Event};
use crate::plugins::keyboard::{Combo, Keyboard};
use crate::plugins::ActionError;
use crate::state::feedback::{FeedbackRule, Test};
use crate::state::StateAddress;

/// The longest any check waits for the render loop or the input loop.
const WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

const DESK: &str = "desk";
const OTHER: &str = "other";
const HOME: &str = "home";
/// The pad that starts the demo agent and shows what it does.
const ALPHA: ControlPos = ControlPos::grid(0, 0);
/// The pad on the desk that opens the other surface, and the one back.
const NEXT: ControlPos = ControlPos::grid(7, 7);
const BACK: ControlPos = ControlPos::grid(0, 7);
const GREEN: Rgb = Rgb::new(30, 220, 90);
/// The application whose rule shows the other surface.
const EDITOR: &str = "Code";
/// A layout the person may choose on the unit (MINI-PRM p7, X-PRM p7).
const CUSTOM_1: u8 = 0x04;
const VIRTUAL_MINI: &str = "virtual-launchpad-mini-mk3";

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

fn surface(id: &str, name: &str, controls: Vec<Control>) -> Surface {
    Surface {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        icon: None,
        category: "custom".into(),
        pages: vec![Page {
            id: HOME.into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
        imported_from: None,
    }
}

/// A pad that opens a surface.
fn opener(pos: ControlPos, label: &str, color: Rgb, surface: &str) -> Control {
    let mut control = Control::new(pos, label, color);
    control.press = Some(Assignment::new(
        NAVIGATION,
        OPEN_SURFACE,
        json!({ "surfaceId": surface }),
    ));
    control
}

/// The desk: a pad that starts the demo agent `alpha` and mirrors it
/// (running pulses green), and a pad that opens the other surface, which
/// has a pad back and a pad of its own.
fn workspace() -> Workspace {
    let mut alpha = Control::new(ALPHA, "Alpha", Rgb::new(40, 110, 255));
    alpha.press = Some(Assignment::new(
        "demo",
        "startTask",
        json!({ "agent": "alpha" }),
    ));
    alpha.states.insert("running".into(), Visual::pulse(GREEN));
    alpha.feedback = vec![
        FeedbackRule::state(StateAddress::new("demo", "alpha"), Test::HasValue).show_from_value(),
    ];
    let desk = surface(
        DESK,
        "Desk",
        vec![alpha, opener(NEXT, "Other", Rgb::new(0, 200, 200), OTHER)],
    );
    let other = surface(
        OTHER,
        "Other",
        vec![
            opener(BACK, "Back", Rgb::new(255, 140, 0), DESK),
            Control::new(ControlPos::grid(3, 3), "Note", Rgb::new(255, 220, 0)),
        ],
    );
    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: vec![desk, other],
        last_active_surface: Some(DESK.into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

fn editor() -> ApplicationIdentity {
    ApplicationIdentity::new(X11, EDITOR, EDITOR)
}

/// The runtime on the desk, with Auto Context on and one rule: the editor
/// shows the other surface. `script` says which application is in front.
struct Desk {
    rt: Arc<Runtime>,
    script: Script,
}

async fn desk() -> Desk {
    let (observer, script) = ScriptedObserver::new(ObserverStatus::available("test"));
    let rt = Runtime::start(RuntimeConfig {
        workspace: workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: Some(Box::new(observer)),
    })
    .await;
    rt.update_context(ContextSettings {
        auto: true,
        rules: vec![ContextRule {
            application: editor(),
            surface_id: OTHER.into(),
            page_id: None,
        }],
        fallback: Fallback::Keep,
    })
    .unwrap();
    Desk { rt, script }
}

impl Desk {
    /// The editor comes to the front.
    fn bring_editor_forward(&self) {
        self.script.report(Report::Seen(Seen::Application {
            identity: editor(),
            own: false,
        }));
    }
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let began = Instant::now();
    while began.elapsed() < WAIT {
        if cond() {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    assert!(cond(), "timed out waiting for: {what}");
}

/// The next event of this kind about this device on the bus.
async fn next_event(events: &mut Receiver<Event>, kind: &str, device: &str) -> Event {
    let found = tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Ok(e) if e.kind == kind && e.payload["deviceId"] == device => return e,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("the bus closed"),
            }
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("no {kind} for {device} within {WAIT:?}"))
}

fn status_of(rt: &Runtime, id: &str) -> Option<DeviceStatus> {
    rt.list_devices()
        .into_iter()
        .find(|d| d.identity.id == id)
        .map(|d| d.status)
}

fn capabilities(rt: &Runtime, id: &str) -> DeviceCapabilities {
    rt.list_devices()
        .into_iter()
        .find(|d| d.identity.id == id)
        .map(|d| d.capabilities)
        .unwrap_or_else(|| panic!("{id} is not listed"))
}

/// The look a pad of the desk or the other surface has now.
fn look(rt: &Runtime, surface: &str, control: &str) -> Option<String> {
    rt.state()
        .control_states
        .get(&control_key(surface, HOME, control))
        .cloned()
}

// ---- Launchpads on fake ports ------------------------------------------------

/// One Launchpad on fake ports with the names macOS gives it: the DAW
/// interface's ports first, then the MIDI interface's, which LaunchDeck
/// drives. Port names derived from docs and community projects (the
/// fixtures M1 and M4 of section 14.3), not observed.
struct Unit {
    id: &'static str,
    sim: SimLaunchpad,
    inputs: [(&'static str, &'static str); 2],
    outputs: [(&'static str, &'static str); 2],
    /// It pulses and flashes by itself; otherwise the renderer animates in RGB.
    native: bool,
}

impl Unit {
    fn mini() -> Unit {
        Unit {
            id: "launchpad-mini-mk3:1",
            sim: SimLaunchpad::mini(),
            inputs: [
                ("Launchpad Mini MK3 LPMiniMK3 DAW Out", "101"),
                ("Launchpad Mini MK3 LPMiniMK3 MIDI Out", "102"),
            ],
            outputs: [
                ("Launchpad Mini MK3 LPMiniMK3 DAW In", "103"),
                ("Launchpad Mini MK3 LPMiniMK3 MIDI In", "104"),
            ],
            native: true,
        }
    }

    fn x() -> Unit {
        Unit {
            id: "launchpad-x:1",
            sim: SimLaunchpad::x(),
            inputs: [
                ("Launchpad X LPX DAW Out", "201"),
                ("Launchpad X LPX MIDI Out", "202"),
            ],
            outputs: [
                ("Launchpad X LPX DAW In", "203"),
                ("Launchpad X LPX MIDI In", "204"),
            ],
            native: false,
        }
    }

    fn midi_in(&self) -> &str {
        self.inputs[1].1
    }

    fn midi_out(&self) -> &str {
        self.outputs[1].1
    }

    /// Its ports appear.
    fn plug(&self, midi: &FakeMidi) {
        let list = |ports: &[(&str, &str); 2]| {
            ports
                .iter()
                .map(|(name, id)| port(name, id))
                .collect::<Vec<_>>()
        };
        midi.set_ports(&list(&self.inputs), &list(&self.outputs));
    }

    /// A press and a release of a control, as the unit sends them.
    fn tap(&self, midi: &FakeMidi, pos: ControlPos) {
        midi.inject(self.midi_in(), &self.sim.press(pos));
        midi.inject(self.midi_in(), &self.sim.release(pos));
    }

    /// The positions whose LED the renderer animates for this unit: its
    /// frame changes every tick there, so they are not compared.
    fn moving(&self) -> &'static [ControlPos] {
        if self.native {
            &[]
        } else {
            &[ALPHA]
        }
    }
}

/// What a unit shows for a frame's LED (section 8.2): a static colour as
/// exact 7-bit RGB, a native pulse or flash as the nearest palette entry,
/// a flash alternating with off, off as RGB off.
fn shown_on_unit(led: Led) -> SimLed {
    let c = led.color;
    if c == Rgb::OFF {
        return SimLed::Rgb(0, 0, 0);
    }
    match led.animation {
        Animation::None => SimLed::Rgb(to_7bit(c.r), to_7bit(c.g), to_7bit(c.b)),
        Animation::Pulse => SimLed::Pulse(nearest(c)),
        Animation::Flash => SimLed::Flash {
            b: nearest(c),
            a: 0,
        },
    }
}

/// Where the unit differs from the frame the render loop gives it now,
/// `moving` positions aside. A unit without native animation must show
/// RGB everywhere: it is never sent a palette entry.
fn differences(rt: &Runtime, unit: &Unit, moving: &[ControlPos]) -> Vec<String> {
    let frame = rt.frame_for(&capabilities(rt, unit.id));
    let mut differ: Vec<String> = frame
        .leds
        .iter()
        .filter(|cell| !moving.contains(&cell.pos))
        .filter_map(|cell| {
            let want = shown_on_unit(cell.led);
            let got = unit.sim.led_at(cell.pos);
            (got != Some(want)).then(|| format!("{:?}: {got:?}, not {want:?}", cell.pos))
        })
        .collect();
    if !unit.native {
        differ.extend(
            frame
                .leds
                .iter()
                .filter(|cell| !matches!(unit.sim.led_at(cell.pos), Some(SimLed::Rgb(..))))
                .map(|cell| format!("{:?}: not RGB on a unit without palette", cell.pos)),
        );
    }
    if !unit.sim.in_programmer_mode() {
        differ.push(format!(
            "layout {:02X}h, not Programmer mode",
            unit.sim.layout()
        ));
    }
    differ
}

/// Waits until the unit is in Programmer mode and shows the frame.
async fn shows_the_frame(rt: &Runtime, unit: &Unit, moving: &[ControlPos]) {
    let began = Instant::now();
    while began.elapsed() < WAIT {
        if differences(rt, unit, moving).is_empty() {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    let differ = differences(rt, unit, moving);
    assert!(differ.is_empty(), "{} shows {differ:#?}", unit.id);
}

/// Look again, a press, the agent's pulse, Auto Context, the navigation
/// pad, an unplug, a power-cycled unit plugged in again, and quitting.
async fn drive(unit: Unit) {
    let desk = desk().await;
    let rt = &desk.rt;
    let midi = FakeMidi::default();
    unit.plug(&midi);
    midi.bind(unit.midi_out(), unit.midi_in(), unit.sim.clone());
    rt.use_fake_ports(&midi);
    let mut events = rt.bus().subscribe();

    // Look again: found on its MIDI interface, taken, drawn.
    rt.look_again();
    next_event(&mut events, types::DEVICE_CONNECTED, unit.id).await;
    assert_eq!(status_of(rt, unit.id), Some(DeviceStatus::Connected));
    shows_the_frame(rt, &unit, &[]).await;

    // A press on the unit is a press on the pad: the agent runs and its
    // pad pulses green, by the unit itself where it can.
    let began = Instant::now();
    unit.tap(&midi, ALPHA);
    let pressed = next_event(&mut events, types::CONTROL_PRESSED, unit.id).await;
    let press_to_event = began.elapsed();
    assert_eq!(pressed.payload["surfaceId"], DESK);
    assert_eq!(pressed.payload["controlId"], "g.0.0");
    assert_eq!(pressed.payload["velocity"], 127);
    wait_until("alpha running", || {
        look(rt, DESK, "g.0.0").as_deref() == Some("running")
    })
    .await;
    if unit.native {
        wait_until("a native green pulse", || {
            unit.sim.led_at(ALPHA) == Some(SimLed::Pulse(nearest(GREEN)))
        })
        .await;
    } else {
        wait_until(
            "a green pulse drawn in RGB",
            || matches!(unit.sim.led_at(ALPHA), Some(SimLed::Rgb(r, g, b)) if g > r && g > b),
        )
        .await;
    }
    shows_the_frame(rt, &unit, unit.moving()).await;

    // The editor comes to the front: Auto Context shows the other surface.
    desk.bring_editor_forward();
    wait_until("the other surface", || {
        rt.state().active_surface_id == OTHER
    })
    .await;
    shows_the_frame(rt, &unit, unit.moving()).await;
    wait_until("nothing where alpha was", || {
        unit.sim.led_at(ALPHA) == Some(SimLed::Rgb(0, 0, 0))
    })
    .await;

    // The unit's navigation pad goes back.
    unit.tap(&midi, BACK);
    wait_until("the desk again", || rt.state().active_surface_id == DESK).await;
    shows_the_frame(rt, &unit, unit.moving()).await;

    // Unplugged: reported once, listed as unplugged, its ports closed.
    midi.set_ports(&[], &[]);
    rt.rescan();
    next_event(&mut events, types::DEVICE_DISCONNECTED, unit.id).await;
    assert_eq!(status_of(rt, unit.id), Some(DeviceStatus::Unplugged));
    assert_eq!(midi.open_links(), 0);

    // Power-cycled while away, and the person picks Custom mode 1 on it
    // before plugging it in again: taken again and drawn whole.
    unit.sim.power_cycle();
    let _ = unit.sim.clone().with_layout(CUSTOM_1);
    unit.plug(&midi);
    let began = Instant::now();
    rt.rescan();
    next_event(&mut events, types::DEVICE_CONNECTED, unit.id).await;
    shows_the_frame(rt, &unit, unit.moving()).await;
    let replug_to_restored = began.elapsed();
    assert_eq!(status_of(rt, unit.id), Some(DeviceStatus::Connected));

    // Quitting gives it back dark, in the layout it showed.
    rt.release_devices();
    assert!(!unit.sim.in_programmer_mode());
    assert_eq!(unit.sim.layout(), CUSTOM_1);
    for pos in capabilities(rt, unit.id).positions() {
        assert_eq!(unit.sim.led_at(pos), Some(SimLed::Rgb(0, 0, 0)), "{pos:?}");
    }
    assert_eq!(status_of(rt, unit.id), Some(DeviceStatus::Available));
    assert_eq!(midi.open_links(), 0);

    let outputs: Vec<String> = midi.take_sent().into_iter().map(|(id, _)| id).collect();
    assert!(!outputs.is_empty());
    assert!(
        outputs.iter().all(|id| id == unit.midi_out()),
        "nothing is sent to the DAW interface"
    );
    println!(
        "{}: press to control.pressed {press_to_event:?}; replug and rescan to the frame \
         restored {replug_to_restored:?} (host-side, fake transport, polled every {POLL:?})",
        unit.id
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launchpad_mini_mk3_is_driven_through_the_runtime() {
    drive(Unit::mini()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launchpad_x_is_driven_through_the_runtime_in_rgb_only() {
    drive(Unit::x()).await;
}

// ---- the virtual Mini MK3 ------------------------------------------------------

/// What the virtual Mini shows for a frame's LED: what the hardware is
/// sent (section 13), a native animation in its nearest palette colour.
fn shown_on_virtual_mini(led: Led) -> Led {
    match led.animation {
        Animation::None => led,
        Animation::Pulse | Animation::Flash => Led {
            color: palette_rgb(nearest(led.color)),
            ..led
        },
    }
}

fn virtual_mini_shows_the_frame(rt: &Runtime, frame: &SurfaceFrame) -> bool {
    let want = rt.frame_for(&capabilities(rt, VIRTUAL_MINI));
    want.leds.len() == frame.leds.len()
        && want
            .leds
            .iter()
            .zip(&frame.leds)
            .all(|(w, f)| w.pos == f.pos && shown_on_virtual_mini(w.led) == f.led)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_virtual_mini_mk3_pulses_in_the_hardwares_palette() {
    let desk = desk().await;
    let rt = &desk.rt;
    let (device, handle) = VirtualDevice::new(virtual_model(&LAUNCHPAD_MINI_MK3));
    rt.add_device(Box::new(device));
    let mut events = rt.bus().subscribe();
    rt.connect_device(VIRTUAL_MINI).unwrap();
    next_event(&mut events, types::DEVICE_CONNECTED, VIRTUAL_MINI).await;
    wait_until("the desk on the virtual Mini", || {
        virtual_mini_shows_the_frame(rt, &handle.frame())
    })
    .await;

    handle.press(ALPHA, 40).unwrap();
    handle.release(ALPHA).unwrap();
    let pressed = next_event(&mut events, types::CONTROL_PRESSED, VIRTUAL_MINI).await;
    assert_eq!(pressed.payload["controlId"], "g.0.0");
    assert_eq!(pressed.payload["velocity"], 127, "the Mini has no velocity");
    let pulse = Led {
        color: palette_rgb(nearest(GREEN)),
        animation: Animation::Pulse,
    };
    assert_ne!(pulse.color, GREEN, "the palette has no such green");
    wait_until("a pulse in the palette's green", || {
        handle.frame().get(ALPHA) == Some(pulse)
    })
    .await;
    assert!(virtual_mini_shows_the_frame(rt, &handle.frame()));

    desk.bring_editor_forward();
    wait_until("the other surface on the virtual Mini", || {
        rt.state().active_surface_id == OTHER && virtual_mini_shows_the_frame(rt, &handle.frame())
    })
    .await;

    // Never part of a look at the hardware: it goes and comes back only
    // when asked.
    rt.rescan();
    assert_eq!(status_of(rt, VIRTUAL_MINI), Some(DeviceStatus::Connected));
    rt.disconnect_device(VIRTUAL_MINI).unwrap();
    next_event(&mut events, types::DEVICE_DISCONNECTED, VIRTUAL_MINI).await;
    assert_eq!(status_of(rt, VIRTUAL_MINI), Some(DeviceStatus::Available));
    rt.connect_device(VIRTUAL_MINI).unwrap();
    wait_until("the other surface again", || {
        virtual_mini_shows_the_frame(rt, &handle.frame())
    })
    .await;
    rt.release_devices();
    assert!(!handle.is_connected());
}
