# Novation Launchpad Mini MK3

What LaunchDeck does with a Launchpad Mini MK3. The Mini is a profile, `src-tauri/src/devices/novation/launchpad_mini_mk3.rs`, driven by `Mk3Device` (`novation/device.rs`), the same device that drives the Launchpad X. The bytes both models share, and why they share them, are in [novation-mk3-protocol.md](novation-mk3-protocol.md); what the Mini reports about itself is in [capabilities.md](capabilities.md); how all of it is tested, with and without a unit, is in [hardware-testing.md](hardware-testing.md).

**Hardware status: NOT HARDWARE VERIFIED (b).** No Novation device was attached when this was written (2026-10-06, macOS 15.4.1, Apple M2), and there is no Windows machine and no Linux desktop here. Everything below is what Novation's documents say and what LaunchDeck does against a simulated Mini (`novation/testkit.rs`, written from the manuals), not what a unit was seen to do. The acceptance suite of [hardware-testing.md](hardware-testing.md) is how that changes.

Sources: Novation, *Launchpad Mini [MK3] Programmer's Reference Manual* (2019-11-08, "the Programmer's Reference" below) and *Launchpad Mini User Guide* v2.0 (English, 2024-02-09, "the User Guide"). Page numbers are the printed ones.

## In short

* The 64 pads, the 8 buttons along the top and the 8 down the right are controls on a surface; the Novation logo is a light.
* Static colours are shown exactly (RGB). Pulsing and flashing pads are animated by the unit itself, in the nearest colour of its fixed palette.
* LaunchDeck finds the unit on its own, checks that it is a Mini, takes it into Programmer Mode, and gives it back as it found it.
* Unplugging, plugging in again, switching the unit off and on, or another program taking it are noticed and handled without a restart.
* The LED brightness, sleep and scrolling text can be set (as commands; Settings does not offer them yet, see below).
* The firmware version the unit reports is shown in its details.

## Ports

The Mini has two USB-MIDI interfaces (Programmer's Reference p6): the **DAW** interface first, for DAWs and Session mode, and the **MIDI** interface second, for Custom modes and Programmer Mode. Programmer-mode presses and the lighting go through the MIDI interface (p10, p12), so LaunchDeck opens only that one. The DAW interface is recognised and never opened.

Each system names them differently. `novation/ports.rs` reads every port name on its own, with the system as a parameter, and pairs the unit's MIDI input with its MIDI output:

| System | MIDI interface (input / output) | DAW interface | How the role is told |
| --- | --- | --- | --- |
| macOS | `Launchpad Mini MK3 LPMiniMK3 MIDI Out` / `Launchpad Mini MK3 LPMiniMK3 MIDI In` (also seen reported without `In`/`Out`) | `… LPMiniMK3 DAW Out` / `… DAW In` | the word after `LPMiniMK3` |
| Windows, classic names | `MIDIIN2 (LPMiniMK3 MIDI)` / `MIDIOUT2 (LPMiniMK3 MIDI)` | `LPMiniMK3 MIDI` (both directions) | the `MIDIIN2`/`MIDIOUT2` wrapper is interface 2; the bare name is interface 1, the DAW one, although it says `MIDI` |
| Windows MIDI Services, new style | `LPMiniMK3 MIDI Out` / `LPMiniMK3 MIDI In` | `LPMiniMK3 DAW Out` / `… DAW In` | the word after `LPMiniMK3` |
| Linux, kernel 5.13 or later | `Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 MI 20:1` (one duplex port) | `… LPMiniMK3 DA 20:0` | ALSA cuts port names at 31 characters, so `MIDI` reads `MI` and `DAW` reads `DA` |
| Linux, older kernels | `Launchpad Mini MK3:Launchpad Mini MK3 MIDI 2 20:1` | `… MIDI 1 20:0` | `MIDI n` is interface n |

These names are derived from Novation's support screenshots, Microsoft's naming rules, the Linux sources and community projects; the fixtures are in `novation/ports_tests.rs`. **None was observed on a real system.** A name is only a hint: the unit's answer to the inquiry decides what it is (below).

**Several units.** Units are numbered in the order of their MIDI input in the system's list: ids `launchpad-mini-mk3:1`, `launchpad-mini-mk3:2`, …, names `Launchpad Mini MK3`, `Launchpad Mini MK3 (2)`, …. The id holds no port name and no ALSA client number, so a unit plugged in again keeps its id. Linux gives each unit its own ALSA client, Windows classic adds a leading `2- ` and the new style a ` (2)`, but macOS gives identical units identical names, so there the k-th MIDI input is paired with the k-th MIDI output: a guess the inquiry then checks. The "USB device ID" the bootloader can set (Programmer's Reference p5, User Guide p22) is not used; whether it changes the port names is undocumented.

