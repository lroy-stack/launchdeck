# Novation Launchpad X

Everything in this file concerns `src-tauri/src/devices/novation/launchpad_x.rs`, the only code in LaunchDeck that knows MIDI bytes. It reaches the ports through the transport in `src-tauri/src/devices/midi/` (`midir` only in `midi/system.rs`), and its golden transcript is `src-tauri/src/devices/novation/launchpad_x_tests.rs`.

**Hardware status: not tested on a physical unit.** The prototype was developed without a Launchpad X. Encoders and decoders are covered by byte-level unit tests. Sections below say what is verified from Novation's documentation and what is not.

Source: Novation, *Launchpad X Programmer's Reference Manual*.

## Ports

A Launchpad X exposes two MIDI port pairs:

| Port name (typical) | Use |
| --- | --- |
| `LPX MIDI In/Out` | Used by LaunchDeck. Carries Programmer-mode notes, CCs and lighting SysEx. |
| `LPX DAW In/Out` | DAW integration. Ignored. |

OS names differ, so `classify_port` matches loosely and is tested on examples:

| OS | Example |
| --- | --- |
| macOS | `LPX MIDI`, `LPX DAW` |
| Linux (ALSA) | `Launchpad X:Launchpad X LPX MIDI 28:1` |
| Windows | `MIDIIN2 (LPX MIDI)` |

A name containing `daw` is the DAW port. A name containing `lpx` or `launchpad x` and `midi` is the MIDI port. On Windows the second interface carries Programmer mode.

## Programmer mode

On connect LaunchDeck sends:

```
F0 00 20 29 02 0C 0E 01 F7     select Programmer mode
```

and on disconnect:

```
F0 00 20 29 02 0C 0E 00 F7     back to Live mode
```

Header `F0 00 20 29 02 0C` is the Novation id plus the Launchpad X product id. Verified in the manual. In Programmer mode the pads and buttons are plain note and CC senders and the device applies no musical behaviour. The Setup page is unavailable until you leave it.

## Pad and button addressing

### Main grid (verified)

```
note = 10 * (8 - row) + col + 1        row 0 = top row, col 0 = left column
```

The manual shows lower-left = 11, upper-left = 81, lower-right = 18, which this formula reproduces. The same number is used as the LED index.

### Auxiliary buttons and logo (UNVERIFIED)

The manual draws these in diagrams that could not be read in text form, so the numbers below follow the layout commonly documented for the Launchpad MK3 family and **have not been confirmed**:

| Control | Number | Type |
| --- | --- | --- |
| Top row, left to right | CC 91 to 98 | CC |
| Side column, top to bottom | CC 89, 79, ... 19 | CC |
| Logo LED | 99 | output only |

Auxiliary controls are therefore marked experimental. If your unit disagrees, change `led_index`, `pos_from_cc` and the two tests beside them. Nothing else depends on these numbers.

### Input decoding

| Message | Meaning |
| --- | --- |
| `9n note vel` with `vel > 0` | Pad pressed (velocity kept). |
| `9n note 0` or `8n note vel` | Pad released. |
| `Bn cc vel` | Aux button pressed (`vel > 0`) or released (`vel = 0`). |
| anything else (aftertouch, clock) | Ignored. |

## Lighting

### What is used

RGB colourspecs in the lighting SysEx (verified in the manual):

```
F0 00 20 29 02 0C 03  03 <index> <r> <g> <b>  [03 <index> <r> <g> <b> ...]  F7
```

Colour channels are 7-bit, so `0..255` is scaled to `0..127`. The manual allows up to 81 colourspecs per message; larger updates are chunked. There are exactly 81 LEDs (64 grid + 8 top + 8 side + 1 logo). Only LEDs whose colour changed are sent.

Clear sends every known LED as black.

### What is deliberately not used

| Feature | Why |
| --- | --- |
| Palette (type `00`) colours | The 128-entry palette cannot be read from the text of the manual. LaunchDeck does not invent it. |
| Hardware flashing (`01`) and pulsing (`02`) colourspecs | Both reference palette indices. `encode_flash` and `encode_pulse` are implemented and byte-tested, but the renderer does not use them. |

### Consequences

* `capabilities().animations` reports `pulse: false, flash: false`, so the renderer **synthesises** animation: pulse is RGB brightness between 25% and 100% on a 1.4 s period, flash is a 0.7 s on and off toggle, both at 25 frames per second.
* Because only changed LEDs are sent, an animated pad sends one small SysEx per frame. Idle pads send nothing.

## Capabilities reported

```
grid 8 x 8, aux top 8, side 8, logo, rgb, velocity, no pressure, no native animations
```

## Lifecycle in LaunchDeck

1. Every 3 seconds (and on **Rescan**) `discover()` lists MIDI ports and registers any Launchpad X it finds as a device. Unplugged, unconnected hardware is forgotten.
2. **Connect** opens the MIDI in and out ports, sends Programmer mode, clears the LEDs and starts forwarding decoded input.
3. If a render fails (cable pulled) the device is disconnected and `device.disconnected` is published. The virtual device is unaffected.
4. **Disconnect** clears the LEDs, returns to Live mode and closes the ports.

## Trying it on real hardware

1. Quit anything else using the Launchpad (Ableton, Novation Components).
2. Plug it in, then **Settings, Rescan, Connect**.
3. Open the Agent Control surface in Live mode and press **Run Agent Demo**. The grid should mirror the on-screen device, with the top row of pads changing colour as the four simulated agents move through their states.
4. Check that pulse and flash look right. Agent Control relies on them (running pulses, approval and failure flash) and on this device they are synthesised from RGB frames at 25 per second, so a slow USB or MIDI path would show as stutter. Then check the edges: top row, side column and the logo. If they do not respond or light, the unverified numbers above are the cause.
5. Report what you saw. A short note of the device firmware, OS and the observed CC numbers is enough to finish verification.

## Known limitations

* Not tested on hardware (see above).
* Auxiliary numbers unverified.
* No palette colours, no hardware animation, no pressure (aftertouch) support.
* The editor shows the first connected device. Several units may be connected, and all receive the same frame.
* Linux needs the ALSA sequencer (`libasound2`). Browser Web MIDI is not used.
