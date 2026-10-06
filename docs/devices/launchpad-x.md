# Novation Launchpad X

Everything in this file concerns the Launchpad X: its profile `src-tauri/src/devices/novation/launchpad_x.rs`, driven by `novation/device.rs` (`Mk3Device`, the device every MK3 model shares). Its bytes are built and read by the MK3 codec it shares with the Launchpad Mini MK3 (`novation/protocol.rs`, `novation/layout.rs`, byte-tested in `novation/protocol_tests.rs`); together they are the only code in LaunchDeck that knows MIDI bytes. It reaches the ports through the transport in `src-tauri/src/devices/midi/` (`midir` only in `midi/system.rs`), and its golden transcript is `src-tauri/src/devices/novation/device_tests.rs`.

**Hardware status: NOT HARDWARE VERIFIED (b).** LaunchDeck was developed without a Launchpad X, and no Novation device was attached when this was last revised (2026-10-06). Encoders and decoders are covered by byte-level tests against the manual, and the device by tests against a simulated unit. "Verified" below means verified in Novation's documentation, not on a unit.

Source: Novation, *Launchpad X Programmer's Reference Manual* (2019-11-08, "X-PRM"). The bytes the X shares with the Launchpad Mini MK3, and the errors in the manual, are in [novation-mk3-protocol.md](novation-mk3-protocol.md); what the X reports about itself is in [capabilities.md](capabilities.md); the acceptance suite for a unit is in [hardware-testing.md](hardware-testing.md).

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

**Identity.** On connect LaunchDeck asks the unit who it is (the Universal Device Inquiry) and what it shows:

```
F0 7E 7F 06 01 F7              who are you?
F0 00 20 29 02 0C 00 F7        which layout? (the unit answers F0 00 20 29 02 0C 00 <layout> F7)
F0 00 20 29 02 0C 0A F7        which LED feedback? (answer … 0A <internal> <external> F7)
F0 00 20 29 02 0C 09 F7        are the LEDs on? (answer … 09 <00 asleep | 01 awake> F7)
```