## Taking the unit and giving it back

Nothing waits for the unit: connecting opens the two ports and asks; every render (every 40 ms) reads what the unit answered since the last one and goes on (`novation/session.rs`). A unit plugged in is found by the port scan within 3 s and taken; **Look again** in Settings › Devices scans at once.

**1. Identify.** One burst of questions (Programmer's Reference p6-7, p21-22):

```
F0 7E 7F 06 01 F7              who are you? (the Universal Device Inquiry)
F0 00 20 29 02 0D 00 F7        which layout do you show?
F0 00 20 29 02 0D 0A F7        which LED feedback?
F0 00 20 29 02 0D 09 F7        are your LEDs on?
```

The Mini answers the inquiry with family `13 01` and four firmware digits. A unit that answers `13 11` is in its bootloader: it is sent nothing more and listed as failed, "The Launchpad is in its bootloader. Unplug it and plug it in again." A unit that answers with another family is turned down, "LaunchDeck could not confirm which Launchpad this is. Press Look again." Replies that carry another model's product byte are ignored. A unit that does not answer within 500 ms (a driver may drop SysEx) is taken anyway, without the answers.

**2. Programmer Mode.** The next render asks for it and reads it back (p7-8):

```
F0 00 20 29 02 0D 0E 01 F7     select Programmer Mode
F0 00 20 29 02 0D 0E F7        which mode? (the unit answers … 0E 01 F7 in Programmer Mode)
```

When the unit answers `01`, the device's details say Programmer Mode is `confirmed`. Otherwise they say `unconfirmed`, the device list carries the notice "Launchpad Mini MK3 connected, but Programmer Mode could not be confirmed.", and LaunchDeck still draws.

**3. Configure.** If external LED feedback was switched off in the unit's Setup menu, lighting by notes would not show in Programmer Mode (User Guide p19), so it is switched on and internal feedback left as it was (`F0 00 20 29 02 0D 0A <internal> 01 F7`). LEDs put to sleep are woken (`… 09 01 F7`). If a brightness is set (below), the unit's own level is read and the new one set. Then every LED is cleared, and only then do presses count: before, a note sent by the Keys layout would read as a pad press.

**4. Draw.** Every render sends what changed (see Lighting).

**5. Keep.** Every 2 s LaunchDeck asks `F0 00 20 29 02 0D 0E F7` (and, while it believes the LEDs asleep, `… 09 F7`). An answer of `00` means the unit is back in Live mode: it was power-cycled faster than the port scan noticed, or another program switched it. Presses stop counting and the unit is taken again from step 1, then drawn whole. The third time within 30 s LaunchDeck leaves the unit to the other program, sends it nothing more and lists it as failed, "Another program is using the Launchpad Mini MK3. Press Look again to use it here." For a unit that confirmed Programmer Mode, three questions in a row without an answer put up the notice "The Launchpad Mini MK3 stopped answering." until the next answer; whether the unit was unplugged is for the port scan to say.

**6. Give back.** On Disconnect and when LaunchDeck quits, a unit LaunchDeck took gets, in order: a running scroll stopped, its LEDs woken if LaunchDeck put them to sleep, every LED cleared, the LED feedback it had if LaunchDeck changed it, the brightness it had if LaunchDeck changed it, then

```
F0 00 20 29 02 0D 0E 00 F7         back to Live mode
F0 00 20 29 02 0D 00 <layout> F7   the layout it showed before
```

Live mode outside a DAW lands in Custom mode 2, Keys (`05`, p8), not in the layout the person had, so the layout read in step 1 is selected again unless it was Keys or Programmer Mode. What is given back is what the unit first reported: after a takeover, what the other program set is not the person's. LaunchDeck never sends DAW mode (`10h`), which belongs to a DAW (p15). A unit that was turned down, or not taken yet, is sent nothing. Quitting runs the same sequence through the app's exit handler (`RunEvent::Exit`; NOT VERIFIED in the app window (f)), and a device dropped without a disconnect runs it once too.

**What the person sees.** The device list keeps E2's statuses: `connected`, `available`, `unplugged`, `unsupported` and `failed`. A connected Mini also has `details.mode` (`acquiring`, `confirmed` or `unconfirmed`), and a `notice` when it is connected with a limitation; a failed one has a `problem` in a sentence: a refusal above, or "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again." Unplugged is listed until the unit returns, and a unit plugged in again is taken again, even one that was refused. See [capabilities.md](capabilities.md).

## Controls

LaunchDeck reads the Programmer Mode layout (Programmer's Reference p10, p13; User Guide p23). Rows are counted from the top in LaunchDeck and from the bottom in the manual:

| Control | Position in LaunchDeck | The unit sends | Read as |
| --- | --- | --- | --- |
| 64 pads | `grid` column 0-7, row 0 (top) to 7 | Note `10 × (8 − row) + column + 1`: 81-88 the top row, 11-18 the bottom row | pressed when the velocity is above 0; released on velocity 0 or a Note Off |
| 8 top buttons: Up, Down, Left, Right, Session, Drums, Keys, User (User Guide p8) | `top` 0-7, left to right | CC 91-98 | pressed when the value is above 0, released on 0 |
| 8 right buttons: seven Scene Launch `>`, then Stop/Solo/Mute | `side` 0-7, top to bottom | CC 89, 79, … 19 | the same |
| Novation logo | `logo` | nothing: a light, not a button | |

The Mini's pads are not velocity-sensitive and have no pressure: its command summary lacks the velocity, aftertouch and fader-velocity commands of the Launchpad X (Programmer's Reference p23). Presses are therefore reported with velocity 127, and the profile says `velocity: false`, `pressure: false`. Aftertouch, clock, notes and CCs outside the map are ignored, whatever the channel. The value a pad sends when pressed and the exact values of the buttons are not in Novation's documents (expected `7Fh` and `00h`; community drivers agree).

## Lighting

The unit has 81 LEDs (64 pads, 8 top, 8 right, the logo), each lit on its own. LaunchDeck's renderer passes pulse and flash to the Mini because the Mini does them itself; the device layer chooses what to send:

| What a pad shows | What is sent (lighting SysEx `03h`, p14) |
| --- | --- |
| a static colour | RGB, each channel scaled from 0-255 to 0-127: exact |
| pulsing | the unit's pulse, in the palette entry nearest the colour |
| flashing | the unit's flash between the nearest palette entry and off |
| off | RGB `0 0 0` |

**Timing.** The unit flashes once per beat and pulses over two beats, at 120 bpm unless it received MIDI clock since it was switched on (p12): **flash 500 ms, pulse 1000 ms**. LaunchDeck never sends clock, but another program that does (a DAW) changes the tempo the Mini animates at until it is switched off. There is no speed control. The Launchpad X, which LaunchDeck animates itself, uses 700 ms and 1400 ms instead, as does the simulator on screen.

**The palette is approximate.** Novation publishes the 128 palette colours only as a picture (p11). LaunchDeck's table (`novation/palette.rs`) was read off that picture and agrees with the colours the manual names, but was not measured on LEDs. A pulsing or flashing pad in a colour the palette lacks shows the nearest entry; a static pad is always exact.

**Batching.** Every change of one render goes into one lighting message (at most 81 LEDs per message), and only LEDs whose colour or animation changed are sent. After waking from sleep, a brightness change, the end of a scroll, or taking the unit again, the next frame is sent whole. Measurements and the rule that would change this are in [hardware-testing.md](hardware-testing.md#the-batching-rule).

## Brightness, sleep and scrolling text

All three are commands of the runtime today: `set_device_brightness`, `set_device_sleep` and `scroll_device_text` (`src-tauri/src/commands/devices.rs`). Settings › Devices does not offer them yet; that is package M13 of the device plan, which also adds the firmware and Programmer Mode lines. The Launchpad X has the same three.

**Brightness** (p22) is a preference, `deviceBrightness`, in percent, saved in the workspace's preferences; unset means each unit keeps its own level. It is one value for every device that has the feature. LaunchDeck reads the unit's level, then sets `F0 00 20 29 02 0D 08 <level> F7` with level = percent × 127 / 100, rounded: when the unit is taken, when the preference changes, and after every replug. Release gives the unit its own level back. How the 128 levels map onto the 8 levels of the Setup menu, and whether a level set this way survives switching the unit off, are undocumented.

**Sleep** (p21) turns the LEDs off (`… 09 00 F7`) until LaunchDeck turns them on (`… 09 01 F7`) or someone presses a pad or button (User Guide p19). Nothing puts the unit to sleep by itself. The press that wakes it only wakes it: it and its release are not passed on as input. On waking, the next frame is sent whole. Whether the unit sends that press at all, and whether it keeps its LEDs while asleep, are unknown; the heartbeat corrects LaunchDeck's idea of it within 2 s.

**Scrolling text** (p20): the unit scrolls the text itself, across the pads and the right column (not the top row or the logo), and keeps the lighting underneath. LaunchDeck sends the text, whether it loops, its speed in pads per second (1-63), its direction and its colour (RGB). LaunchDeck's own limits, since the unit's are undocumented: 1 to 64 printable ASCII characters; anything else is refused with "Scrolling text takes up to 64 letters, digits and punctuation marks." No message says when a scroll ends, so a scroll that does not loop is taken as over after an estimate (six pad columns per character, an assumption), and the frame is then sent whole. Stopping sends the text command alone (`F0 00 20 29 02 0D 07 F7`).

**Older firmware.** When a unit answers the other questions but not the brightness or sleep question, that feature is taken off for the unit and its notice says so, for example "Firmware version 0531 does not support brightness. Novation Components can update it."

## Firmware and Novation Components

The inquiry reply carries four digits, which LaunchDeck shows as the bootloader shows them on the pads (for example `0531`). How they map to Novation's version names ("2.0") is undocumented, so LaunchDeck does not translate them, and no feature is tied to a version: a feature is taken off only when the unit does not answer for it (above). Firmware is updated with Novation Components (User Guide p7); LaunchDeck never updates firmware, never enters the bootloader, never changes the device ID and never edits Custom modes. Disconnect the unit in LaunchDeck, or quit LaunchDeck, before using Components on it.

## Limitations

* **Crash.** If LaunchDeck crashes or is killed, the unit is not given back: it stays in Programmer Mode, with its Setup menu unavailable (entering Programmer Mode by SysEx disables the Setup entry, p7-8), until LaunchDeck starts again and gives it back, or the unit is unplugged and plugged in again (it always powers on in Live mode, User Guide p20).
* **Identical units.** With two Minis, which physical unit is `launchpad-mini-mk3:1` follows the system's port order and may change after a replug; nothing about ports is saved.
* **A DAW at the same time.** LaunchDeck never opens the DAW interface and never sends DAW mode. A DAW that drives the Mini (Ableton Live's script switches it to DAW mode and Session on identification) takes it out of Programmer Mode; LaunchDeck takes it back, and after three times within 30 s leaves it to the DAW (above). Use one or the other.
* **Windows 10 ports are exclusive.** A port another program holds (a DAW, Components) cannot be opened, and the unit is listed as failed with the control-port sentence. Close the other program and press Look again.
* **macOS pairing** of identical units by list order is a guess the inquiry checks; with one unit it does not matter.
* **The palette is approximate** and the native animation follows any MIDI clock the unit received.
* **Legacy mode** (User Guide p20, another note map) is not handled specially: Programmer Mode is expected to leave it, which is unknown.
* **Settings and wording.** Settings › Devices lists and connects the Mini but does not show brightness, sleep, text or firmware yet, and some app text still names only the Launchpad X (M13).
* **The simulator** on screen stays the Virtual Launchpad X. A virtual Mini exists in the code and its tests but is not offered in the app.
* Linux needs the ALSA sequencer (`libasound2`).

## Verification status

| What | How it is checked | Status |
| --- | --- | --- |
| Every byte sent and read | `novation/protocol_tests.rs`, against the manual's hex lines, each test naming its page | run here, 2026-10-06 |
| Port names on macOS, Windows (both styles) and Linux (both kernels) | `novation/ports_tests.rs`, fixtures derived from documents and community projects | run here; names NOT observed on any system |
| Taking, keeping and giving back; lighting; inputs; unplug and replug | `novation/device_tests.rs`, `novation/session_tests.rs`, a simulated Mini behind fake ports | run here |
| Through the whole runtime | `core/runtime/device_tests.rs` | run here |
| Giving the unit back when the app quits | the exit handler | written, NOT VERIFIED in the app window (f) |
| Anything on a physical Mini MK3: ports, inquiry, presses, colours, timing, brightness, sleep, text, release | the acceptance suite, `scripts/launchpad-acceptance.sh` | **NOT HARDWARE VERIFIED (b)**, 2026-10-06: run here once without a unit only |
| Windows, Linux | | NOT VERIFIED (d, e): no machine here |

## Trying it on a unit

1. Quit anything else using the Mini (Ableton Live, Novation Components).
2. Plug it in. Within 3 s Settings › Devices lists "Launchpad Mini MK3" as connected (press Look again otherwise); the pads show the surface on screen.
3. Run the acceptance suite, `sh scripts/launchpad-acceptance.sh`, and answer on the unit ([hardware-testing.md](hardware-testing.md#with-a-unit-the-acceptance-suite)). It writes a report; commit it under `docs/verification/` and the open questions get their answers.
