# What a device says about itself

Everything above the device layer knows a device only through the `SurfaceDevice` trait (`src-tauri/src/devices/mod.rs`) and what it reports: its **capabilities** (what the renderer may draw on it), its **features** (what it can do beyond drawing frames), its **details** (what LaunchDeck observed on it) and, through the device manager, its **status** in a sentence. This file describes each, the Launchpad Mini MK3's and Launchpad X's values, and how to add a device model.

The types are in `devices/mod.rs` (`DeviceCapabilities`, `DeviceIdentity`), `devices/features.rs` (`DeviceFeatures`, `DeviceDetails`, `ModeState`, `DeviceSetting`, `TextScroll`, `ScrollDirection`) and `devices/manager.rs` (`DeviceInfo`, `DeviceStatus`, `DeviceChange`). The values reported by a physical unit are NOT HARDWARE VERIFIED (b): no Novation device was attached (2026-10-06); they come from the profiles and are tested against simulated units.

## Identity

`DeviceIdentity { id, name, model, kind }`. `model` is the device family, `launchpad-mini-mk3` or `launchpad-x`; it is what `compatibleDevices` in a manifest names. `id` is `<model>:<n>` for hardware, numbered in the system's port order and never holding a port name, so a unit plugged in again keeps it; `virtual-launchpad-x` for the simulator. `name` is what a person reads: `Launchpad Mini MK3`, `Launchpad Mini MK3 (2)` for a second unit. `kind` is `hardware` or `virtual`. Another Launchpad model is listed with model `unsupported` and the id `unsupported:<its name as a slug>`.

## Capabilities: what the renderer may draw

`DeviceCapabilities` is the renderer's contract, and its fields do not change: code outside the device layer builds it field by field, and the TypeScript schema mirrors it. New things a device can do go into `DeviceFeatures` instead.

| Field | Meaning | Mini MK3 | X | Virtual X |
| --- | --- | --- | --- | --- |
| `grid` | width and height of the pad grid, or none | 8 × 8 | 8 × 8 | 8 × 8 |
| `aux` | buttons along the top and down the side, and a logo light | 8, 8, logo | 8, 8, logo | 8, 8, logo |
| `rgb` | shows any colour; otherwise the renderer reduces colour to brightness | yes | yes | yes |
| `velocity` | presses carry how hard; otherwise they report 127 | no | yes | yes |
| `pressure` | pads report pressure after the press | no | no | no |
| `animations.pulse`, `animations.flash` | the device pulses and flashes by itself; otherwise the renderer draws the animation frame by frame (pulse 1.4 s, flash 0.7 s) | yes, yes | no, no | yes, yes (CSS) |

A position the device lacks is dropped from the frame; the control still exists in the surface. The renderer reads nothing else.

## Features: what a device can do beyond drawing

`DeviceFeatures` (camelCase in JSON, every field defaults to off) says what LaunchDeck can ask of the device besides frames. The renderer never reads it.

| Field | Meaning | Mini MK3 | X | Virtual X | Virtual Mini MK3 |
| --- | --- | --- | --- | --- | --- |
| `hostMode` | LaunchDeck switches the device into a mode it owns (Programmer Mode) and back | yes | yes | no | no |
| `palette` | lights by index into a fixed colour table (used for native animation) | yes | no (unused) | no | yes |
| `batchedLighting` | many LEDs in one message | yes | yes | no | no |
| `flashPeriodMs`, `pulsePeriodMs` | the period of its native animation, when the device fixes it | 500, 1000 | none (LaunchDeck draws them) | none | 500, 1000 |
| `brightness` | its LED brightness can be set | yes | yes | no | yes (recorded) |
| `sleep` | its LEDs can be turned off and on | yes | yes | no | yes (recorded) |
| `scrollingText` | it scrolls text by itself | yes | yes | no | yes (recorded) |
| `firmwareVersion` | it reports a firmware version | yes | yes | no | no |

The Mini's flash and pulse periods are its defaults at 120 bpm (*Launchpad Mini [MK3] Programmer's Reference Manual* p12); MIDI clock from another program changes them. A unit whose firmware does not answer for brightness or sleep loses that feature (below). The virtual Mini MK3 exists for tests and is not offered in the app.

The settings a device can be given are a `DeviceSetting`: `Brightness(Option<percent>)` (`None` gives the device its own level back) or `Sleep(bool)`. Text is a `TextScroll { text, color, speed, direction, looping }`; LaunchDeck checks it before sending (1 to 64 printable ASCII characters, 1 to 63 pads per second). A device without the feature refuses with "This device cannot do that." These are the trait's defaults, so a device that has none of it, like the virtual X and the test fakes, writes no code for them:

```rust
fn features(&self) -> DeviceFeatures { DeviceFeatures::default() }
fn details(&self) -> DeviceDetails { DeviceDetails::default() }
fn apply(&mut self, setting: DeviceSetting) -> Result<(), DeviceError>;          // refuses
fn scroll_text(&mut self, text: Option<&TextScroll>) -> Result<(), DeviceError>; // refuses
```

