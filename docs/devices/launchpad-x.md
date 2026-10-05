# Novation Launchpad X

Everything in this file concerns the Launchpad X: its profile `src-tauri/src/devices/novation/launchpad_x.rs`, driven by `novation/device.rs` (`Mk3Device`, the device every MK3 model shares). Its bytes are built and read by the MK3 codec it shares with the Launchpad Mini MK3 (`novation/protocol.rs`, `novation/layout.rs`, byte-tested in `novation/protocol_tests.rs`); together they are the only code in LaunchDeck that knows MIDI bytes. It reaches the ports through the transport in `src-tauri/src/devices/midi/` (`midir` only in `midi/system.rs`), and its golden transcript is `src-tauri/src/devices/novation/device_tests.rs`.

**Hardware status: not tested on a physical unit.** The prototype was developed without a Launchpad X. Encoders and decoders are covered by byte-level unit tests. Sections below say what is verified from Novation's documentation and what is not.

Source: Novation, *Launchpad X Programmer's Reference Manual*.

## Ports

A Launchpad X has two USB-MIDI interfaces (X-PRM p6): the **DAW** interface first and the **MIDI** interface second. LaunchDeck opens only the MIDI interface, which carries Programmer-mode notes, CCs and lighting SysEx; the DAW interface is recognised and never opened.

Each system names them differently. `src-tauri/src/devices/novation/ports.rs` reads every name on its own, with the system as a parameter, and pairs the unit's MIDI input with its MIDI output:

| System | MIDI interface (input / output) | DAW interface | How the role is told |
| --- | --- | --- | --- |
| macOS | `Launchpad X LPX MIDI Out` / `Launchpad X LPX MIDI In` | `… LPX DAW Out` / `… LPX DAW In` | the word after `LPX` |
| Windows, classic names | `MIDIIN2 (LPX MIDI)` / `MIDIOUT2 (LPX MIDI)` | `LPX MIDI` (both directions) | the `MIDIIN2`/`MIDIOUT2` wrapper is interface 2; a bare name is interface 1, the DAW one |
| Windows MIDI Services, new style | `LPX MIDI Out` / `LPX MIDI In` | `LPX DAW Out` / `LPX DAW In` | the word after `LPX` |
| Linux, kernel 5.13 or later | `Launchpad X:Launchpad X LPX MIDI In 28:1` (one duplex port) | `…LPX DAW In 28:0` | the word after `LPX` |
| Linux, older kernels | `Launchpad X:Launchpad X MIDI 2 28:1` | `…MIDI 1 28:0` | `MIDI n` is interface n |

Several units: Linux gives each its own ALSA client (`28` above), Windows classic adds a leading `2- ` and new style a ` (2)`, and macOS gives identical names, so there the k-th MIDI input is paired with the k-th MIDI output. Units are numbered in the order of their MIDI input: ids `launchpad-x:1`, `launchpad-x:2`, …, names `Launchpad X`, `Launchpad X (2)`, …. The id does not contain a port name or an ALSA client number, so a unit plugged in again keeps `launchpad-x:1`; nothing about ports is saved, so with two units which physical one is number 1 may change after a replug. Other Launchpad models are listed once per unit as not supported and never opened. Every name above is derived from Novation's documents, Microsoft's naming rules, the Linux sources and community projects; the fixtures are in `novation/ports_tests.rs`. **None of it has been observed on a real system**: no Launchpad X, Windows machine or Linux desktop was available.

## Taking the unit and giving it back

LaunchDeck drives the X in Programmer mode. Header `F0 00 20 29 02 0C` is the Novation id plus the Launchpad X product id. Verified in the manual. In Programmer mode the pads and buttons are plain note and CC senders and the device applies no musical behaviour. The Setup page is unavailable until you leave it.

Nothing waits for the unit: connecting only opens the ports and asks; every render (every 40 ms) reads what the unit answered since the last one and goes on (`novation/session.rs`).

**Identity.** On connect LaunchDeck asks the unit who it is (the Universal Device Inquiry):

```
F0 7E 7F 06 01 F7              who are you?
```

The answer names the model's family and four firmware digits, shown in the device's details as the bootloader shows them (a captured X reply reads `0238`). The X is accepted with family `03 01` (Ableton's scripts and a captured reply) or `13 01` (what X-PRM p6 prints, a copy of the Mini MK3's). A unit that answers as its bootloader (second family byte `11`, inferred for the X from the Mini MK3's manual) is sent nothing more and is listed as failed: "The Launchpad is in its bootloader. Unplug it and plug it in again." One that answers with another model's family is turned down the same way: "LaunchDeck could not confirm which Launchpad this is. Press Look again." Both wait for Look again. A unit that does not answer, as through a driver that drops SysEx, is taken anyway after 500 ms.

**Taking it.** The first render that has the answer, or the first one 500 ms after the question, sends

```
F0 00 20 29 02 0C 0E 01 F7     select Programmer mode
```

then the clear frame, and only then are presses read: before, a note sent in Note mode would read as a pad press.

**Giving it back.** On disconnect, a unit LaunchDeck took gets the clear frame and

```
F0 00 20 29 02 0C 0E 00 F7     back to Live mode
```

A unit that was turned down, or not taken yet, is sent nothing.

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

Colour channels are 7-bit, so `0..255` is scaled to `0..127`. The manual allows up to 81 colourspecs per message; larger updates are chunked. There are exactly 81 LEDs (64 grid + 8 top + 8 side + 1 logo). Only LEDs whose 7-bit colour changed are sent: a change that scales to the colour the pad already shows (255 and 254 are both `7F`) sends nothing. All the changes of one frame go in one message.

Clear sends every known LED as black.

### What is deliberately not used

| Feature | Why |
| --- | --- |
| Palette (type `00`) colours | The 128-entry palette cannot be read from the text of the manual. LaunchDeck does not invent it. |
| Hardware flashing (`01`) and pulsing (`02`) colourspecs | Both reference palette indices. The shared codec encodes them (byte-tested in `novation/protocol_tests.rs`), but the X's frames never carry them. |

### Consequences

* The profile's `capabilities.animations` reports `pulse: false, flash: false`, so the renderer **synthesises** animation: pulse is RGB brightness between 25% and 100% on a 1.4 s period, flash is a 0.7 s on and off toggle, both at 25 frames per second.
* Because only changed LEDs are sent, an animated pad sends one small SysEx per frame. Idle pads send nothing.

## Capabilities reported

```
grid 8 x 8, aux top 8, side 8, logo, rgb, velocity, no pressure, no native animations
```

## Lifecycle in LaunchDeck

1. Every 3 seconds (and on **Rescan**) `NovationHardware` lists the MIDI ports, `ports.rs` sorts them into units, and the device manager registers any Launchpad X it finds as a device. Unplugged, unconnected hardware is forgotten.
2. **Connect** opens the MIDI in and out ports and asks the unit who it is; the next render takes it (Programmer mode, the LEDs cleared, then decoded input is forwarded), as described above. A unit in its bootloader or of another model is listed as failed until Look again.
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
* With two identical units, which one is `launchpad-x:1` follows the system's port order and may change after a replug.
* Linux needs the ALSA sequencer (`libasound2`). Browser Web MIDI is not used.