The answer names the model's family and four firmware digits, shown in the device's details as the bootloader shows them (a captured X reply reads `0238`). The X is accepted with family `03 01` (Ableton's scripts and a captured reply) or `13 01` (what X-PRM p6 prints, a copy of the Mini MK3's). A unit that answers as its bootloader (second family byte `11`, inferred for the X from the Mini MK3's manual) is sent nothing more and is listed as failed: "The Launchpad is in its bootloader. Unplug it and plug it in again." One that answers with another model's family is turned down the same way: "LaunchDeck could not confirm which Launchpad this is. Press Look again." Both wait for Look again, or for the unit to be plugged in again. When the unit does not answer, as through a driver that drops SysEx, LaunchDeck goes on without the answers after 500 ms.

**Taking it.** The first render that has the answers, or the first one 500 ms after the questions, asks for Programmer mode and reads it back:

```
F0 00 20 29 02 0C 0E 01 F7     select Programmer mode
F0 00 20 29 02 0C 0E F7        which mode? (the unit answers F0 00 20 29 02 0C 0E 01 F7 in Programmer mode)
```

The first render that has that answer, or the first one 500 ms later, makes sure the LEDs show what LaunchDeck draws. External LED feedback switched off in the Setup menu hides lighting by notes in Programmer mode (MINI-UG p19 says so for the Mini MK3, which shares the `0Ah` feedback command; for the X this is assumed, and whether it also hides the lighting SysEx is unknown), so it is turned on, leaving internal feedback as it was (`F0 00 20 29 02 0C 0A <internal> 01 F7`); LEDs put to sleep are woken (`F0 00 20 29 02 0C 09 01 F7`). If a brightness is set (below), the unit's level is read and the new one set. Then the clear frame, and only then are presses read: before, a note sent in Note mode would read as a pad press. When the unit answered `01` the device's details say Programmer mode is `confirmed`. Otherwise they say `unconfirmed` and the device list carries the notice "Launchpad X connected, but Programmer Mode could not be confirmed.": LaunchDeck still draws. A reply that carries another model's product byte is ignored.

**Keeping it.** Every 2 s LaunchDeck asks a unit it took `F0 00 20 29 02 0C 0E F7`. An answer of `00` means the unit is back in Live mode: it was unplugged and plugged in again faster than the 3 s port scan noticed, or another program (Ableton Live, for example) switched it. Presses stop counting, and the unit is taken again from the identity questions on, then cleared and drawn whole. The third time within 30 s LaunchDeck leaves the unit to the other program, sends it nothing more (also not on disconnect) and lists it as failed: "Another program is using the Launchpad X. Press Look again to use it here." For a unit that confirmed Programmer mode, three questions in a row without an answer put up the notice "The Launchpad X stopped answering." until the next answer (a unit that never answered keeps its could-not-be-confirmed notice); whether the unit was unplugged is still for the port scan to say.

**Giving it back.** On disconnect, and when LaunchDeck quits, a unit LaunchDeck took has a running scroll stopped and its LEDs woken if LaunchDeck put them to sleep, then gets the clear frame and

```
F0 00 20 29 02 0C 0A <internal> <external> F7   the LED feedback it had, if LaunchDeck changed it
F0 00 20 29 02 0C 08 <level> F7   the brightness it had, if LaunchDeck changed it
F0 00 20 29 02 0C 0E 00 F7     back to Live mode
F0 00 20 29 02 0C 00 <layout> F7   the layout the unit showed before
```

Live mode lands in Note mode (`01`, X-PRM p7), so the layout is selected again only when the unit showed another one (a Custom mode, for example) and not Programmer mode; a unit that did not say which layout it showed is left in Note mode. After the unit was taken again, what it gets back is what it first reported, not what LaunchDeck or another program set in between. A unit that was turned down, or not taken yet, is sent nothing. Quitting runs the same sequence through the app's exit handler (`RunEvent::Exit`; NOT VERIFIED in the app window (f)), and a device dropped without a disconnect runs it once too. A crash or a kill skips it: the unit stays in Programmer mode until LaunchDeck starts again or the unit is plugged in again.

## Pad and button addressing

### Main grid (verified)

```
note = 10 * (8 - row) + col + 1        row 0 = top row, col 0 = left column
```

The manual shows lower-left = 11, upper-left = 81, lower-right = 18, which this formula reproduces. The same number is used as the LED index.

### Auxiliary buttons and logo (official, not tested on a unit)

These numbers are Novation's: the Programmer mode layout diagram gives them (X-PRM p10; the same image is in the Mini MK3's manual, whose User Guide also names the logo index 99, p23). An earlier version of this file called them unverified because the diagram could not be read as text; it has since been read. They are still not tested on a unit:

| Control | Number | Type |
| --- | --- | --- |
| Top row, left to right | CC 91 to 98 | CC |
| Side column, top to bottom | CC 89, 79, ... 19 | CC |
| Logo LED | 99 | output only |

They live in `novation/layout.rs` (`led_index`, `pos_from_cc`), shared with the Mini MK3. Nothing else depends on them.

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
| Palette (type `00`) colours | Static colours are exact in RGB. The palette is published only as a picture (X-PRM p12); LaunchDeck's table, read off that picture, is approximate and used only for the Mini MK3's native animation. |
| Hardware flashing (`01`) and pulsing (`02`) colourspecs | Both take palette entries. The shared codec encodes them and the Mini MK3 uses them, but the X's profile does not declare native animation, so its frames never carry them. Turning them on is a one-line profile change, left for after a check on a unit. |

### Consequences

* The profile's `capabilities.animations` reports `pulse: false, flash: false`, so the renderer **synthesises** animation: pulse is RGB brightness between 25% and 100% on a 1.4 s period, flash is a 0.7 s on and off toggle, both at 25 frames per second.
* Because only changed LEDs are sent, an animated pad sends one small SysEx per frame. Idle pads send nothing.

## Capabilities reported

```
grid 8 x 8, aux top 8, side 8, logo, rgb, velocity, no pressure, no native animations
features: Programmer mode, batched lighting, brightness, sleep, scrolling text, firmware version; no palette
```

## Brightness, sleep and scrolling text

The X has the same three commands as the Mini MK3 (X-PRM p23 text, p24 sleep, p25 brightness), and LaunchDeck offers them the same way: the brightness preference (in percent, for every device that has it), `set_device_sleep` and `scroll_device_text`. They send nothing until they are used, so the X's bytes do not change otherwise. Settings does not offer them yet (package M13 of the device plan). How they behave is described in [launchpad-mini-mk3.md](launchpad-mini-mk3.md#brightness-sleep-and-scrolling-text), with `0C` in place of `0D`.