The runtime exposes them as `Runtime::set_device_brightness(percent)` (a preference for every device with the feature, saved as `deviceBrightness` in the workspace's preferences), `set_device_sleep(id, asleep)` and `scroll_device_text(id, text)`, and the app as the commands of the same names. Nothing puts a device to sleep by itself.

## Details: what LaunchDeck observed

`DeviceDetails` is what LaunchDeck saw on a connected device. It changes while the device runs; when it does, the device manager reports `DeviceChange::Updated` and the runtime sends its state again, with no event on the bus.

| Field | Meaning |
| --- | --- |
| `firmware` | the version the device reported, as the device shows it (`0531` for a Launchpad: four digits, not translated to Novation's version names) |
| `mode` | `none` (the device has no such mode, or LaunchDeck does not hold it), `acquiring` (being identified and switched over), `confirmed` (the device said it is in the mode), `unconfirmed` (switched over, but the device did not say so) |
| `brightness` | percent, when LaunchDeck set or read it |
| `asleep` | LaunchDeck put its LEDs to sleep and they have not woken |
| `scrolling` | a text LaunchDeck sent is still scrolling |
| `notice` | connected, with a limitation, in a sentence |

## Status, problems and notices

The device list (`DeviceInfo`, one per device) gives each device a `status`:

| Status | Meaning |
| --- | --- |
| `connected` | LaunchDeck holds the device and draws on it |
| `available` | found, not connected (not yet, or the person disconnected it) |
| `unplugged` | was in use and is gone; it is connected again when it returns, even after a refusal |
| `unsupported` | another Launchpad model: listed by name, never opened |
| `failed` | the last connect failed, or the device turned LaunchDeck down; `problem` says why, and LaunchDeck waits for Look again or a replug |

A `problem` (only with `failed`) is one of these sentences:

* "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again." (the ports could not be opened)
* "The Launchpad is in its bootloader. Unplug it and plug it in again."
* "LaunchDeck could not confirm which Launchpad this is. Press Look again." (it answered as another model)
* "Another program is using the Launchpad Mini MK3. Press Look again to use it here." (taken over three times within 30 s)

A `notice` (only with `connected`, from `details.notice`) says the device works with a limitation:

* "Launchpad Mini MK3 connected, but Programmer Mode could not be confirmed."
* "The Launchpad Mini MK3 stopped answering."
* "Firmware version 0531 does not support brightness. Novation Components can update it." (or "sleep", or "brightness or sleep")

The model's name in these sentences is the device's. An unsupported device's own sentence is "Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3." The operating system's text is never in a sentence; it goes to the log, through `DeviceError::detail`.

A `DeviceInfo` in JSON:

```json
{
  "identity": { "id": "launchpad-mini-mk3:1", "name": "Launchpad Mini MK3", "model": "launchpad-mini-mk3", "kind": "hardware" },
  "capabilities": { "grid": { "width": 8, "height": 8 }, "aux": { "top": 8, "side": 8, "logo": true },
                    "rgb": true, "velocity": false, "pressure": false, "animations": { "pulse": true, "flash": true } },
  "connected": true,
  "status": "connected",
  "details": { "firmware": "0531", "mode": "confirmed", "brightness": null, "asleep": false, "scrolling": false, "notice": null },
  "features": { "hostMode": true, "palette": true, "batchedLighting": true, "flashPeriodMs": 500, "pulsePeriodMs": 1000,
                "brightness": true, "sleep": true, "scrollingText": true, "firmwareVersion": true }
}
```

`problem` and `notice` are left out when there is none. The app's Zod schema (`src/lib/schema/runtime.ts`) does not read `details`, `features` and `notice` yet and drops them; package M13 of the device plan adds them, with a contract fixture, and shows them in Settings › Devices.

## Adding a device model

Read [CODEBASE_MAP.md](../architecture/CODEBASE_MAP.md#1-where-does-it-belong) and [ENGINEERING_RULES.md](../architecture/ENGINEERING_RULES.md) first. The rules that matter most here: no MIDI byte outside `devices/` (R5), `midir` only in `devices/midi/system.rs`, no `cfg!(target_os)` under `devices/` (take a `Platform`), no model name in `manager.rs`, `source.rs`, `features.rs`, `midi/`, the renderer or the runtime, and no new field in `DeviceCapabilities`.

**Another Novation MK3 model** whose manual defines the same commands with the same bytes (compare the manuals first, as [novation-mk3-protocol.md](novation-mk3-protocol.md#why-one-layer) did; a model that differs, like the Launchpad Pro MK3, is not one of these):

1. Add its product byte to `Product` in `novation/protocol.rs`, with the manual page.
2. Write its profile, `novation/<model>.rs`, after `launchpad_mini_mk3.rs`: model id, name, product, inquiry families, port-name tokens, interface order, Live layout, capabilities and features, each with the manual page it comes from.
3. Add it to `KNOWN` in `novation/hardware.rs` (recognised on the ports; listed as not supported yet), with port fixtures for every system in `novation/ports_tests.rs`. Then to `DRIVEN`, with tests over the simulator in `novation/device_tests.rs` (`SimLaunchpad` has `mini()` and `x()`; the new model gets its own, with its product byte, family and power-on layout from its manual).
4. Change the `Unsupported` sentence in `devices/mod.rs` and the user-facing wording, write its notes in `docs/devices/`, and update the map's `devices` row.

**Another family** (not a Novation MK3): a module `devices/<family>/` with its own `SurfaceDevice` and a `HardwareSource` over the `devices::midi` transport, so that it is tested against `FakeMidi` like the Launchpads. The manager takes one `HardwareSource`; a second family needs a source that asks both, in `devices/`, without model names. Its capabilities say what the renderer may draw; anything more goes into `DeviceFeatures`, with defaults off so that the other devices do not change. A new top-level module gets its map row in the same commit (R1, R21).