## Lifecycle in LaunchDeck

1. Every 3 seconds (and on **Look again**) `NovationHardware` lists the MIDI ports, `ports.rs` sorts them into units, and the device manager registers any Launchpad X it finds as a device. Unplugged, unconnected hardware is forgotten.
2. **Connect** opens the MIDI in and out ports and asks the unit who it is; the next render takes it (Programmer mode, the LEDs cleared, then decoded input is forwarded), as described above. A unit in its bootloader or of another model is listed as failed until Look again, or until it is unplugged and plugged in again.
3. While connected, the unit is asked every 2 s whether it is still in Programmer mode, and taken again if it is not (see Keeping it above). If a render fails (cable pulled) the device is disconnected and `device.disconnected` is published. The virtual device is unaffected.
4. **Disconnect**, or quitting LaunchDeck, gives a unit LaunchDeck took back (see Giving it back above) and closes the ports.

## Trying it on real hardware

1. Quit anything else using the Launchpad (Ableton, Novation Components).
2. Plug it in. Within 3 s **Settings › Devices** lists it as connected; press **Look again** otherwise.
3. Open the Agent Control surface in Live mode and press **Run Agent Demo**. The grid should mirror the on-screen device, with the top row of pads changing colour as the four simulated agents move through their states.
4. Check that pulse and flash look right. Agent Control relies on them (running pulses, approval and failure flash) and on this device they are synthesised from RGB frames at 25 per second, so a slow USB or MIDI path would show as stutter. Then check the edges: top row, side column and the logo. If they do not respond or light, the unverified numbers above are the cause.
5. Run the acceptance suite, `sh scripts/launchpad-acceptance.sh` (it takes an X when no Mini MK3 is plugged in), and commit its report under `docs/verification/`; see [hardware-testing.md](hardware-testing.md).

## Defects fixed in the device lane

The X adapter of the prototype was correct but minimal. These defects were found while adding the Mini MK3 (plan `LAUNCHPAD_MINI_MK3_PLAN.md` section 2.3) and fixed for both models, each in its own commit, with a test and the golden transcript changed only by that fix. All fixes are tested against simulated units and fake ports only: NOT HARDWARE VERIFIED (b).

| Id | Defect | Fixed by |
| --- | --- | --- |
| D-X1 | Windows classic names: `LPX MIDI` is the DAW interface but was taken for the MIDI one, so one X became two units, one bound to the DAW interface | `77fe667` |
| D-X2 | Linux kernels before 5.13 name both interfaces `Launchpad X MIDI 1`/`2`: again two units | `77fe667` |
| D-X3 | Ids were a slug of the port name: two identical units collided, and Linux ids changed with the ALSA client on replug | `7010b2c` |
| D-X4 | Other Launchpad models were listed once per port, or not at all | `77fe667` |
| D-X5 | Programmer mode was entered blind: a unit in its bootloader or another model would have been driven, and Live-mode notes read as presses | `688c34d` |
| D-X6 | Programmer mode was never confirmed (the input ignored SysEx) | `38d3086` (SysEx delivered), `7a00689` |
| D-X7 | Release landed in Note mode, not in the layout the person had | `2692ecb` |
| D-X8 | A unit with external LED feedback off or its LEDs asleep showed nothing while LaunchDeck drew | `78f80d1` |
| D-X9 | A quick power cycle or another program switching the layout went unnoticed | `a552ccc` |
| D-X10 | Quitting LaunchDeck left the unit in Programmer mode | `dd6a8d6` |
| D-X11 | The output was opened and Programmer mode sent before the input existed; an input failure left a half-open adapter | `38d3086` |

## Known limitations

* Not tested on hardware (see above): NOT HARDWARE VERIFIED (b).
* No hardware animation (LaunchDeck draws pulse and flash), no palette colours, no pressure (aftertouch).
* A crash leaves the unit in Programmer mode until LaunchDeck starts again or the unit is plugged in again.
* On Windows 10 a port held by another program (a DAW, Novation Components) cannot be opened: the unit is listed as failed until that program lets go and Look again is pressed.
* The editor shows the first connected device. Several units may be connected, and all receive the same frame.
* With two identical units, which one is `launchpad-x:1` follows the system's port order and may change after a replug.
* Linux needs the ALSA sequencer (`libasound2`). Browser Web MIDI is not used.
