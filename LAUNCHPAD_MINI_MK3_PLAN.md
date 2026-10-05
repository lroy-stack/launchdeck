# Launchpad Mini MK3 plan: first-class support and a shared Novation MK3 layer

Owner's brief: `MINI_MK3_BRIEF.md` (session scratchpad, 2026-10-05). This plan answers its 17 research questions, decides the device-layer architecture and splits the work into packages M1 to M13 (section 16.4). `PRODUCTIZATION_PLAN.md` stays the single architectural arbiter of the milestone; package M1 and M12 record the decisions below in it (crate confinement, the device lane, the deferred Tier 3 items).

**Hardware status of everything in this plan: NOT HARDWARE VERIFIED.** No Novation device is attached to this Mac (2026-10-05), there is no Windows machine and no Linux desktop here. Protocol facts come from Novation's documents; port names per OS come from Novation screenshots, operating-system sources and community projects. Nothing below was run on a Launchpad Mini MK3 or a Launchpad X.

Base of the code facts: worktree `/Users/lr0y/Movies/launchdeck-ux`, branch `productization-ux`, commit `0c944ea` (E2 done: `db6304b`, `f94f9a9`, `0c944ea`). The main tree (`productization`, `18578a1`) still has the pre-E2 device files of `de2e6bd`; the device lane must not start from it.

Sources, as cited below (printed page numbers equal PDF page numbers):

| Key | Document | Kind |
| --- | --- | --- |
| MINI-PRM | Novation, *Launchpad Mini [MK3] Programmer's Reference Manual*, PDF 2019-11-08, 23 pp | official |
| X-PRM | Novation, *Launchpad X Programmer's Reference Manual*, PDF 2019-11-08, 29 pp | official |
| PRO3-PRM | Novation, *Launchpad Pro [MK3] Programmer's Reference Guide*, PDF 2020-04-15 | official |
| MK2-PRM | Novation, *Launchpad MK2 Programmer's Reference Manual* v1.03 | official (older model, contrast only) |
| MINI-UG | Novation, *Launchpad Mini User Guide* v2.0 (English), PDF 2024-02-09 | official |
| X-UG | Novation, *Launchpad X User Guide* v2, PDF 2024-01-25 | official |
| NOV-ABL | Novation support article 360010019559 "How to set up the Launchpad Mini MK3 with Ableton Live" (updated 2026-09-18), screenshots | official |
| NOV-FW2 | Novation article "Launchpad Mini and X 2.0 Firmware Update" (undated; Synthtopia dates it 2022-06-29) | official |
| ABL12 | Ableton Live 12 MIDI Remote Scripts, decompiled mirror `gluon/AbletonLive12_MIDIRemoteScripts` (`Launchpad_Mini_MK3/*`, `Launchpad_X/*`, `novation/*`) | community (Ableton's shipped code, not Novation) |
| MS-MIDI | Microsoft `microsoft/MIDI` docs `kb/how-midi1-port-names-are-generated.md`, `kb/midi1-name-mapping.md` (2026-10-02) | official for Windows, not Novation |
| LINUX | Linux kernel `sound/usb/midi.c`, `sound/core/seq/seq_midi.c`, `seq_clientmgr.c`, `include/sound/rawmidi.h` | official for Linux, not Novation |
| MIDIR | `midir` 0.11.0 source (the version in `Cargo.lock`) | crate source |
| community | `ruohki/lunchpad` (captured X inquiry reply), `kangalio/launchy`, `obeezzy/lpminimk3`, `skymeyer/go-show-control`, `keysmashes/lp`, `git-moss/DrivenByMoss`, `bitwig/bitwig-extensions`, `hdavid/Launchpad95` | community |

Confidence words: **official** (a Novation document says so), **community-confirmed** (several independent non-Novation sources agree), **inferred** (our reasoning from sources), **unknown**.

---

## 1. Official protocol findings

The Mini and X manuals were diffed after normalisation: the text matches byte for byte apart from the product byte (Mini `0Dh`, X `0Ch`), the port-name stems, the Live-mode fallback layout and the X-only commands. The Programmer-mode diagram, the palette swatches and the flash/pulse waveform figures are byte-identical images in both manuals (SHA-1 of the extracted images). Every protocol fact below was re-read in the local copies of MINI-PRM pages 5-23 and MINI-UG pages 18-23 while writing this plan.

### Q1. MIDI ports per operating system

**Answer.** The Mini exposes two USB-MIDI interfaces: `LPMiniMK3 DAW In / Out (or first interface on Windows)` for DAWs and Session mode, and `LPMiniMK3 MIDI In / Out (or second interface on Windows)` for Custom modes, Lighting Custom modes and Programmer mode (MINI-PRM p6, official). What `midir` reports differs by system:

| System | Input names (computer receives) | Output names (computer sends) | Confidence |
| --- | --- | --- | --- |
| macOS (CoreMIDI `kMIDIPropertyDisplayName` = device + endpoint) | `Launchpad Mini MK3 LPMiniMK3 DAW Out`, `Launchpad Mini MK3 LPMiniMK3 MIDI Out` | `Launchpad Mini MK3 LPMiniMK3 DAW In`, `Launchpad Mini MK3 LPMiniMK3 MIDI In` | community-confirmed (NOV-ABL 2024 screenshot shows `Launchpad Mini MK3 (LPMiniMK3 MIDI Out)`; Bitwig, DrivenByMoss; MIDIR `coremidi/mod.rs:77-81`) |
| macOS, reported variant | `Launchpad Mini MK3 LPMiniMK3 DAW`, `Launchpad Mini MK3 LPMiniMK3 MIDI` (no In/Out) | same | unknown (launchy; E2's test fixture) |
| Windows classic (WinMM names, the default) | `LPMiniMK3 MIDI` (**the DAW interface**), `MIDIIN2 (LPMiniMK3 MIDI)` (the MIDI interface) | `LPMiniMK3 MIDI`, `MIDIOUT2 (LPMiniMK3 MIDI)` | official (NOV-ABL Windows screenshot; MS-MIDI naming rule) |
| Windows, second identical unit | `2- LPMiniMK3 MIDI`, `MIDIIN2 (2- LPMiniMK3 MIDI)` (marker not stable across restarts) | `2- …`, `MIDIOUT2 (2- …)` | inferred from MS-MIDI |
| Windows MIDI Services "new style" (Automatic naming, in preview 2026-10-04) | `LPMiniMK3 DAW Out`, `LPMiniMK3 MIDI Out`; second unit `… (2)` | `LPMiniMK3 DAW In`, `LPMiniMK3 MIDI In` | inferred from MS-MIDI rules |
| Linux ALSA, kernel ≥ 5.13 (`client:port-name c:p`, names cut at 31 chars) | `Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 DA 20:0`, `…LPMiniMK3 MI 20:1` | the same strings (duplex sequencer ports) | community-confirmed (launchy, lpminimk3, DrivenByMoss) + LINUX |
| Linux ALSA, kernel < 5.13 | `Launchpad Mini MK3:Launchpad Mini MK3 MIDI 1 20:0` (DAW), `…MIDI 2 20:1` (MIDI) | same | community-confirmed + LINUX |

Launchpad X equivalents: macOS `Launchpad X LPX MIDI Out/In`; Windows classic `LPX MIDI` (DAW) and `MIDIIN2 (LPX MIDI)` / `MIDIOUT2 (LPX MIDI)`; Linux `Launchpad X:Launchpad X LPX MIDI In 28:1` (no truncation). The Pro MK3 orders its interfaces differently (MIDI first, DIN second, DAW third; PRO3-PRM p6), so interface order is per-model data. On Windows 10 a WinMM port is exclusive: a port held by Ableton or Components cannot be opened (MS-MIDI). **NOT VERIFIED on any system here.**

### Q2. Which port carries Programmer Mode

**Answer.** The MIDI interface (second on Windows). Programmer-mode events are "sent by the device on the MIDI interface" (MINI-PRM p10); lighting by MIDI events goes "through the MIDI interface" (p12); p6 assigns "Light controls … Programmer mode" to it. Notes and CCs sent to the DAW interface change hidden Session-layout state instead (p15). SysEx is accepted on the MIDI interface (community-confirmed: launchy, sophixer, Zynthian forum 2024-04). Decision: LaunchDeck opens only the MIDI-interface pair and never the DAW pair. Confidence: official.

### Q3. Reliable identification beyond port names

**Answer.** The Universal Device Inquiry. Host sends `F0 7E 7F 06 01 F7`; the Mini answers (17 bytes) `F0 7E 00 06 02 00 20 29 13 01 00 00 v1 v2 v3 v4 F7` in its application and `… 13 11 00 00 b1 b2 b3 b4 F7` in its bootloader; the four version bytes are one decimal digit each, the digits the bootloader shows on its pads (MINI-PRM p6, official).

Copy error: X-PRM p6 and PRO3-PRM p6 print the same `13 01` for their models. Ableton's shipped scripts use Mini `(19, 1)` = `13 01`, X `(3, 1)` = `03 01`, Pro MK3 `(35, 1)` = `23 01` (ABL12 `*/sysex_ids.py`), and a captured real X reply reads `F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7` (lunchpad test `real_launchpad_x_reply`). The USB product ids `0x0113` (Mini), `0x0103` (X), `0x0123` (Pro MK3) have the family byte as low byte (ABL12; not reachable through `midir`). Confidence: Mini official; X community-confirmed.

Second check: every Novation SysEx reply carries the model's header `F0 00 20 29 02 <product>`; a readback answered with product `0Dh` confirms a Mini (MINI-PRM p6-7, official). Decision (section 5): port names are hints; the inquiry decides model, firmware and bootloader; the product byte of the readback replies confirms the model.

### Q4. SysEx to enter, leave and read Programmer Mode

**Answer.** Enter `F0 00 20 29 02 0D 0E 01 F7`; leave to Live `F0 00 20 29 02 0D 0E 00 F7`; read `F0 00 20 29 02 0D 0E F7`, reply `F0 00 20 29 02 0D 0E <00|01> F7` (MINI-PRM p7-8). Selecting layout `7Fh` with `F0 00 20 29 02 0D 00 7F F7` also enters Programmer mode; layout readback is `F0 00 20 29 02 0D 00 F7` → `… 00 <layout> F7`. Mini layouts: `00` Session (DAW mode only), `04` Custom 1 (Drums), `05` Custom 2 (Keys), `06` Custom 3 (User, Lighting), `0D` DAW Faders (DAW mode only), `7F` Programmer (p7). Live via `0Eh 00` lands in Session when DAW mode is on, else in **Custom 2 (Keys)** (p8); the X lands in Note mode (`01`, X-PRM p8). Entering Programmer mode by SysEx disables the Setup entry (holding Session) until Live is selected again (p7-8). "Launchpad Mini always powers on in Live Mode" (MINI-UG p20). Confidence: official.

### Q5. Input mapping: 8×8 grid, top buttons, right-side buttons

**Answer.** Programmer layout (MINI-PRM p10 diagram, p13 examples; MINI-UG p23): grid note `10 × r + c` with `r` = 1 (bottom) … 8 (top) and `c` = 1 (left) … 8, so 11-18 is the bottom row and 81-88 the top row (lower left 11, upper left 81, lower right 18 are the manual's own examples). Top row CC 91-98 left to right (Up, Down, Left, Right, Session, Drums, Keys, User on the Mini, MINI-UG p8). Right column CC 89, 79, 69, 59, 49, 39, 29, 19 top to bottom (seven scene launch `>` buttons and Stop/Solo/Mute at 19). Logo index 99 is a lit position, not a button. Pads send Notes, top and right buttons send CCs; every position accepts either for lighting. LaunchDeck's `ControlPos` mapping (`row 0` = top) is `note = 10 × (8 − row) + col + 1`, top `91 + index`, side `89 − 10 × index`, logo 99: exactly what `launchpad_x.rs` already does. The existing comment "UNVERIFIED (diagrams only)" means not hardware-tested: the numbers are official. Confidence: official.

### Q6. Press and release

**Answer.** The device sends Note On with velocity 0 for releases and never `80h`; it accepts `80h` or `90h` velocity 0 (MINI-PRM p6, official). The press velocity of a Mini pad and the CC values of the buttons (expected `7Fh` press, `00h` release) are not in any Novation document; community drivers treat value > 0 as press, 0 as release (community-confirmed). The Mini has **no velocity and no pressure**: its command summary lacks `04h` velocity curve, `0Bh` aftertouch and `0Dh` fader velocity, which the X manual has (MINI-PRM p23 vs X-PRM p29); its product page claims neither. MINI-UG p18 ("velocity, aftertouch, and fader" pages) and MINI-PRM p15 (an aftertouch sentence) are copy errors from the X documents. Decision: value > 0 = press, 0 or `80h` = release; Mini presses are reported with velocity 127 and `capabilities.velocity = false`, `pressure = false`; `A0h`/`D0h` are ignored. The output channel of device-to-host events is not stated (expected channel 1); the decoder masks the channel. Confidence: official (encoding, no velocity); community-confirmed (values).

### Q7. Independently addressable LEDs

**Answer.** 81: 64 grid, 8 top, 8 right, 1 logo ("81 RGB LEDs", product page; "up to 81 <colourspec>" MINI-PRM p14; "logo LED may be addressed" MINI-UG p23). Confidence: official.

### Q8. Static colours, pulse, flash, direct RGB

**Answer.** Two ways (MINI-PRM p12-14):

* MIDI events on the MIDI interface: channel 1 (`90h` note / `B0h` CC) static palette colour, channel 2 (`91h`/`B1h`) flashing, channel 3 (`92h`/`B2h`) pulsing; the note/CC number is the LED index, the velocity/value a palette index (0 = off). Examples (p13): `90 0B 05` lower left static red, `91 51 13` upper left flashing green, `92 12 2D` lower right pulsing blue, `90 12 00` or `80 12 00` off (also stops a pulse).
* LED SysEx `F0 00 20 29 02 0D 03 <colourspec>… F7`, colourspec = `<type> <index> <data>`: type 0 static palette (1 byte), type 1 flash with **colour B then colour A**, type 2 pulse (1 byte), type 3 RGB `r g b` each 0-127. Official example: `F0 00 20 29 02 0D 03 00 0B 0D 01 0C 15 17 02 0D 25 F7` (pad 11 static yellow, pad 12 flashing between bright green 21 and dim green 23, pad 13 pulsing turquoise 37).

Flash alternates between colour A (the static or pulsing colour already set) and colour B at 50 % duty, one period per beat; pulse runs dark to full (waveform 25 % → 100 %) over two beats; both follow incoming MIDI clock, else 120 bpm or the last clock received: **500 ms flash and 1000 ms pulse by default** (p12). There is no frequency control. Direct RGB exists only as SysEx type 3; flash and pulse take palette indices only. The palette is fixed (128 entries) and published only as a swatch image without numbers (p11); no MK3 command redefines it. Confidence: official.

### Q9. Batched LED updates

**Answer.** One `03h` message carries up to 81 colourspecs of mixed types (p14). Sizes: full RGB frame 6 + 1 + 81 × 5 + 1 = 413 bytes; full palette frame 251 bytes; one RGB LED 13 bytes; one note 3 bytes. No readback. Confidence: official. Which strategy is faster on the device is not documented (section 8.4 decides by measurement).

### Q10. Brightness

**Answer.** `F0 00 20 29 02 0D 08 <0-127> F7`, readback `… 08 F7` → `… 08 <level> F7` (MINI-PRM p22). The Setup menu offers 8 levels; the mapping of 0-127 onto them, and whether a SysEx level survives a power cycle, are undocumented. The bootloader saves a brightness and "low brightness levels may be required for [the device] to be powered on by certain devices" (p5). Confidence: official (command); unknown (persistence).

### Q11. Scrolling text

**Answer.** `F0 00 20 29 02 0D 07 [<loop> [<speed> [<colourspec> [<text>]]]] F7` (MINI-PRM p20): loop `00`/`01`; speed in pads per second, values `40h` and above are negative (value − `80h`) and scroll left to right; colourspec `00 <palette>` or `01 <r> <g> <b>`; text in ASCII. Empty `F0 00 20 29 02 0D 07 F7` stops a scroll; omitting the text changes loop, speed or colour of the current scroll. The scroll covers the pads and the right column, not the top row or the logo, and "any lighting covered by the scroll is retained". Example: `F0 00 20 29 02 0D 07 01 07 00 25 48 65 6C 6C 6F 20 57 6F 72 6C 64 F7` ("Hello World", looping, 7 pads/s, palette 37); the manual's decimal line is wrong (prints 104 for `6Ch`), use the hex. No readback, no completion message and no in-text speed bytes are documented (the MK2 had both: MK2-PRM p14), maximum length and non-ASCII behaviour are undocumented. Confidence: official; unknown (limits, end of scroll).

### Q12. Sleep and wake

**Answer.** `F0 00 20 29 02 0D 09 <mode> F7` with **0 = LEDs off (sleep), 1 = on (wake)**; readback `… 09 F7` (MINI-PRM p21). Lights stay off "until turning them back on with this message or interacting with the device"; "Press any button or pad to wake up the device" (MINI-UG p19). Unknown: whether the waking press is also sent as MIDI and whether LED state is kept while asleep (the wording suggests it is). Confidence: official; unknown (waking press, retention).

### Q13. Settings that can interfere with the renderer

**Answer.**

* LED feedback `F0 00 20 29 02 0D 0A <internal> <external> F7`, readback `0A` (MINI-PRM p22). MINI-UG p19: internal feedback lights pads when pressed **in Custom Modes**; external feedback lights pads "when MIDI is received externally in Custom Modes and Programmer Mode"; both are on by default. A unit whose external feedback was switched off would not show note lighting in Programmer mode; whether the `03h` SysEx is affected is unknown. Official.
* MIDI Destination (MINI-UG p19, no SysEx, cannot be read or set) can mirror data to both USB ports; opening only the MIDI-interface input avoids duplicates either way. Official.
* Legacy mode (MINI-UG p20, no SysEx): another note map (grid 36-99). `0Eh 01` is expected to leave it (unknown). Official (mode exists).
* MIDI clock arriving at the device changes flash and pulse timing; the last tempo is kept (MINI-PRM p12). LaunchDeck never sends clock. Official.
* DAW mode `10h`, set by DAWs. Ableton's Mini script sends DAW mode on (`F0 00 20 29 02 0D 10 01 F7`) and then layout Session on identification, which takes the unit out of Programmer mode (ABL12 `launchpad_mini_mk3.py` `on_identified`). LaunchDeck never sends `10h`. Community-confirmed.

### Q14. State to restore on release

**Answer.** The unit powers on in Live mode (MINI-UG p20), so nothing LaunchDeck sets as mode survives a replug. "Remember to switch the device back to Live mode once done" (MINI-PRM p9). Live via `0Eh 00` lands in Keys (not in DAW mode), not in the layout the person had, so the layout must be read (`00h` readback) before acquisition and restored after (`00 <layout>`). LED feedback and brightness are readable and restorable. Never send `10h` (it belongs to a DAW: "When the DAW/software exits, it should send SysEx to revert the device to Standalone mode", MINI-PRM p15). No document describes what the device does when the host closes its ports or crashes; USB-MIDI 1.0 has no port-open state visible to the device (inferred), so after a crash the unit stays in Programmer mode with the Setup entry disabled until a host sends Live or the unit is replugged. Confidence: official (restorable state); inferred (crash).

### Q15. Firmware versions that matter

**Answer.** The manuals have no revision history and tie no command to a firmware version. Firmware 2.0 (NOV-FW2, 2022) added the Custom Mode keystroke widget on the Mini (and Custom 5-8 on the X, whose layout bytes are undocumented). MINI-UG v2.0 documents Legacy mode and MIDI Destination, absent from the 2019 manual (probably 2.x additions, inferred). How the four inquiry digits map to "2.0" is undocumented (a real X reads `0 2 3 8`). Updates go through Novation Components (MINI-UG p7). Decision: read and show the four digits; no version gate (nothing justifies one); warn only from evidence (a documented readback that a unit does not answer, section 12.4). Confidence: official (no ties); unknown (digit mapping).

### Q16. Telling several Minis apart

**Answer.** The bootloader sets a "USB device ID" 1-16; "If more than one Launchpad Mini [MK3] is used on the same USB, each should have a different device ID" (MINI-PRM p5); MINI-UG p22 says it gives each unit its own Session Ring in Ableton. Neither document says whether it changes the USB product id, the port names or byte 2 of the inquiry reply (printed `00h`). Per system: macOS gives identical names (only CoreMIDI unique ids differ; pairing sources with destinations by order is a guess), Linux gives each unit its own ALSA client number (unique but not stable across replug), Windows classic adds a leading `N- ` marker that "is not guaranteed to be the same across restarts" (MS-MIDI), new style appends ` (N)`. There is no serial number over MIDI. Decision: runtime ids `<model>:<n>` by enumeration order, nothing persisted; which physical unit is #1 may change after a replug (documented limitation); device-ID management is out of scope. Confidence: official (device ID exists); inferred (OS behaviour); unknown (effect of the device ID).

### Q17. What the Mini shares with the Launchpad X

| Shared (identical bytes apart from the product byte) | Model-specific |
| --- | --- |
| SysEx header `F0 00 20 29 02 <product>` and the readback mechanism | product byte: Mini `0Dh`, X `0Ch` |
| `0Eh` Programmer/Live with readback; `00h` layout select with readback (`00`, `04`, `05`, `06`, `0D`, `7F`) | inquiry family: Mini `13 01` (bootloader `13 11`), X `03 01` (manual misprint `13 01`) |
| `03h` LED SysEx (types 0-3, 81 colourspecs, mixed) | port-name stems `LPMiniMK3` / `LPX`; interface order (same for these two, different on the Pro MK3) |
| channel 1/2/3 note and CC lighting; flash and pulse timing; the 128-entry palette | Live fallback layout: Mini Keys `05`, X Note `01` |
| `07h` text, `08h` brightness, `09h` sleep, `0Ah` feedback, `10h` DAW mode, `01h` fader setup, `14h` Session colour (readbacks where listed) | X-only layouts `01` Note, `07` Custom 4 (Custom 5-8 from firmware 2.0, bytes undocumented) |
| the Programmer layout (11-88, 91-98, 89…19, logo 99), note-off handling | X-only commands `04h`, `0Bh`, `0Dh`, `0Fh`, `13h`, `15h`-`17h`; `12h` middle byte (Mini unused) |
| the Universal Device Inquiry message layout | grid velocity and aftertouch (X only); button labels |

The Pro MK3 differs more (product `0Eh`, family `23 01`, three interfaces) and stays out of the shared layer. Confidence: official (mechanical diff of the two manuals and their command summaries; image hashes).

Known copy errors in the official documents, which tests must not copy: (1) family `13 01` printed for X and Pro MK3; (2) "hold the Capture MIDI button" for the Mini bootloader (MINI-PRM p5, MINI-UG p21; the Mini has no such button); (3) the aftertouch sentence of MINI-PRM p15; (4) the "velocity, aftertouch, fader" setup pages of MINI-UG p18; (5) the decimal line of the text example (MINI-PRM p20, X-PRM p23).

---

## 2. Current device architecture assessment

Read at `launchdeck-ux@0c944ea`.

### 2.1 What is there

* `devices/mod.rs` (202 lines): `SurfaceDevice` (`identity`, `capabilities`, `connect(InputSender)`, `disconnect`, `is_connected`, `render(&SurfaceFrame)`, `clear`), `DeviceCapabilities` (Copy: `grid`, `aux{top, side, logo}`, `rgb`, `velocity`, `pressure`, `animations{pulse, flash}`), `DeviceIdentity{id, name, model, kind}`, `Led`, `LedCell`, `SurfaceFrame`, `DeviceInput`, `InputKind::{Pressed{velocity}, Released}`, `DeviceError` (sentences + `detail()`), E2's `PortNames` and `PortSource` (with an X-specific factory method `launchpad_x(found)`).
* `devices/launchpad_x.rs` (465 production lines, the only `midir` user): pure protocol functions (`enter/exit_programmer_mode`, `led_index`, `pos_from_note/cc`, `decode_message`, `to_7bit`, `encode_rgb_updates` chunked at 81, `encode_flash`, `encode_pulse`), port classification (`classify_port`, `names_launchpad_x`, `is_other_launchpad`), E2 discovery (`discover` zips the filtered MIDI inputs and outputs by list order, `device_id` = `launchpad-x:` + slug of the input name, `unsupported_id`), `SystemPorts` (midir listing, two clients per scan), and the adapter `LaunchpadX` (output opened, `0E 01` sent, then input opened with `Ignore::All`, then `clear()`; render diffs a `HashMap<u8, Rgb>` and sends RGB colourspecs; disconnect sends clear and `0E 00`).
* `devices/manager.rs` (299 production lines): entries with `in_use`, `unplugged`, `problem`; E2's `DeviceStatus` (`connected`, `available`, `unplugged`, `unsupported`, `failed`), `DeviceInfo{identity, capabilities, connected, status, problem?}`, `Rescan{to_connect, changes}`, `CONNECT_PROBLEM`; disconnect-on-render-failure; `rescan_hardware(retry_failed)` building a new device for a returning unit.
* `devices/virtual_device.rs`: `VirtualLaunchpadX` (`VIRTUAL_ID = "virtual-launchpad-x"`, X capabilities with native pulse/flash animated by CSS at 1.4 s / 0.7 s), `VirtualHandle` (press/release/frame/on_change). `VIRTUAL_ID` and `Runtime::virtual_handle()` are used by `core/runtime/tests.rs` and integration tests; neither may change.
* `renderer/mod.rs`: `compose` gives one `LedCell` per capability position; `adapt` passes `Pulse`/`Flash` through when the capability says native, otherwise synthesises (pulse 1400 ms cosine 0.25-1.0, flash 700 ms 50 %). No protocol knowledge; needs no change.
* Runtime (`core/runtime/devices.rs`, `startup.rs`): a 40 ms render tick under the runtime mutex calls `compose` and `DeviceManager::render` per connected device; a 3 s rescan on the blocking pool takes the same mutex to rescan and connect; inputs flow through one unbounded channel to `handle_input`. Every device I/O happens under the runtime lock.
* UI (E2): `src/lib/devices.ts` (`statusText` with "Not supported yet: LaunchDeck works with the Launchpad X"), `DeviceStatusLine.tsx` ("Plug in a Launchpad X"), `src/features/settings/DevicesPanel.tsx` (Look again, Connect/Disconnect, Technical details mentioning Programmer mode and the "LPX MIDI port").
* Tests: 9 byte and discovery tests in `launchpad_x.rs`, 9 manager tests (E2's `FakePorts`/`FakeUnit` replace whole devices, so the adapter's real bytes are never exercised against a fake), 4 virtual-device tests, 7 renderer tests, runtime tests over the virtual device.

### 2.2 Classification

| Piece | Class | Decision |
| --- | --- | --- |
| `SurfaceDevice`, `DeviceCapabilities`, `ControlPos`, `SurfaceFrame`, `Led`, `DeviceInput`, renderer, manager status model | generic | keep; extend with generic, optional additions (section 11) |
| `DeviceCapabilities` fields | generic | keep **unchanged** (the artifacts lane builds it by struct literal in `artifacts/export/tests.rs`; a new field would break that code after the merge) |
| programmer enter/exit, `led_index`, `pos_from_note/cc`, `decode_message`, `to_7bit`, colourspec encoders, chunking at 81 | MK3-family | extract to `devices/novation/` parameterised by the product byte |
| render diff and `HashMap<u8, Rgb>` cache | MK3-family, reusable | generalise to an LED-state cache that also knows native flash/pulse (section 8.5) |
| `classify_port`, `discover` (zip by order), `device_id` slug, `is_other_launchpad` | X-specific and defective | replace with a per-system port parser and unit pairing shared by every MK3 model (section 6) |
| `SystemPorts` and the adapter's `midir` calls | generic transport | move to `devices/midi/system.rs`, the only `midir` file |
| X capabilities, name, model id, `LPX` tokens, product `0Ch` | X-specific | keep as X profile data |
| E2 `PortSource::launchpad_x` | X-specific in a generic file | replace with a generic `HardwareSource` |
| `VirtualLaunchpadX` | generic with X data baked in | parameterise by a virtual model; keep `VirtualLaunchpadX::new()` and `VIRTUAL_ID` |

### 2.3 Defects found (documented here so that fixes are "documented bugs")

| Id | Defect | Where | Fixed by |
| --- | --- | --- | --- |
| D-X1 | Windows classic: `LPX MIDI` is the **DAW** interface; it is classified as MIDI, and one X becomes two units, the auto-connected one bound to the DAW interface (no Programmer input there) | `classify_port`, `discover` | M3 |
| D-X2 | Linux kernels < 5.13 name both interfaces `Launchpad X MIDI 1/2`: both classified MIDI, two units | same | M3 |
| D-X3 | Ids slug the OS port name: two identical units collide and `register()` drops the second; Linux ids embed the ALSA client number, which changes on replug | `device_id` | M3 |
| D-X4 | Other models listed per port: a Linux Mini twice (`…LPMiniMK3 DA` has no "daw"), a Windows-classic Mini never (`LPMiniMK3 MIDI` has no "launchpad") | `is_other_launchpad` | M3 |
| D-X5 | Programmer mode is entered blind: a unit in its bootloader or another model would be driven; firmware unknown; Live-mode notes during connect could be read as pad presses | `connect` | M5 |
| D-X6 | Programmer mode is never confirmed (input opened with `Ignore::All`, so no reply can arrive anyway) | `connect` | M1 (SysEx delivered), M5 |
| D-X7 | Release lands in Note mode (X) / Keys (Mini), not the layout the person had | `disconnect` | M5 |
| D-X8 | A unit whose external LED feedback or LED sleep was set in its Setup menu shows nothing while LaunchDeck draws | `connect` | M5 |
| D-X9 | A quick power cycle (within the 3 s scan) or another program switching the layout goes unnoticed: unit in Live mode, no input, stale cache | manager + adapter | M5 |
| D-X10 | Quitting LaunchDeck leaves the unit in Programmer mode with Setup disabled (no `RunEvent` handler, no `Drop`) | `lib.rs`, adapter | M8 |
| D-X11 | Output opened and `0E 01` sent before the input exists; an input-open failure leaves a half-open adapter that reads as connected | `connect` | M1 |
| D-L1 | Device I/O under the runtime lock: any wait for a SysEx reply there would freeze input and navigation | runtime | acquisition is a non-blocking state machine (section 9) |

---

## 3. Launchpad X overlap

The X adapter is a correct but minimal MK3 driver: its bytes for Programmer entry/exit, RGB lighting and input decoding are exactly what the Mini needs with `0Dh` instead of `0Ch`. What it lacks (identity, readback, restore, native lighting, settings, release on quit, per-system ports) is lacking for both models. So the Mini does not get its own copy: both models run on one generic MK3 device driven by a per-model profile, and the X keeps its observable behaviour except where a defect of section 2.3 is fixed, each fix in its own commit with a test that pins the X bytes before and after.

Rules that keep the X intact:

* **Golden transcript.** M1 adds a test that records every byte the X sends over a fake transport (connect, first frame, a diff, an unchanged frame, disconnect) and every input it decodes. Every later package must keep it green; the only commits allowed to change it are the ones named in section 2.3, and each changes it together with a test of the fixed behaviour.
* **Capabilities unchanged.** The X keeps `velocity: true`, `pressure: false`, `animations: {pulse: false, flash: false}`. Its frames therefore never carry native animations, and its lighting stays RGB type-3 colourspecs, byte-identical. Turning on native flash/pulse for the X is a one-line profile change left for after a hardware check (section 16.3).
* **New features cost no bytes until used.** Brightness, sleep and text are offered for the X too (same commands, X-PRM p23-25), but send nothing unless the person uses them.

---

## 4. Shared protocol layer and module layout

### 4.1 Decision

A shared Novation MK3 layer is justified (section 1, Q17). It holds only what both manuals define identically: framing, the commands LaunchDeck uses (`00h`, `03h`, `07h`, `08h`, `09h`, `0Ah`, `0Eh` and their readbacks), the inquiry, the reply parser, the Programmer layout, channel lighting and the palette. Model data lives in profiles; there is no `if model == …` anywhere. X-only commands (`04h`, `0Bh`, `0Dh`, `0Fh`, `13h`, `15h`-`17h`), DAW-port commands (`01h`, `10h`, `12h`, `14h`) and the Pro MK3 stay out.

### 4.2 Module layout (`src-tauri/src/devices/`)

```
devices/
  mod.rs              SurfaceDevice, DeviceCapabilities (unchanged), frame and input types, DeviceError;
                      declarations and re-exports (R13)
  features.rs         generic optional features: DeviceFeatures, DeviceDetails, ModeState,
                      DeviceSetting, TextScroll, ScrollDirection (section 11)
  source.rs           HardwareSource trait, HardwareScan, Found (generic; replaces E2's PortSource/PortNames)
  manager.rs          generic; no model names
  virtual_device.rs   VirtualDevice parameterised by a VirtualModel; VirtualLaunchpadX::new() and VIRTUAL_ID kept
  midi/
    mod.rs            PortInfo {name, id}, PortSnapshot, MidiBackend, MidiLink (no midir)
    system.rs         THE ONLY midir FILE: enumerate with ids, open an input+output pair (SysEx on)
    framing.rs        MessageFramer: bytes from a callback into whole MIDI messages (pure)
    testkit.rs        #[cfg(test)] FakeMidi in-memory backend
  novation/
    mod.rs            declarations, re-exports, module doc
    protocol.rs       pure MK3 codec: header(product), commands, readback queries, inquiry,
                      reply parser, colourspecs, chunking, channel lighting, text
    layout.rs         Programmer-mode index map and input decoding (shared)
    palette.rs        the 128-entry approximate palette and nearest-colour lookup (section 8.3)
    profile.rs        ModelProfile (data only) and virtual_model(profile)
    launchpad_x.rs    the X profile
    launchpad_mini_mk3.rs  the Mini profile
    ports.rs          per-system port-name parsing and unit pairing (takes Platform)
    hardware.rs       NovationHardware: HardwareSource over a MidiBackend
    session.rs        acquisition, ownership and release state machine (pure)
    leds.rs           LED-state cache and lighting planner
    device.rs         Mk3Device: SurfaceDevice gluing link, session, leds, layout
    testkit.rs        #[cfg(test)] SimLaunchpad: a byte-level device model written from the manual
    protocol_tests.rs, ports_tests.rs, device_tests.rs, measure_tests.rs, hardware_tests.rs (#[ignore])
```

Module privacy (R15): `mod midi; mod novation; mod features; mod source;` are private in `devices/mod.rs`; types the runtime, commands or UI need are re-exported with `pub use` (`DeviceFeatures`, `DeviceDetails`, `ModeState`, `DeviceSetting`, `TextScroll`, `ScrollDirection`); everything else is `pub(crate)` or private. `pub mod launchpad_x` disappears from the public API (nothing outside `devices` uses it; checked with grep at `0c944ea`).

Dependency rule (R2): everything above stays inside `crate::{devices, core::model}` plus `platform`. `ports.rs` takes `Platform` as a parameter (R3; `cfg!` is not allowed under `devices/**`).

### 4.3 Crate confinement move

`midir` moves from `devices/launchpad_x.rs` to `devices/midi/system.rs`. In the same commit (M1): `scripts/architecture/rust.mjs` (`crateConfinement` midir row), `scripts/check-architecture.test.mjs` (lines 149-150), `docs/architecture/CODEBASE_MAP.md` (line 13 "A new device model", line 78 `devices` row, line 135 confinement), `docs/architecture/ENGINEERING_RULES.md` R5, `docs/architecture/overview.md` (lines 31-34, 51, 59), `AGENTS.md` line 34, `PRODUCTIZATION_PLAN.md` line 196 (5.1 crate confinement), `src-tauri/src/lib.rs` module doc line 9.

### 4.4 Transport seam

```rust
// devices/midi/mod.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortInfo { pub name: String, pub id: String }        // midir port_name() and id()
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PortSnapshot { pub inputs: Vec<PortInfo>, pub outputs: Vec<PortInfo> } // OS order
pub(crate) type OnBytes = Box<dyn FnMut(&[u8]) + Send + 'static>;

pub(crate) trait MidiBackend: Send + Sync {
    fn ports(&self) -> PortSnapshot;
    /// Opens the input first (SysEx delivered: Ignore::TimeAndActiveSense), then the output.
    /// Both or neither: an output failure closes the input.
    fn open(&self, input: &PortInfo, output: &PortInfo, on_bytes: OnBytes)
        -> Result<Box<dyn MidiLink>, DeviceError>;
}
pub(crate) trait MidiLink: Send {
    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError>;
    fn close(self: Box<Self>);
}
```

`system.rs` resolves a `PortInfo` by name **and** id (on Windows the ports of one unit may share an interface path id, MIDIR `winmm/mod.rs`; names differ there), never by the first name match. `FakeMidi` (testkit) scripts port lists per OS fixture, records sent messages per output, injects bytes into an open input, can refuse opens (busy port), remove ports (unplug) and, from M4, binds an output to a `SimLaunchpad` that answers synchronously.

---

## 5. Identity and detection

1. **Discovery** (every 3 s and on Look again): `NovationHardware::scan()` lists ports through the backend and parses them (section 6). A unit = a model profile + a MIDI-interface input/output pair. Port names are hints only.
2. **Identity** (on connect, section 9): the inquiry reply must come from Novation (`00 20 29`) with a family in the profile's accepted list. Mini: `13 01`. X: `03 01` and the misprinted `13 01` (the readback product byte decides). A family whose second byte is `11h` with the profile's first byte is the bootloader: LaunchDeck sends nothing more, closes and reports "The Launchpad is in its bootloader. Unplug it and plug it in again." Another family: closes and reports "LaunchDeck could not confirm which Launchpad this is. Press Look again." No reply within the timeout: continue unconfirmed (some drivers may drop SysEx) and say so in a notice.
3. **Model confirmation**: readback replies must carry the profile's product byte; replies with another product byte are ignored (logged).
4. **Firmware**: the four inquiry digits, shown joined as the bootloader shows them (e.g. `0531`), in `DeviceDetails.firmware`. Mapping to Novation's marketing numbers is unknown and not attempted.
5. **Identity record**: `DeviceIdentity { id: "launchpad-mini-mk3:1", name: "Launchpad Mini MK3", model: "launchpad-mini-mk3", kind: Hardware }`; unit 2 and later get ` (2)` … in the name and `:2` … in the id. Nothing about ports is persisted.

---

## 6. MIDI-port selection

`devices/novation/ports.rs`, pure, `fn parse(snapshot: &PortSnapshot, platform: Platform, profiles: &[&ModelProfile]) -> Parsed { units, unsupported }`.

Per port (each direction separately):

1. **Unit marker and direction.** Linux: strip the trailing ` <client>:<port>` (client = unit key, port number kept). Windows: strip a `MIDIIN<n> (…)` / `MIDIOUT<n> (…)` wrapper (n ≥ 2 is interface n), a leading `<N>- ` or `<N> - ` marker (unit key N, default 1), a trailing ` (<N>)` new-style marker (also after the device part: `Launchpad X (2) LPX MIDI Out`). All systems: strip a trailing ` In` / ` Out` (direction hint, optional).
2. **Model.** Tokenise on non-alphanumerics, case-insensitive. Mini: token `lpminimk3`, or the sequence `launchpad mini mk3`. X: token `lpx`, or `launchpad` followed by exactly `x` (so `Launchpad XL` is not an X). Other ports with `launchpad` or `lppromk3`: another Novation model (unsupported list).
3. **Role.** The token after the stem token (`lpminimk3`, `lpx`): `daw` or the truncated `da` = DAW; `midi` or `mi` not followed by a number = MIDI. `midi <n>` (Linux < 5.13: `Launchpad Mini MK3 MIDI 2`) = interface n. Windows classic: a wrapper with n ≥ 2 = interface n (overrides the inner word "MIDI"); a bare name without wrapper and without In/Out on `Platform::Windows` = interface 1. Linux without a role token: ALSA port p = interface p + 1. Interface n maps to a role through `profile.interfaces` (Mini and X: `[Daw, Midi]`).
4. **Units.** Group MIDI-role ports by (model, unit key). Linux pairs input and output by identical `midir` id (a duplex sequencer port). Windows pairs within a unit key; if two MIDI inputs share a key (models that gave identical names, MS-MIDI), split by port id. macOS (identical names for identical units) pairs the k-th MIDI-role input with the k-th MIDI-role output: a guess the inquiry then confirms (the reply must arrive on the paired input; otherwise the notice of section 9 applies).
5. **Ids.** Units of a model are numbered 1, 2, … in the order of their MIDI input in the OS list: `launchpad-mini-mk3:1`. DAW ports are recognised and never opened. Another Novation model is listed once per unit as `unsupported:<slug of its name>` (`-2` for a second unit).

Fixtures (section 14.3) cover every row of Q1's table. Every fixture is labelled "derived from docs/community, not observed on this system".

---

## 7. Input mapping

`devices/novation/layout.rs`, shared:

| Control | `ControlPos` | Device sends | Decoded as |
| --- | --- | --- | --- |
| 64 pads | `Grid{col 0-7, row 0-7}` (row 0 top) | Note `10 × (8 − row) + col + 1` (11-88) | `Pressed` if velocity > 0, `Released` if 0 or `8xh` |
| 8 top buttons | `Top{index 0-7}` left to right | CC 91-98 | value > 0 press, 0 release |
| 8 right buttons | `Side{index 0-7}` top to bottom | CC 89, 79, … 19 | same |
| logo | `Logo` | nothing (output only) | CC/note 99 ignored |

The decoder masks the channel. `A0h`/`D0h` (pressure), clock, active sensing, unknown CCs and notes outside the map are ignored; SysEx goes to the reply path, never to input. Press velocity: passed through when `capabilities.velocity` is true (X, unchanged), reported as 127 otherwise (Mini). Input is **gated**: nothing is forwarded until Programmer Mode is entered (and again after a detected Live fallback), because Live-mode layouts send notes in the same range (Keys, Drums). `devices/midi/framing.rs` splits a callback's bytes into whole messages (two messages in one callback, a SysEx split over callbacks, stray data bytes dropped). Raw note and CC numbers never leave `devices/novation`.

---

## 8. LED mapping

### 8.1 Indices

The LED index of a position is its Programmer index (section 7, plus logo 99): 81 distinct indices, one full message.

### 8.2 Palette or RGB, chosen by the device layer

The renderer stays semantic: it gives `Led { color: Rgb, animation }` and passes `Pulse`/`Flash` through only when `capabilities.animations` says native. The device chooses:

| Frame LED | Colourspec sent | Why |
| --- | --- | --- |
| `animation: None` | type 3 RGB, each channel `to_7bit` (exact colour) | RGB is only available as SysEx type 3; exact |
| `Pulse` (native only) | type 2, palette `nearest(color)` | hardware pulse takes a palette index (p14) |
| `Flash` (native only) | type 1, B = `nearest(color)`, A = 0 | flashes between the colour and off, LaunchDeck's flash semantics |
| off | type 3 `0 0 0` | same as the X today (byte identity) |

Timing on hardware is the device's: flash 500 ms, pulse 1000 ms at the default 120 bpm, or another tempo if another program sent MIDI clock to the unit since power-on (p12). LaunchDeck's synthesised timing (700/1400 ms) is used only where animation is not native (the X). There is no fake frequency control. `DeviceFeatures.flash_period_ms/pulse_period_ms` report 500/1000 for the Mini.

### 8.3 Palette values

No numeric palette is published. `palette.rs` uses the table below, **approximate**: derived from the official swatch image (MINI-PRM p11, byte-identical to X-PRM p12) by taking each swatch's fill colour and removing the image's grey floor, `c = round((s − 0x61) × 255 / 0x9E)` clamped to 0-255. Checks against the manual's named examples: 5 red `(255,0,0)`, 13 yellow `(255,255,0)`, 21 bright green and 23 dim green (the p14 flash example), 37 turquoise `(0,228,255)`, 45 blue `(0,0,255)`, 0 off. Not measured on LEDs (NOT HARDWARE VERIFIED); the acceptance suite shows the whole palette for a visual check.

`nearest(c)`: weighted squared distance `2Δr² + 4Δg² + 3Δb²` over entries 1-127 (entry 0 only for `Rgb::OFF`), first index wins ties. `palette_rgb(i)` returns the table colour (used by the virtual Mini, section 13).

```rust
pub(crate) const PALETTE: [(u8, u8, u8); 128] = [
    (0, 0, 0), (132, 132, 132), (200, 200, 200), (255, 255, 255), (255, 132, 132), (255, 0, 0), (200, 0, 0), (132, 0, 0), // 0-7
    (255, 236, 187), (255, 132, 0), (200, 69, 0), (132, 34, 0), (255, 228, 103), (255, 255, 0), (200, 200, 0), (132, 132, 0), // 8-15
    (200, 255, 103), (157, 255, 0), (103, 200, 0), (52, 132, 0), (157, 255, 132), (0, 255, 0), (0, 200, 0), (0, 132, 0), // 16-23
    (157, 255, 157), (0, 255, 69), (0, 200, 34), (0, 132, 16), (157, 255, 173), (0, 255, 173), (0, 200, 103), (0, 132, 52), // 24-31
    (157, 255, 236), (0, 255, 219), (0, 200, 157), (0, 132, 86), (157, 236, 255), (0, 228, 255), (0, 165, 200), (0, 103, 132), // 32-39
    (157, 200, 255), (0, 165, 255), (0, 103, 200), (0, 52, 132), (103, 69, 255), (0, 0, 255), (0, 0, 200), (0, 0, 132), // 40-47
    (173, 132, 255), (103, 0, 255), (52, 0, 200), (34, 0, 132), (255, 132, 255), (255, 0, 255), (200, 0, 200), (132, 0, 132), // 48-55
    (255, 132, 187), (255, 0, 157), (200, 0, 103), (132, 0, 69), (255, 34, 0), (219, 132, 0), (200, 157, 0), (103, 103, 0), // 56-63
    (0, 132, 0), (0, 132, 69), (0, 69, 187), (0, 0, 255), (0, 132, 132), (69, 0, 236), (173, 132, 157), (69, 34, 52), // 64-71
    (255, 0, 0), (236, 255, 103), (228, 250, 0), (173, 255, 0), (34, 200, 0), (0, 255, 173), (0, 219, 255), (0, 103, 255), // 72-79
    (69, 0, 255), (173, 0, 250), (228, 69, 200), (103, 34, 0), (255, 103, 0), (200, 245, 0), (187, 255, 69), (0, 255, 0), // 80-87
    (132, 255, 103), (173, 250, 187), (132, 255, 240), (173, 211, 255), (103, 157, 240), (187, 157, 245), (245, 69, 255), (255, 0, 173), // 88-95
    (255, 157, 0), (236, 228, 0), (211, 255, 0), (200, 173, 0), (132, 103, 0), (0, 144, 34), (34, 157, 69), (52, 52, 103), // 96-103
    (52, 69, 173), (173, 118, 52), (200, 0, 0), (245, 132, 103), (245, 144, 34), (255, 236, 69), (219, 245, 103), (187, 228, 34), // 104-111
    (52, 52, 103), (245, 245, 187), (200, 250, 211), (219, 219, 255), (211, 187, 255), (132, 132, 132), (187, 187, 187), (245, 255, 255), // 112-119
    (219, 0, 0), (118, 0, 0), (52, 240, 0), (0, 132, 0), (236, 228, 0), (132, 103, 0), (228, 157, 0), (157, 34, 0), // 120-127
];
```

### 8.4 Batching, decided by measurement

Default (M4, unchanged for the X): every change of one render tick goes into **one `03h` message** (types mixed, chunked at 81 colourspecs) in the frame's position order. Reasons: static colours need RGB, which only SysEx carries; one message per tick is the fewest messages; it is what the X sends today. Note/CC lighting (3 bytes per LED, palette only) stays implemented in `protocol.rs` but unused (`NOTE_LIGHTING_MAX_CHANGES = 0` in `leds.rs`).

The implementer of M10 runs `measure_tests.rs` (ignored measurement test) and records, for one LED, eight LEDs, a 64-pad page switch and an 81-LED redraw: bytes on the wire, USB-MIDI event packets (a 3-byte channel message is one 4-byte packet; an n-byte SysEx is ⌈n/3⌉), messages per tick, and host encode-plus-send time over the fake link (median of 1000 runs). Host-side numbers cannot show device latency; the acceptance suite (M11) measures device processing with a readback barrier (send the lighting, then `0Eh` query; the reply arrives after the device processed everything before it) for notes vs SysEx. Rule recorded in `docs/devices/hardware-testing.md`: keep one batch per tick unless the hardware measurement shows per-LED notes faster by more than 1 ms for palette-only diffs of ≤ k LEDs; then set `NOTE_LIGHTING_MAX_CHANGES = k` in one commit.

### 8.5 LED cache and invalidation

`leds.rs` keeps `[Option<LedState>; 100]` indexed by LED index, `LedState = Rgb([u8; 3]) | Pulse(u8) | Flash { b: u8, a: u8 }` in device units (7-bit RGB or palette). `None` = unknown. A render sends only entries whose desired state differs, then stores them after every message was sent. Events:

| Event | Cache | Next render sends |
| --- | --- | --- |
| Programmer Mode entered (first or re-acquired) | an explicit clear frame (81 × RGB off, as the X does today), then all `Rgb(0,0,0)` | every non-off LED |
| wake from sleep, brightness change, text scroll stopped or estimated over | all `None` | all 81 LEDs in one message (no blink) |
| device power cycle or takeover detected | phase goes back to acquisition | as the first acquisition |
| disconnect | dropped | — |

Open hardware question (risk, acceptance step): whether a static RGB colourspec stops a running flash or only changes its colour A. If hardware shows flash persisting, the planner first sends type 0 palette 0 for an LED leaving `Flash`; one commit, with a test.

---

## 9. Lifecycle and mode ownership

### 9.1 State machine (`novation/session.rs`, pure; advanced by `render()` every 40 ms)

Nothing waits under the runtime lock: `connect()` opens the link and sends the probe, then returns; each `render()` drains replies (pushed by the input callback into a queue), advances the phase and sends what the new phase needs. The clock is injected (`Arc<dyn Fn() -> Instant + Send + Sync>`) so tests control timeouts.

```
connect ──▶ Probing ──(inquiry + readbacks answered, or 500 ms)──▶ Entering ──(0Eh reply or 500 ms)──▶ Owned
              │ bootloader / other family                                                              │ heartbeat 0Eh = 00
              ▼                                                                                        ▼
           Refused (sends nothing more; render → Err; Failed + sentence)              Probing again (re-acquire, full redraw)
                                                                                       │ 3 takeovers in 30 s
                                                                                       ▼
                                                                                    Yielded (sends nothing; Failed + sentence)
disconnect / quit: Owned|Entering ──▶ release sequence ──▶ Closed;  Probing|Refused|Yielded ──▶ close only
```

Constants (`session.rs`): `REPLY_TIMEOUT = 500 ms`, `HEARTBEAT = 2 s`, `MISSED_LIMIT = 3`, `TAKEOVER_LIMIT = 3` within `TAKEOVER_WINDOW = 30 s`. All to be checked on hardware (M11 records reply times).

### 9.2 Acquisition (both models; Mini bytes shown, the X uses `0C`)

1. Open the MIDI-interface pair (input first, SysEx delivered). Open failure: "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again." (status Failed; E2's no-MIDI-words rule kept, hence "control port").
2. **Probe** (one burst): `F0 7E 7F 06 01 F7`, `F0 00 20 29 02 0D 00 F7` (layout), `F0 00 20 29 02 0D 0A F7` (feedback), `F0 00 20 29 02 0D 09 F7` (sleep). Replies fill the restore set `{layout, feedback, asleep}` and the identity.
3. **Enter**: `F0 00 20 29 02 0D 0E 01 F7`, then `F0 00 20 29 02 0D 0E F7`. Reply `01`: `ModeState::Confirmed`. Reply `00` or none: `Unconfirmed` and the notice "Launchpad Mini MK3 connected, but Programmer Mode could not be confirmed." LaunchDeck still draws (degrades, never half-owns).
4. **Configure** (ownership policy, deterministic): if the restore set says external feedback off: `F0 00 20 29 02 0D 0A <internal> 01 F7` (internal untouched: it only acts in Custom Modes, MINI-UG p19). If asleep: `F0 00 20 29 02 0D 09 01 F7`. If a brightness preference is set (section 12.1): `F0 00 20 29 02 0D 08 F7` then `F0 00 20 29 02 0D 08 <level> F7` (the device answers the query before it applies the set, so the reply is the level to restore). Then the clear frame; cache all off; open the input gate.
5. **Owned**: render diffs; every 2 s `F0 00 20 29 02 0D 0E F7` (and `… 09 F7` while LaunchDeck believes the unit asleep). Reply `00` = Live fallback (power cycle or another program): close the input gate, count a takeover, go back to Probing (re-reads identity and restore set; a power-cycled unit reports its fresh state). Three takeovers in 30 s: Yielded, "Another program is using the Launchpad Mini MK3. Press Look again to use it here." Three unanswered heartbeats: notice "The Launchpad Mini MK3 stopped answering." (cleared by the next reply); unplug detection stays with the 3 s port scan.

### 9.3 Release (person disconnects, Look-again replacement, app quit)

Stop rendering (the manager holds the lock), then: stop a text scroll LaunchDeck started (`F0 00 20 29 02 0D 07 F7`); the clear frame; restore feedback if changed (`0A <saved int> <saved ext>`); restore brightness if changed (`08 <saved>`); `F0 00 20 29 02 0D 0E 00 F7`; if the saved layout is known, not `7F` and not the profile's Live fallback (Mini `05`, X `01`): `F0 00 20 29 02 0D 00 <saved> F7`; close input and output. Send errors are ignored (the unit may be gone). Never `10h`. A Drop guard runs the same sequence once if nobody did (M8).

### 9.4 App quit and crashes

`lib.rs` switches to `tauri::Builder::…build(generate_context!())?.run(|app, event| …)` and on `RunEvent::Exit` calls `Runtime::release_devices()` (bounded: a few SysEx sends, no waiting). The tray's Quit calls `app.exit(0)`, which emits `RunEvent::Exit` (Tauri 2), so `tray.rs` needs no change. A crash or a kill skips this: the unit stays in Programmer mode, Setup disabled, until LaunchDeck starts again (acquisition then release) or the unit is replugged (powers on in Live). Documented in `docs/devices/launchpad-mini-mk3.md`. The release on quit is NOT VERIFIED in the app window (f).

### 9.5 Device states seen by the person

E2's statuses are kept (no new `DeviceStatus` value: the TypeScript enum would reject it). `connected` means LaunchDeck holds the ports; `details.mode` says `acquiring`, `confirmed` or `unconfirmed`; `notice` carries a connected-with-a-limitation sentence; `problem` (status `failed`) carries refusals (bootloader, other model, taken over, port unavailable).

---

## 10. Reconnect

| Situation | Detected by | What happens |
| --- | --- | --- |
| Unplugged | the 3 s port scan (port gone) or a failed send | release attempted (errors ignored), status `unplugged`, `device.disconnected` once; no crash |
| Plugged in again | the scan | a new `Mk3Device` for the unit (ports may have changed), full acquisition: identity confirmed, Programmer Mode re-entered, brightness preference reapplied, clear + full frame; `device.connected`; no restart |
| Power cycle faster than the scan, or Ableton switching the layout | heartbeat `0Eh` reply `00` | re-acquire with full redraw; takeover guard |
| Unit answers nothing any more but its ports stay | 3 missed heartbeats | notice; the scan decides about unplug |

Recovery time: ≤ 3 s scan + about two render ticks of acquisition with a prompt device (fake transport: measured in M10; hardware: M11).

---

## 11. Capability model

### 11.1 `DeviceCapabilities`: unchanged

Same fields, same meaning (renderer, Zod, artifacts' device fit). The Mini declares:

```rust
DeviceCapabilities { grid: Some(GridCaps { width: 8, height: 8 }), aux: AuxCaps { top: 8, side: 8, logo: true },
    rgb: true, velocity: false, pressure: false, animations: AnimationCaps { pulse: true, flash: true } }
```

The X keeps `velocity: true, pressure: false, animations: {false, false}`.

### 11.2 New generic types (`devices/features.rs`)

```rust
/// What a device can do beyond drawing frames. The renderer never reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceFeatures {
    pub host_mode: bool,          // LaunchDeck switches it into a mode it owns (Programmer Mode) and back
    pub palette: bool,            // lights by index into a fixed colour table (used for native animation)
    pub batched_lighting: bool,   // many LEDs in one message
    pub flash_period_ms: Option<u16>, // native animation period when the device fixes it
    pub pulse_period_ms: Option<u16>,
    pub brightness: bool,
    pub sleep: bool,
    pub scrolling_text: bool,
    pub firmware_version: bool,   // the device reports a firmware version
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModeState { #[default] None, Acquiring, Confirmed, Unconfirmed }

/// What LaunchDeck observed on a connected device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDetails {
    pub firmware: Option<String>,   // "0531": the digits as the bootloader shows them
    pub mode: ModeState,
    pub brightness: Option<u8>,     // percent, when LaunchDeck set or read it
    pub asleep: bool,
    pub scrolling: bool,
    pub notice: Option<String>,     // connected, with a limitation, in a sentence
}

pub enum DeviceSetting { Brightness(Option<u8> /* percent; None = the device's own level */), Sleep(bool) }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextScroll { pub text: String, pub color: Rgb, pub speed: u8, pub direction: ScrollDirection, pub looping: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollDirection { #[default] Left, Right }   // Left: text moves right to left (positive speed)
```

| | Mini | X (hardware) | Virtual X | Virtual Mini |
| --- | --- | --- | --- | --- |
| host_mode | yes | yes | no | no |
| palette | yes | no (unused) | no | yes |
| batched_lighting | yes | yes | no | no |
| flash/pulse period | 500/1000 | none (synthesised) | none (CSS) | 500/1000 |
| brightness, sleep, scrolling_text | yes | yes | no | yes (state recorded) |
| firmware_version | yes | yes | no | no |

### 11.3 Trait additions (default implementations; the virtual X and test fakes compile unchanged)

```rust
fn features(&self) -> DeviceFeatures { DeviceFeatures::default() }
fn details(&self) -> DeviceDetails { DeviceDetails::default() }
fn apply(&mut self, setting: DeviceSetting) -> Result<(), DeviceError> { Err(DeviceError::Unavailable(..)) }
fn scroll_text(&mut self, text: Option<&TextScroll>) -> Result<(), DeviceError> { Err(DeviceError::Unavailable(..)) }
```

`disconnect()` stays the graceful release (section 9.3) and is safe on a dead port.

### 11.4 `DeviceInfo` and errors

`DeviceInfo` gains `notice: Option<String>` (skipped when `None`; from `details().notice` while connected), `details: DeviceDetails` and `features: DeviceFeatures` (always serialised, camelCase). `DeviceChange::Updated(id)` is returned by `DeviceManager::render` when `details()` changed; the runtime then only calls `emit_state()` (no new bus event type, so no observable fixture changes). `DeviceError` gains `Bootloader(String)`, `OtherModel(String)`, `TakenOver(String)` (refusals: the manager stores their sentence as `problem`, status `failed`, retried only on Look again) and `Unavailable(String)` ("This device cannot do that."). `DeviceError::Unsupported` becomes "Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3." when the Mini is enabled (M6).

The Zod schemas parse unknown keys away, so new Rust fields do not break the UI before M13; M13 adds them to `src/lib/schema/runtime.ts`.

---

## 12. Settings and features

### 12.1 Brightness (device preference, Tier 2)

* Preference: `Preferences.device_brightness: Option<u8>` (percent 0-100), `#[serde(default, skip_serializing_if = "Option::is_none")]`; `None` = leave the device's own level. Not part of Surfaces; the default workspace fixture does not change. One value for every device with the feature (multi-device UI is out of scope).
* Mapping: level = `(percent × 127 + 50) / 100` (0 → 0, 50 → 64, 100 → 127); a read level gives percent `(level × 100 + 63) / 127`.
* Applied on acquisition and immediately when changed (query, then set; the device answers the query before it applies the set, so the reply is the level to restore; only the first reply after acquisition is kept, so later changes never overwrite the person's own level); restored on release if LaunchDeck changed it; reapplied after every reconnect; cache invalidated (full redraw) after a change.
* Runtime: `Runtime::set_device_brightness(&self, percent: Option<u8>) -> Result<(), String>` (validates ≤ 100, saves the preference with `persist`, applies it to every connected device with the feature, emits workspace and state). The UI must use this command, never `update_preferences`, for brightness; M13 adds `deviceBrightness` to `preferencesSchema` so that saving other preferences keeps it (Zod strips unknown keys).

### 12.2 Sleep and wake (Tier 2)

`Runtime::set_device_sleep(&self, id: &str, asleep: bool)`: sends `09 00` or `09 01`. No automatic sleep. While asleep LaunchDeck keeps its cache but treats it as unknown; on wake (command, or any press from the unit, or a heartbeat `09` reply `01`) it invalidates the cache and redraws. The first press after LaunchDeck put the unit to sleep only wakes it and is not forwarded with its release (whether the device sends it at all is unknown; the heartbeat corrects the state within 2 s if it does not).

### 12.3 Scrolling text (Tier 2)

`Runtime::scroll_device_text(&self, id: &str, text: Option<TextScroll>)`. Official options only: loop, speed 1-63 pads per second, direction (sign of the speed byte), colour (RGB colourspec `01 r g b`, 7-bit), text. LaunchDeck's limits (not the device's, which are undocumented): 1-64 characters, printable ASCII `0x20-0x7E`; anything else is refused with "Scrolling text takes up to 64 letters, digits and punctuation marks." `None` stops (`07` empty) and invalidates the cache. Since no completion message is documented, a non-looping scroll is "over" after an estimate of `(characters × 6 + 9) / speed` seconds (column width 6 is an assumption), after which the cache is invalidated; covered lighting is retained by the device anyway (p20). No generic text renderer is written (the hardware does it). An Action Library action for text is deferred (Tier 3).

### 12.4 Firmware display and old firmware

`details.firmware` from the inquiry. No version gate (Q15). Evidence-based warning: when a documented readback that others answered gets no reply (brightness `08h`, sleep `09h`), the feature is marked unavailable for that unit and the notice says "Firmware version 0531 does not support brightness. Novation Components can update it." Docs explain Components; LaunchDeck never updates firmware.

### 12.5 LED feedback

Configured and restored by the session (section 9.2). Not a user setting.

### 12.6 Commands (M8) and UI (M13)

Tauri commands `set_device_brightness(percent: Option<u8>)`, `set_device_sleep(id: String, asleep: bool)`, `scroll_device_text(id: String, text: Option<TextScroll>)`, returning the `CmdResult` of the day. UI (M13): Settings › Devices shows "Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (confirmed)", a brightness slider (with "Device's own level"), Sleep/Wake, a capability list in words, a "Scroll a test text" button; notes, CCs and SysEx appear only under the collapsed Technical details.

---

## 13. Virtual-device parity

* `virtual_device.rs` becomes `VirtualDevice` built from a `VirtualModel { id, name, model, capabilities, features, display: fn(Led) -> Led }`. `VirtualLaunchpadX::new()` keeps returning the X model (`VIRTUAL_ID`, name and capabilities unchanged, `display` = identity, features none), so the app's simulator and every test stay as they are.
* `novation::profile::virtual_model(&LAUNCHPAD_MINI_MK3)` gives the virtual Mini: id `virtual-launchpad-mini-mk3`, the Mini's capabilities (no velocity: presses report 127), features as in 11.2, and `display` = the same mapping as the hardware: a pulsing or flashing LED shows `palette_rgb(nearest(color))`, a static LED its exact colour. Same shared function as `leds.rs`, so the virtual Mini cannot drift from the adapter.
* The virtual Mini records brightness (percent), sleep (frame all off while asleep; the first press wakes it and is not forwarded, nor its release: the same rule as the adapter, section 12.2), and the current `TextScroll` (`VirtualHandle::text()`); `features()`/`details()` report them.
* The app keeps the Virtual Launchpad X as its simulator; a virtual Mini in the app (simulator model choice) is deferred (multi-device UI). The UI's CSS timings stay 1.4 s / 0.7 s for the virtual X, which matches the X hardware's synthesised timing.

---

## 14. Testing strategy

### 14.1 Protocol byte tests (`novation/protocol_tests.rs`, from the manual's hex lines, each test names its page)

| Test | Expected bytes |
| --- | --- |
| inquiry request (MINI-PRM p6) | `F0 7E 7F 06 01 F7` |
| Mini application reply parsed (p6; digits illustrative) | `F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 01 F7` → family `[13,01]`, version `[0,5,3,1]` |
| Mini bootloader reply (p6) | `F0 7E 00 06 02 00 20 29 13 11 00 00 00 01 00 02 F7` → bootloader |
| X reply (captured, community) | `F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7` → `[03,01]`, `[0,2,3,8]` |
| malformed replies rejected | 16 bytes; digit `0A`; no `F7`; manufacturer `00 20 2A`; data byte ≥ `80` |
| Programmer / Live / query (p7-8) | `F0 00 20 29 02 0D 0E 01 F7`, `… 0E 00 F7`, `F0 00 20 29 02 0D 0E F7`; reply `F0 00 20 29 02 0D 0E 01 F7` → readback `0E [01]` |
| layout select / query (p7) | `F0 00 20 29 02 0D 00 7F F7`, `F0 00 20 29 02 0D 00 05 F7`, `F0 00 20 29 02 0D 00 F7` |
| LED SysEx example (p14) | static(11, 13), flash(12, B 21, A 23), pulse(13, 37) → `F0 00 20 29 02 0D 03 00 0B 0D 01 0C 15 17 02 0D 25 F7` |
| RGB colourspec | rgb(11, 127, 0, 64) → `F0 00 20 29 02 0D 03 03 0B 7F 00 40 F7` |
| chunking | 82 colourspecs → 2 messages (81 + 1); 81 RGB → one 413-byte message |
| channel lighting (p13) | `90 0B 05`, `91 51 13`, `92 12 2D`, off `90 12 00` |
| text (p20, hex line) | loop, speed 7, palette 37, "Hello World" → `F0 00 20 29 02 0D 07 01 07 00 25 48 65 6C 6C 6F 20 57 6F 72 6C 64 F7`; stop `F0 00 20 29 02 0D 07 F7`; RGB white `01 7F 7F 7F`; right direction speed 7 → `79` |
| brightness (p22) | `F0 00 20 29 02 0D 08 7F F7`, query `F0 00 20 29 02 0D 08 F7` |
| sleep (p21) | sleep `F0 00 20 29 02 0D 09 00 F7`, wake `… 09 01 F7`, query `… 09 F7` |
| feedback (p22) | `F0 00 20 29 02 0D 0A 00 01 F7`, query `… 0A F7` |
| X product byte | every command above with `0C` equals the existing X tests |
| layout map (p10, p13) | lower left 11, upper left 81, lower right 18; top 91-98; side 89…19; logo 99; 81 distinct indices |
| input (p6, p10) | `90 0B 7F` grid(0,7) press; `90 0B 00` and `80 0B 00` release; `B0 5B 7F` top 0 press; `B0 13 00` side 7 release; `B0 63 7F`, `A0 0B 40`, `D0 40`, `F8`, `90 05 7F` ignored; Mini `90 0B 05` → velocity 127, X → 5 |

### 14.2 Transport tests (`novation/device_tests.rs`, real `Mk3Device` over `FakeMidi` + `SimLaunchpad`)

`SimLaunchpad` is written from the manual, independently of `protocol.rs` (it parses bytes itself, so the tests are not tautological): power-on Live (Mini layout `05`, X `01`), feedback `(1,1)`, awake, brightness 127, configurable family/firmware/product, bootloader, silent, answers per command; applies `03h` types 0-3 and channel 1-3 notes in Programmer mode (external feedback off: notes ignored); text, sleep; `press(pos)` / `release(pos)` produce Programmer bytes; `power_cycle()`; `take_over()` (selects layout `00` like Ableton). Scenarios: discovery per fixture → unit; acquisition transcript (section 9.2, exact bytes); bootloader refusal sends nothing after the probe; other family refused; silent unit: notice, still draws; restore set and release transcript; feedback off restored; asleep woken; heartbeat Live fallback → re-acquire + full redraw; takeover ×3 → yielded; all 64 + 8 + 8 press/release in order; logo never input; full frame in one message; no redundant sends; fragmented SysEx and two messages per callback; malformed and unknown messages ignored; unplug (sends fail or ports vanish) → no panic, release attempted; replug → restored. The X golden transcript (from M1) lives here from M4 on.

### 14.3 OS port fixtures (`novation/ports_tests.rs`; labelled "derived from docs/community, not observed")

| Id | System | Names (inputs / outputs) | Expected |
| --- | --- | --- | --- |
| M1 | macOS | `Launchpad Mini MK3 LPMiniMK3 DAW Out`, `… MIDI Out` / `… DAW In`, `… MIDI In` | one Mini, pair (`…MIDI Out`, `…MIDI In`), id `launchpad-mini-mk3:1` |
| M2 | macOS | `Launchpad Mini MK3 LPMiniMK3 DAW`, `… MIDI` both directions | one Mini on `… MIDI` |
| M3 | macOS | M1 twice (distinct ids) | two Minis, k-th with k-th |
| M4 | macOS | `Launchpad X LPX DAW Out`, `Launchpad X LPX MIDI Out` / `… DAW In`, `… MIDI In` | one X |
| M5 | macOS | `IAC Driver Bus 1`, `Launchpad Pro MK3 LPProMK3 MIDI`, `… DAW`, `… DIN` | no unit; Pro MK3 listed once |
| W1 | Windows | `LPMiniMK3 MIDI`, `MIDIIN2 (LPMiniMK3 MIDI)` / `LPMiniMK3 MIDI`, `MIDIOUT2 (LPMiniMK3 MIDI)` | one Mini on `MIDIIN2`/`MIDIOUT2`; `LPMiniMK3 MIDI` = DAW |
| W2 | Windows | W1 + `2- LPMiniMK3 MIDI`, `MIDIIN2 (2- LPMiniMK3 MIDI)` / `2- …`, `MIDIOUT2 (2- …)` | two Minis |
| W3 | Windows | W1 twice, per-unit ids | two Minis split by id |
| W4 | Windows | `LPX MIDI`, `MIDIIN2 (LPX MIDI)` / `LPX MIDI`, `MIDIOUT2 (LPX MIDI)` | one X on `MIDIIN2` (D-X1) |
| W5 | Windows | output `Microsoft GS Wavetable Synth` | nothing |
| N1 | Windows new style | `LPMiniMK3 DAW Out`, `LPMiniMK3 MIDI Out` / `LPMiniMK3 DAW In`, `LPMiniMK3 MIDI In` | one Mini |
| N2 | Windows new style | N1 with ` (2)` appended | second Mini |
| N3 | Windows new style | `Launchpad X LPX MIDI Out` / `… In`; `Launchpad X (2) LPX MIDI Out` / `… In` | two X |
| L1 | Linux ≥ 5.13 | `Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 DA 20:0`, `… MI 20:1` (both lists, ids `20:0`, `20:1`) | one Mini on `20:1` |
| L2 | Linux < 5.13 | `Launchpad Mini MK3:Launchpad Mini MK3 MIDI 1 20:0`, `… MIDI 2 20:1` | one Mini on `MIDI 2` |
| L3 | Linux | L1 for clients 20 and 24 | two Minis |
| L4 | Linux ≥ 5.13 | `Launchpad X:Launchpad X LPX DAW In 28:0`, `… LPX MIDI In 28:1` | one X on `28:1` |
| L5 | Linux < 5.13 | `Launchpad X:Launchpad X MIDI 1 28:0`, `… MIDI 2 28:1` | one X on `MIDI 2` (D-X2) |
| L6 | Linux | `Midi Through:Midi Through Port-0 14:0`, `Launchpad XL:Launchpad XL MIDI 1 24:0` | no unit; XL listed |

### 14.4 Runtime E2E (`core/runtime/device_tests.rs`, in-crate, `#[cfg(test)]`)

Real `Runtime` (`scan_hardware: false`), real workspace with a demo-agent pad (press `demo.startTask`, feedback mirroring the agent; running = pulse), a navigation pad and a second surface reached by a Context Engine rule (`ScriptedObserver`), real renderer, actions and feedback; hardware source swapped for `NovationHardware` over `FakeMidi` + `SimLaunchpad` through a `#[cfg(test)]` hook (no `RuntimeConfig` field: about 18 struct-literal sites). Steps: Look again → acquired (sim in Programmer, LEDs equal the frame) → press → `control.pressed` with `deviceId: launchpad-mini-mk3:1` → demo agent running → sim LED pulsing palette → foreground change → other surface on the sim → unplug → `device.disconnected`, status unplugged → power-cycled sim replugged → re-acquired, LEDs restored → `release_devices()` → sim in Live, layout restored. The same for the X (RGB only, no palette bytes) and the virtual Mini (palette-quantised pulse in the frame).

### 14.5 Regression gates

Existing suites stay green in every package: `cargo.sh test --no-fail-fast` (Action Library: `action_library`, `catalog_contract`; Feedback: `state_feedback`, `feedback_editor`, `state_hub`, `state_characterization`; Agent Control: `agent_control_flows`, `agent_control_golden`, `agent_bridge`, `agents_contract`; Context Engine: `context_engine`, `context_contract`; Plugin SDK: `plugin_sdk`, `plugin_states`, `plugins_contract`; plus the lib tests), Vitest, typecheck, lint, the architecture check.

### 14.6 Measurements

`novation/measure_tests.rs` (`#[ignore = "a measurement: run it by name with --ignored --nocapture"]`, the precedent of `core/runtime/tests.rs:644`): LED strategies (section 8.4), connect-to-Owned in ticks and fake time, full render encode+send, incremental diff, decode-to-`DeviceInput` latency; the E2E records press-to-`control.pressed` and unplug-to-restored. Host-side only; device latency is M11's.

---

## 15. Hardware acceptance strategy

### 15.1 The suite

`devices/novation/hardware_tests.rs`, one test `launchpad_acceptance` marked `#[ignore = "needs a real Launchpad; run scripts/launchpad-acceptance.sh"]`, run against whichever supported Launchpad it finds (a Mini MK3 first, else an X), using the real `SystemMidi` backend, `NovationHardware` with `Platform::current()` and `Mk3Device`. `scripts/launchpad-acceptance.sh` runs `cargo test --lib launchpad_acceptance -- --ignored --nocapture --test-threads=1` from `src-tauri/` (honours `LAUNCHDECK_CARGO`) and writes the report to `$LAUNCHDECK_ACCEPTANCE_REPORT` (default: a file in the system temp folder, path printed). The owner answers on the device itself: **press the top right `>` button (Side 0) for yes, the bottom right button (Side 7, Stop/Solo/Mute) for no**; 60 s per step, then "skipped". Without a Launchpad it prints "No Launchpad Mini MK3 or Launchpad X found: NOT HARDWARE VERIFIED" and passes.

### 15.2 Steps (each prints raw bytes, the result and the open question it answers)

1. Detection: every port name and id per direction, in OS order; the parse; the chosen pair (Q1, Q16).
2. Inquiry on the MIDI interface: bytes, family, firmware digits, reply time (Q3, Q15). Optional (`LAUNCHDECK_ACCEPTANCE_DAW=1`): inquiry on the DAW interface (does it answer, which bytes).
3. Automatic Programmer Mode: the acquisition transcript; `0Eh` readback `01`; layout readback in Programmer mode (Q4).
4. Inputs: the tool lights every control dim; each press lights it green and records the raw bytes (channel, velocity/value) and the release; all 64 + 8 + 8 must be pressed (Q5, Q6).
5. Full palette: indices 0-63, then 64-127 on the grid; "do the colours match the manual's chart?" (Q8; validates section 8.3).
6. RGB: gradients per channel; exact white, red, green, blue.
7. Native flash and pulse: one row each; period estimate by the owner ("about twice a second?", "about once a second?"); flash → static transition (does the flash stop?) (Q8, section 8.5 open question).
8. Full frame and batching: device processing time with the readback barrier for 1, 8, 64, 81 LEDs as one SysEx, per-LED SysEx and per-LED notes; recommendation printed (section 8.4).
9. Brightness: 0, 64, 127 with readback; "visibly different?"; how many distinct levels (Q10).
10. Scrolling text: "Hello World" looping, speed 7; left to right; RGB colour; stop; a non-looping text: any SysEx after it ends? a 64- and 200-character text (Q11).
11. Sleep and wake: `09 00`; readback; press to wake: was the press sent as MIDI? were the LEDs restored? (Q12)
12. LED feedback: external off, light by SysEx and by note: which show? internal off/on in Programmer mode (Q13).
13. Disconnect and reconnect: "unplug the unit", then "plug it in": time to Owned, `0Eh` readback `00` before re-entry (powers on in Live), frame restored (Q14).
14. Safe release: release sequence; "is the unit back in its normal mode, and does holding Session open Setup?"; the layout restored (Q14).
15. Summary: per step pass / fail / skipped, firmware, OS, kernel (Linux), Windows naming style, and the measured numbers.

### 15.3 Status until the owner runs it

Every physical behaviour is reported NOT HARDWARE VERIFIED (no Novation device attached, 2026-10-05). The next hardware milestone: run the suite on macOS with a Mini MK3 (and an X if available), commit the report under `docs/verification/`, then settle the open questions of section 1 (press values, flash → static, sleep retention, batching).

---

## 16. Tier 1/2/3 scope, documentation and work packages

### 16.1 Scope

| Tier | Item | Package |
| --- | --- | --- |
| 1 | robust discovery, per-system port pairs, ids | M3 |
| 1 | identification by inquiry, firmware read, bootloader refusal | M5 |
| 1 | Programmer Mode entry with readback; safe release with restore; release on quit | M5, M8 |
| 1 | 64 + 8 + 8 inputs, press/release, logo LED output | M4, M6 |
| 1 | palette, RGB, native pulse and flash, batched full frame, diff cache | M4, M6 |
| 1 | reconnect and state restoration; heartbeat | M5 |
| 1 | capability reporting; virtual parity; protocol, transport, OS-fixture and E2E tests | M6, M9, M10 |
| 2 | brightness preference, sleep/wake, scrolling text, firmware display, notices | M7, M8, M13 |
| 2 | user-facing device settings | M13 |
| 3, prepared | multiple Launchpads (ids, pairing, probe; no UI) | M3 (ids only) |
| 3, prepared | DAW-port coexistence: never open the DAW port; takeover guard yields to a DAW | M5 |
| 3, research only | DAW-mode faders, Custom Mode coexistence (Lighting Custom Modes instead of Programmer mode), musical coexistence (MIDI clock), a text action in the Action Library, a virtual Mini in the app, native animation on the X | deferred (`PRODUCTIZATION_PLAN.md` section 4 entry, M12) |

Out of scope (brief): firmware flashing, bootloader management, factory reset, Components replacement, Custom Mode editor, Ableton Session, sequencer, DAW integration, device-ID changes, other Launchpad models, multi-device UI.

Rule zero: this is device-model support behind the existing `SurfaceDevice` trait requested by the owner, not a new product subsystem; M12 records it in `PRODUCTIZATION_PLAN.md`.

### 16.2 Documentation to write

| File | Content | Package |
| --- | --- | --- |
| `docs/devices/launchpad-mini-mk3.md` | what LaunchDeck does with the Mini: ports per system, lifecycle, Programmer Mode, controls, lighting, features, limitations (crash, identical units, DAW coexistence), firmware and Components, verification status | M12 |
| `docs/devices/novation-mk3-protocol.md` | the shared layer: what is shared and why, the commands used with page references, the copy errors, model profiles; references the manuals instead of copying them | M12 |
| `docs/devices/capabilities.md` | `DeviceCapabilities`, `DeviceFeatures`, `DeviceDetails`, statuses and notices; how to add a device model | M12 |
| `docs/devices/hardware-testing.md` | the acceptance suite, its report, the open questions, the measurements and the batching rule | M10, M11, M12 |
| `docs/devices/launchpad-x.md` | updated: module paths, official aux/logo numbers (still not hardware-tested), fixed defects | M1-M3 (paths), M12 |
| maps and rules | CODEBASE_MAP, ENGINEERING_RULES R5, overview, AGENTS.md, `PRODUCTIZATION_PLAN.md` | M1, M2-M4 (rows), M12 |

Package R3 of `PRODUCTIZATION_PLAN.md` will later move `docs/devices/*` into `docs/user/` and `docs/architecture/`; these files are written so they can move unchanged.

### 16.3 Lanes and order

The packages run **sequentially** in a device-lane worktree the orchestrator creates from `productization-ux` after E2 (`0c944ea` or later). In parallel the UX lane does E3, E4, E5 in its worktree and the artifacts lane works in the main tree. Device packages M1-M12 stay out of `src/features/settings/**` and `src/features/onboarding/**` and change nothing under `src/` at all; M13 (all UI) runs after the lanes are merged.

Expected merge points for the orchestrator: `src-tauri/src/lib.rs` (M8: `generate_handler!` additions and the `RunEvent` handler; the UX lane also adds commands), `src-tauri/src/core/model.rs` (M7: one `Preferences` field; the artifacts lane edits other parts), `src-tauri/src/core/runtime/{devices.rs, mod.rs, startup.rs}` (M7, M10), `docs/architecture/CODEBASE_MAP.md`, `PRODUCTIZATION_PLAN.md`, `docs/devices/launchpad-x.md`.

### 16.4 Work packages

**Rules for every package.**

* Work only in the device-lane worktree the orchestrator gives you (below `$WT`). `cd "$WT"` first; `git status` before and after.
* Cargo: `C=/private/tmp/claude-501/-Users-lr0y-Movies-launchdeck/de5fb0f6-fd95-4c41-bf94-4c11fa0f3ba8/scratchpad/cargo.sh`, run from `$WT/src-tauri`. Never plain `cargo`, never `cargo clean`.
* Gate before every Rust commit: `$C fmt --check`, `$C clippy --all-targets -- -D warnings`, the package's test filters; at package end `$C test --no-fail-fast` and, from `$WT`, `node scripts/check-architecture.mjs` and `LAUNCHDECK_CARGO=$C npm run check`.
* Commits: conventional `<type>: <description>` with a body (what and why), no attribution lines, only your paths: `git add <paths> && git commit -m "…" -- <paths>`; wait and retry if `.git/index.lock` exists. `git show --stat HEAD` after each commit.
* Tests first where behaviour changes; structural commits separate from behaviour commits (R18); every map or rule change in the same commit as the ownership change (R21).
* Launchpad X behaviour stays byte-identical: the golden transcript (M1) is changed only by the commits named for defects D-X1 … D-X11, each together with a test of the fix.
* No desktop driving (no `open`, no app launch, no `tauri dev`, no screenshots), no hardware assumptions: anything physical is NOT HARDWARE VERIFIED in commit bodies and docs.
* Do not touch `src/features/settings/**`, `src/features/onboarding/**` (M1-M12: nothing under `src/`), `examples/*.json` fixtures, `DeviceCapabilities` fields, the `DeviceStatus` values, `VIRTUAL_ID`, `RuntimeConfig`.
* Read the sections of this plan each package names: `/Users/lr0y/Movies/launchdeck/LAUNCHPAD_MINI_MK3_PLAN.md`.

#### M1. MIDI transport seam; `midir` only in `devices/midi/system.rs` (Rust, M)

Goal: put all MIDI I/O behind `MidiBackend`/`MidiLink` so adapters can be tested against an in-memory transport; move `midir` to one transport file; pin the X bytes with a golden transcript. Read sections 2, 4.3, 4.4.

Files: new `src-tauri/src/devices/midi/{mod.rs, system.rs, testkit.rs}`; modified `src-tauri/src/devices/{mod.rs, launchpad_x.rs, manager.rs}`, `scripts/architecture/rust.mjs`, `scripts/check-architecture.test.mjs`, `docs/architecture/{CODEBASE_MAP.md, ENGINEERING_RULES.md, overview.md}`, `AGENTS.md`, `PRODUCTIZATION_PLAN.md` (line 196 only), `src-tauri/src/lib.rs` (module doc line 9 only), `docs/devices/launchpad-x.md` (file paths only).

Decisions: the traits and types of section 4.4 exactly (`pub(crate)`); `SystemMidi` lists ports with `port_name()` and `id()` (fresh clients per call, as today), opens input first with `Ignore::TimeAndActiveSense`, resolves ports by name and id, closes the input if the output fails. `FakeMidi` (in `testkit.rs`, `#[cfg(test)]`) records `(output id, bytes)` per send, injects bytes into an open input, can mark opens busy and remove ports. E2's `PortNames`/`PortSource` stay for now (`SystemPorts::list` uses the backend's names); `LaunchpadX` holds `Option<Box<dyn MidiLink>>` and an `Arc<AtomicBool>` input gate, opens through the backend (both or neither, fixes D-X11), sends `0E 01`, opens the gate, then `clear()`; disconnect unchanged (clear, `0E 00`) then closes the link. Output bytes stay identical; SysEx now reaches the callback and is ignored by `decode_message` (3-byte messages only).

Tests first (`launchpad_x.rs` tests or `devices/launchpad_x_tests.rs`), the **X golden transcript** over `FakeMidi` with macOS names `Launchpad X LPX MIDI Out`/`In`:
* connect → exactly two messages: `F0 00 20 29 02 0C 0E 01 F7`, then one 413-byte clear `F0 00 20 29 02 0C 03` + 81 × `03 <idx> 00 00 00` + `F7`, indices in `capabilities().positions()` order (81..88, 71..78, …, 11..18, 91..98, 89, 79, …, 19, 99);
* render a frame with `grid(0,0)` = `Rgb(255,0,128)` → `F0 00 20 29 02 0C 03 03 51 7F 00 40 F7`; the same frame again → nothing;
* inject `90 0B 64` → `DeviceInput { grid(0,7), Pressed{100} }`; inject `F0 00 20 29 02 0C 0E 01 F7` → nothing;
* disconnect → the 413-byte clear then `F0 00 20 29 02 0C 0E 00 F7`, link closed, `is_connected() == false`;
* open refused (busy) → `Err`, nothing sent, not connected.

Validation: `$C test --lib devices::`; `node scripts/check-architecture.mjs`; `npx vitest run scripts/check-architecture.test.mjs` (or `npm test`); full gate.

Commits: (1) `refactor: put MIDI I/O behind a transport seam in devices/midi` (seam, system backend, fake, X through it, golden transcript, rule and map changes together; body: "bytes sent are identical; the input now opens before the first byte is sent and receives SysEx, which the X ignores; an input failure no longer leaves a half-open adapter (D-X11)").

Do not: change any byte the X sends; add a dependency; keep any `midir` use outside `devices/midi/system.rs`; touch TypeScript.

#### M2. Shared Novation MK3 codec (Rust, M)

Goal: a pure, byte-tested codec for both models in `devices/novation/`, with the X module moved there and building its messages through it. Read sections 1, 4, 7, 14.1.

Files: `git mv src-tauri/src/devices/launchpad_x.rs src-tauri/src/devices/novation/launchpad_x.rs` (and its tests file if M1 made one); new `src-tauri/src/devices/novation/{mod.rs, protocol.rs, layout.rs, protocol_tests.rs}`; modified `src-tauri/src/devices/{mod.rs, manager.rs}` (paths), `docs/architecture/{CODEBASE_MAP.md, overview.md}` (devices row and table), `docs/devices/launchpad-x.md` (path).

Decisions: `protocol.rs` API: `Product(u8)` with `MINI_MK3 = Product(0x0D)`, `X = Product(0x0C)`; `sysex(product, command, data)`, `query(product, command)`, `programmer_mode(product, on)`, `select_layout(product, layout)`, `brightness(product, level)`, `sleep(product, leds_on)` (0 = sleep), `feedback(product, internal, external)`, `enum Colourspec { Static{index, palette}, Flash{index, b, a}, Pulse{index, palette}, Rgb{index, r, g, b} }` (7-bit values), `lighting(product, &[Colourspec]) -> Vec<Vec<u8>>` (chunks of 81), `channel_lighting(kind, index, palette) -> [u8; 3]` (notes on channel 1/2/3), `text(product, looping, speed: i8, colour: TextColour, ascii: &[u8])`, `text_stop(product)`, `INQUIRY`, `enum Reply { Identity { family: [u8;2], member: [u8;2], version: [u8;4] }, Readback { product: u8, command: u8, data: Vec<u8> } }`, `parse_reply(&[u8]) -> Option<Reply>` (identity: exactly 17 bytes, `F0 7E <any> 06 02 00 20 29`, digits 0-9; readback: Novation header, at least one data byte, all data < `80h`), `to_7bit`. `layout.rs`: `led_index`, `pos_from_note`, `pos_from_cc`, `decode(bytes, velocity: bool) -> Option<(ControlPos, InputKind)>` (velocity false → 127). The X keeps thin wrappers (`enter_programmer_mode()`, `encode_rgb_updates`, …) so its existing tests stay unchanged in this package.

Tests first: every row of section 14.1 (Mini `0D`; X spot checks with `0C`), each test naming its manual page; the p14 example and the p20 hex line verbatim.

Validation: `$C test --lib devices::`; full gate.

Commits: (1) `refactor: move the Launchpad X module under devices/novation` (pure move + paths, tests unchanged); (2) `feat: add the shared Novation MK3 codec with the Mini MK3 bytes from its manual`; (3) `refactor: build the Launchpad X messages with the shared codec` (wrappers delegate; X tests and golden transcript unchanged).

Do not: copy the manual's decimal text line or its `13 01` X family into tests; add X-only commands or DAW-port commands; change X bytes.

#### M3. Port parsing, unit pairing, model profiles, generic hardware source (Rust, L)

Goal: deterministic per-system port selection and stable ids for every MK3 model; the manager free of model knowledge. Read sections 5, 6, 14.3.

Files: new `src-tauri/src/devices/source.rs`, `src-tauri/src/devices/novation/{profile.rs, launchpad_mini_mk3.rs, ports.rs, ports_tests.rs, hardware.rs}`; modified `src-tauri/src/devices/{mod.rs, manager.rs}`, `src-tauri/src/devices/novation/{mod.rs, launchpad_x.rs}`, `src-tauri/src/devices/midi/testkit.rs`, `docs/architecture/CODEBASE_MAP.md`, `docs/devices/launchpad-x.md` (ports section).

Decisions: `ModelProfile { model, name, product: Product, families: &[[u8;2]], stem_tokens, name_tokens, interfaces: &[Role], live_layout: u8, capabilities, features }` (features added in M7; until then omit the field). X: `launchpad-x`, "Launchpad X", `0x0C`, families `[[0x03,0x01],[0x13,0x01]]` (comment: X-PRM p6 misprint, Ableton and a captured reply give `03 01`), `["lpx"]`, `[["launchpad","x"]]`, `[Daw, Midi]`, `0x01`, today's capabilities. Mini: `launchpad-mini-mk3`, "Launchpad Mini MK3", `0x0D`, `[[0x13,0x01]]`, `["lpminimk3"]`, `[["launchpad","mini","mk3"]]`, `[Daw, Midi]`, `0x05`, capabilities of section 11.1. Supported profiles in this package: X only; Mini units are recognised and listed as unsupported until M6. `source.rs`: `trait HardwareSource: Send { fn scan(&mut self) -> HardwareScan; fn device(&self, id: &str) -> Option<Box<dyn SurfaceDevice>>; }`, `HardwareScan { units: Vec<Found { id, name }>, unsupported: Vec<DeviceIdentity> }`. `NovationHardware { backend: Arc<dyn MidiBackend>, platform: Platform, last: Vec<FoundUnit> }`. `DeviceManager::new(input)` uses `NovationHardware` over `SystemMidi` with `Platform::current()`; `with_hardware(input, Box<dyn HardwareSource>)` replaces `with_ports`. The parsing algorithm is section 6, verbatim.

Tests first: every fixture of section 14.3 in `ports_tests.rs` (parse result: model, role, pair, unit key, id). E2's manager tests migrate to a small `FakeHardware` in the manager tests (same assertions); one test runs `NovationHardware` over `FakeMidi`.

Commits: (1) `feat: parse Launchpad port names per system and pair each unit's ports` (pure, unused); (2) `refactor: discover hardware through a generic source in the device manager` (`NovationHardware` wraps E2's existing `discover()`; behaviour unchanged; E2 tests pass); (3) `fix: open the Launchpad X's MIDI interface, not its DAW interface, on Windows and older Linux` (swap E2's classifier for the parser; fixtures W4 and L5; unsupported Launchpads once per unit, D-X1, D-X2, D-X4; remove `classify_port`, `discover`, `is_other_launchpad`); (4) `fix: give each Launchpad a stable id that survives a replug` (`launchpad-x:1`, names "(2)", D-X3; tests: two units, replug with a new ALSA client keeps `:1`; update E2 test ids).

Validation: `$C test --lib devices::`; full gate. If the package passes ~1200 changed lines, split commits (3)-(4) into M3b.

Do not: use `cfg!(target_os)` under `devices/**` (take `Platform`); persist any port index; open DAW ports; enable the Mini.

#### M4. Generic MK3 device; the X moved onto it with identical bytes (Rust, L)

Goal: one `Mk3Device` for every profile, with the LED planner, input framing and a minimal session; the X adapter struct removed. Read sections 7, 8.1, 8.2, 8.5, 9.1, 14.2.

Files: new `src-tauri/src/devices/midi/framing.rs`, `src-tauri/src/devices/novation/{session.rs, leds.rs, device.rs, testkit.rs, device_tests.rs}`; modified `src-tauri/src/devices/novation/{mod.rs, hardware.rs, launchpad_x.rs (profile data only)}`, `src-tauri/src/devices/midi/{mod.rs, testkit.rs}` (bind an output to a responder), `docs/architecture/CODEBASE_MAP.md`.

Decisions: `Mk3Device { profile: &'static ModelProfile, found, backend, link: Option<Box<dyn MidiLink>>, session: Session, leds: LedCache, replies: Arc<Mutex<VecDeque<Reply>>>, gate: Arc<AtomicBool>, clock }`. The input callback frames bytes (`MessageFramer`), sends SysEx replies to the queue and, while the gate is open, decodes through `layout::decode` (velocity per `profile.capabilities.velocity`) to the `InputSender`. Session in this package: connect → `0E 01` → clear → Owned; disconnect → clear → `0E 00` → close (exactly today's X behaviour). Planner: frame order, RGB `to_7bit`, one `03h` batch per tick chunked at 81, cache updated after all messages were sent, clear = 81 × RGB off in position order. `SimLaunchpad` per section 14.2 (written from the manual, not from `protocol.rs`).

Tests first: the golden transcript (unchanged) against `Mk3Device`; planner (no redundant sends, chunking, order); framing (two messages in one callback, SysEx split over three callbacks, stray data bytes, `F8` inside, running status ignored); input (`A0`, `D0`, CC 99, note 5 ignored); the sim's LED state equals the frame after a render.

Commits: (1) `test: add a byte-level Launchpad simulator for device tests`; (2) `feat: frame incoming MIDI bytes into whole messages`; (3) `refactor: drive the Launchpad X through a generic MK3 device` (golden transcript unchanged).

Validation: `$C test --lib devices::`; full gate.

Do not: change X bytes; add acquisition steps (M5); use the palette (M6).

#### M5. Ownership lifecycle: identity, confirmed Programmer Mode, restore, heartbeat (Rust, L)

Goal: section 9 for every profile, each X-visible change its own documented fix. Read sections 5, 9, 10, 11.4, 14.2.

Files: `src-tauri/src/devices/{mod.rs, features.rs (new: DeviceDetails, ModeState only), manager.rs}`, `src-tauri/src/devices/novation/{session.rs, device.rs, device_tests.rs, testkit.rs}`, `src-tauri/src/core/runtime/devices.rs` (handle `DeviceChange::Updated`: `emit_state()` only), `docs/devices/launchpad-x.md` (behaviour notes).

Tests first (exact bytes; Mini sim for new behaviour, X sim for the golden transcript), with a fake clock:
* probe burst `F0 7E 7F 06 01 F7`, `F0 00 20 29 02 0D 00 F7`, `F0 00 20 29 02 0D 0A F7`, `F0 00 20 29 02 0D 09 F7`; replies `F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 01 F7`, `… 00 05 F7`, `… 0A 01 01 F7`, `… 09 01 F7`; next tick `F0 00 20 29 02 0D 0E 01 F7`, `F0 00 20 29 02 0D 0E F7`; reply `… 0E 01 F7`; next tick the clear and the frame; `details()` = firmware `"0531"`, mode confirmed;
* bootloader reply `… 13 11 …` → nothing more sent, `render` → `Err(Bootloader)`, `disconnect` sends nothing, manager status failed with the sentence;
* family `03 01` on Mini ports → `OtherModel`; X sim answering `13 01` → accepted;
* silent sim → after 500 ms `0E 01` + query, after another 500 ms notice "Launchpad Mini MK3 connected, but Programmer Mode could not be confirmed.", clear and frame still sent, input accepted;
* input before Owned (sim in Keys sends `90 30 7F`) → not forwarded;
* saved layout `04` → release `…0E 00 F7` then `F0 00 20 29 02 0D 00 04 F7`; saved `05` (Mini) / `01` (X) → no layout select;
* feedback read `01 00` → after confirm `F0 00 20 29 02 0D 0A 01 01 F7`, release `… 0A 01 00 F7` before `0E 00`; asleep read `00` → `F0 00 20 29 02 0D 09 01 F7`;
* heartbeat every 2 s `F0 00 20 29 02 0D 0E F7`; sim power cycle → reply `00` → probe again, clear, full frame; three takeovers in 30 s → `Err(TakenOver)`, nothing sent; three unanswered heartbeats → notice, cleared by the next reply.

Commits: (0) `feat: show device details and connected-with-a-limitation notices` (types, trait `details()`, `DeviceInfo.notice/details`, `DeviceChange::Updated`, refusal variants and `problem`, `CONNECT_PROBLEM` → "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again."; no X byte change); (1) `fix: identify a Launchpad before taking it` (D-X5: inquiry, refusals, firmware, input gate); (2) `fix: confirm Programmer Mode by reading it back` (D-X6); (3) `fix: give the person their layout back on release` (D-X7); (4) `fix: keep LED feedback on and wake a sleeping Launchpad while LaunchDeck draws` (D-X8); (5) `fix: notice a power cycle or a takeover and take the Launchpad back` (D-X9). Each updates the golden transcript with its own change only.

Validation: `$C test --lib devices::`, `$C test --lib core::runtime::`; full gate. If the package passes ~1200 changed lines, commits (3)-(5) become M5b, run right after.

Do not: wait for replies inside `connect`, `render` or under the runtime lock; send `10h`; add a `DeviceStatus` value; enable the Mini.

#### M6. Launchpad Mini MK3 support with native lighting (Rust, M)

Goal: the Mini works end to end on the shared device with palette pulse and flash. Read sections 8, 11.1, 14.

Files: new `src-tauri/src/devices/novation/palette.rs`; modified `src-tauri/src/devices/novation/{leds.rs, launchpad_mini_mk3.rs, hardware.rs (supported list), device_tests.rs, testkit.rs}`, `src-tauri/src/devices/mod.rs` (`Unsupported` sentence), `src-tauri/src/devices/manager.rs` (test sentences), `docs/architecture/CODEBASE_MAP.md`.

Tests first: palette (`nearest` of each table colour = its first index: `(255,0,0)` → 5, `(0,0,255)` → 45, `(0,228,255)` → 37, `(0,255,0)` → 21, `OFF` → 0, a near-black colour is not 0 unless OFF); planner with native caps: `Pulse` `Rgb(0,0,255)` at `grid(0,0)` → colourspec `02 51 2D`, `Flash` `Rgb(255,0,0)` at `grid(0,7)` → `01 0B 05 00`, static `Rgb(255,0,128)` → `03 51 7F 00 40`, mixed in one `03h` message; X frames (no native caps) unchanged; Mini transport: M1/W1/L1 fixtures → one unit; full acquisition transcript with `0D`; all 80 controls pressed and released in order (`90 <note> 7F`, `90 <note> 00`, `B0 <cc> 7F`, `B0 <cc> 00`) → 160 `DeviceInput`s with velocity 127; full frame of 81 LEDs in one message; unplug/replug restores; malformed messages ignored.

Commits: (1) `feat: approximate the Launchpad palette from the manuals' swatches` (table of section 8.3 with its derivation in the doc comment); (2) `feat: light native pulse and flash on devices that declare them`; (3) `feat: support the Launchpad Mini MK3` (profile enabled, capabilities, sentence, transport tests).

Validation: `$C test --lib devices::`; full gate.

Do not: change X capabilities; use note lighting (`NOTE_LIGHTING_MAX_CHANGES` stays 0); claim the palette is measured.

#### M7. Brightness, sleep and scrolling text as generic features (Rust, L)

Goal: section 12.1-12.4 in Rust. Read sections 11.2, 11.3, 12.

Files: `src-tauri/src/devices/{features.rs, mod.rs, manager.rs}`, `src-tauri/src/devices/novation/{session.rs, device.rs, leds.rs, profile.rs, launchpad_x.rs, launchpad_mini_mk3.rs, device_tests.rs, testkit.rs}`, `src-tauri/src/core/model.rs` (`Preferences.device_brightness` only), `src-tauri/src/core/runtime/{devices.rs, startup.rs}` (pass the preference to the manager at start).

Tests first: `DeviceInfo.features` per section 11.2's table; brightness 50 % → `F0 00 20 29 02 0D 08 F7` then `F0 00 20 29 02 0D 08 40 F7`, sim reply `… 08 7F F7` → release sends `F0 00 20 29 02 0D 08 7F F7` before `0E 00`; `None` → restore and no further brightness bytes; reconnect reapplies; next render is a full 81-LED frame; sleep `F0 00 20 29 02 0D 09 00 F7`, wake `… 09 01 F7` + full frame; a press while asleep wakes and is not forwarded (nor its release); text `TextScroll { "Hello World", Rgb(255,255,255), 7, Left, true }` → `F0 00 20 29 02 0D 07 01 07 01 7F 7F 7F 48 65 6C 6C 6F 20 57 6F 72 6C 64 F7`, `Right` → speed byte `79`, stop `… 07 F7` + full frame, refusal of 65 characters and of `é`; release stops a running scroll; the default workspace fixture test still passes (field skipped when `None`); runtime methods validate and persist.

Commits: (1) `feat: list what each device can do beyond drawing` (types, trait defaults, profiles' features, `DeviceInfo.features`); (2) `feat: set Launchpad brightness as a device preference`; (3) `feat: put a Launchpad to sleep and wake it`; (4) `feat: scroll text on a Launchpad`. Each commit body: X gains the feature; no X byte changes unless the person uses it.

Validation: `$C test --lib devices::`, `$C test --lib core::`; full gate (incl. the default-workspace fixture test). If the package passes ~1200 changed lines, commits (3)-(4) become M7b, run right after.

Do not: add commands or touch `lib.rs` (M8); add a `Workspace` field; regenerate fixtures; auto-sleep.

#### M8. Release on quit; device commands (Rust, S)

Goal: graceful exit always runs the release sequence (D-X10); Tauri commands for the features. Read sections 9.3, 9.4, 12.6.

Files: `src-tauri/src/lib.rs`, `src-tauri/src/commands/devices.rs`, `src-tauri/src/core/runtime/devices.rs` (`release_devices`), `src-tauri/src/devices/manager.rs` (`release_all`), `src-tauri/src/devices/novation/device.rs` (Drop guard, released flag), `docs/architecture/CODEBASE_MAP.md` (commands count and names).

Tests first: `release_all` sends each owned device's release transcript once; a second call and the later `Drop` send nothing; `Drop` of an owned device sends the release once; virtual devices disconnect.

Commits: (1) `fix: release every Launchpad when LaunchDeck quits` (`RunEvent::Exit` handler calling `Runtime::release_devices()`; body: NOT VERIFIED in the app window (f), no hardware (b)); (2) `feat: add commands for Launchpad brightness, sleep and scrolling text`.

Validation: `$C test --lib devices::`, `$C test --lib core::runtime::`; `$C clippy --all-targets -- -D warnings`; full gate.

Do not: start the app; touch `tray.rs` unless `app.exit` is shown not to emit `RunEvent::Exit`; add TypeScript.

#### M9. Virtual parity (Rust, M)

Goal: a virtual Mini that behaves like the physical adapter, the virtual X unchanged. Read section 13.

Files: `src-tauri/src/devices/virtual_device.rs`, `src-tauri/src/devices/novation/profile.rs` (`virtual_model`), `src-tauri/src/devices/novation/mod.rs` (re-export), `docs/architecture/CODEBASE_MAP.md` (devices row).

Tests first: `VirtualLaunchpadX::new()` identity, capabilities, features and frames unchanged (existing tests untouched); virtual Mini: id `virtual-launchpad-mini-mk3`, press reports 127, a pulsing `Rgb(40,110,255)` shows `palette_rgb(nearest(..))` with `Animation::Pulse`, static colours exact, asleep → all-off frame, the first press wakes it and is not forwarded (nor its release), brightness and text recorded in `details()`.

Commits: (1) `refactor: build the virtual device from a virtual model` (X unchanged); (2) `feat: a virtual Launchpad Mini MK3 with the hardware's lighting`.

Validation: `$C test --lib devices::`, `$C test --no-fail-fast` (integration tests use the virtual X).

Do not: change `VIRTUAL_ID`, the app's default simulator or any CSS.

#### M10. Runtime end-to-end tests and measurements (Rust, M)

Goal: section 14.4 and 14.6. Files: new `src-tauri/src/core/runtime/device_tests.rs`, `src-tauri/src/devices/novation/measure_tests.rs`; modified `src-tauri/src/core/runtime/{mod.rs (#[cfg(test)] mod device_tests), devices.rs (#[cfg(test)] hooks: swap the hardware source, register a device)}`, `src-tauri/src/devices/manager.rs` (`#[cfg(test)] set_hardware`), new `docs/devices/hardware-testing.md` (measurements section).

Tests: the scenarios of 14.4 for the Mini, the X and the virtual Mini, with polling waits (≤ 2 s) because the runtime's render loop runs; event assertions on `rt.bus()`. Then run `$C test --lib novation::measure_tests -- --ignored --nocapture` (the narrower filter keeps the runtime's own ignored measurement out) and record the printed table in `docs/devices/hardware-testing.md` with "host-side, fake transport; not device latency" and the batching rule of section 8.4.

Commits: (1) `test: drive a Launchpad Mini MK3 through the real runtime`; (2) `test: measure the device layer and record the batching rule`.

Validation: `$C test --lib core::runtime::`; `$C test --no-fail-fast`.

Do not: add a `RuntimeConfig` field or public API for tests; claim device latency.

#### M11. Physical acceptance suite (Rust + script, M)

Goal: section 15. Files: new `src-tauri/src/devices/novation/hardware_tests.rs`, `scripts/launchpad-acceptance.sh`; modified `src-tauri/src/devices/novation/mod.rs` (`#[cfg(test)] mod hardware_tests;`), `docs/devices/hardware-testing.md`.

Decisions: steps 1-15 of section 15.2; yes/no on Side 0 / Side 7; report file; graceful exit without hardware. Run it here once: it must print the NOT HARDWARE VERIFIED line and pass (it creates CoreMIDI clients only; no window).

Commits: (1) `test: add the Launchpad hardware acceptance suite` (body: written, run here only without a device: NOT HARDWARE VERIFIED).

Validation: `sh scripts/launchpad-acceptance.sh` with `LAUNCHDECK_CARGO=$C`; full gate.

Do not: run it as part of `check` or CI; open the DAW port unless `LAUNCHDECK_ACCEPTANCE_DAW=1`.

#### M12. Device documentation and plan records (docs, M)

Goal: section 16.2. Files: new `docs/devices/{launchpad-mini-mk3.md, novation-mk3-protocol.md, capabilities.md}`; modified `docs/devices/{hardware-testing.md, launchpad-x.md}`, `docs/architecture/{CODEBASE_MAP.md, overview.md}`, `PRODUCTIZATION_PLAN.md` (one row in section 12 for the device lane, one entry in section 4 for the deferred Tier 3 items, the rule-zero note).

Decisions: plain English like the existing docs; cite manuals by title and page; leave `README.md` to package R4 and name the Mini there in the R4 note of `PRODUCTIZATION_PLAN.md`; mark every physical claim NOT HARDWARE VERIFIED (b) with the date; no copied tables of the manual beyond the bytes LaunchDeck sends.

Validation: `npm run docs:links`; `node scripts/check-architecture.mjs`.

Commits: (1) `docs: describe Launchpad Mini MK3 support and the shared Novation layer`; (2) `docs: record the device lane in the productization plan`.

#### M13. Device settings UI (after the lanes are merged; TypeScript + a Rust fixture, L)

Goal: section 12.6 in the app; the X-only wording made model-neutral. Runs on the merged branch the orchestrator names, after E2-E5 and M1-M12.

Files: `src/lib/schema/{runtime.ts, workspace.ts}` (`notice`, `details`, `features`; `deviceBrightness` in `preferencesSchema`), `src/lib/ipc.ts` (three commands), `src/lib/preview.ts`, `src/lib/devices.ts` and `devices.test.ts` ("Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3"; notice shown with connected), `src/components/DeviceStatusLine.tsx` ("Plug in a Launchpad"), `src/features/settings/DevicesPanel.tsx` (or the E5 section component that holds devices) and its test, `docs/architecture/FRONTEND_MAP.md` if a component is added; `src-tauri/examples/dump_devices.rs` + `examples/devices.json` + its Rust guard test and a Vitest parse (R10).

Tests: Vitest for wording, the panel (firmware, Programmer Mode, brightness slider including "Device's own level", sleep/wake, capability list, text test), schemas parse the new fixture; `npm run typecheck`, `npm run lint`, `npm test`, `npm run build`; `LAUNCHDECK_CARGO=$C npm run check`.

Commits: (1) `feat: model-neutral device wording`; (2) `feat: device settings for Launchpad brightness, sleep and text`; (3) `test: pin the device list shape with a contract fixture`.

Do not: show notes, CCs or SysEx outside Technical details; send the whole preferences object to change brightness; check the window yourself (NOT VERIFIED (f)).

---

## 17. Definition of Done

Who checks: **A** automated here, **HW** a real Launchpad (NOT HARDWARE VERIFIED until the owner runs M11), **H** a person in the app window (NOT VERIFIED (f)), **CI** GitHub Actions (NOT VERIFIED (a)).

| # | Brief item | Concrete check | Who | Package |
| --- | --- | --- | --- | --- |
| 1 | Dedicated identity | Mini profile; `DeviceIdentity` model `launchpad-mini-mk3`, id `launchpad-mini-mk3:1`; unit test | A | M3, M6 |
| 2 | Detection not only by names | inquiry family + readback product byte decide; tests: other family refused, X with `13 01` accepted | A; HW | M5 |
| 3 | Correct ports | fixtures M1-M5, W1-W5, N1-N3, L1-L6 pass; real names per OS | A; HW per OS | M3 |
| 4 | Inquiry | probe bytes and parsing tests; firmware in details | A; HW | M2, M5 |
| 5 | Automatic Programmer Mode | acquisition transcript after Look again and after a scan | A; HW | M5, M10 |
| 6 | Verified transition where possible | `0Eh` readback → confirmed; silent → notice | A; HW | M5 |
| 7 | 64 grid | 64 note presses/releases decoded in order | A; HW (step 4) | M6 |
| 8 | 8 top | CC 91-98 | A; HW | M6 |
| 9 | 8 side | CC 89…19 | A; HW | M6 |
| 10 | Press/release | value > 0 / 0 / `8xh`; velocity 127 on the Mini | A; HW (values) | M2, M6 |
| 11 | All LEDs | 81 distinct indices; full frame lights the sim's 81 LEDs | A; HW | M2, M6 |
| 12 | Logo LED if supported | index 99 output; never input | A; HW | M2, M6 |
| 13 | Palette | colourspec types 0/1/2 bytes; palette table and nearest tests | A; HW (step 5) | M2, M6 |
| 14 | RGB | type 3 bytes, `to_7bit` | A; HW | M2, M4 |
| 15 | Native flash | `01 idx B 00`; timing 500 ms documented | A; HW (step 7) | M6 |
| 16 | Native pulse | `02 idx P`; 1000 ms documented | A; HW | M6 |
| 17 | Batch | one `03h` per tick, chunked at 81; measured rule recorded | A; HW (step 8) | M4, M10 |
| 18 | Diff/cache | no redundant sends; invalidation table tests | A | M4, M7 |
| 19 | Full redraw after reconnect | replug → clear + full frame | A; HW (step 13) | M5, M10 |
| 20 | No crash on disconnect | unplug with failing sends: no panic, status unplugged | A; HW | M5, M10 |
| 21 | Reconnect restores | E2E unplug → replug → LEDs equal the frame, brightness reapplied | A; HW | M7, M10 |
| 22 | Graceful release | release transcript; layout and feedback restored; release on quit | A; HW (step 14); H (quit) | M5, M8 |
| 23 | Accurate capabilities | Mini caps and features as section 11 (velocity/pressure false) | A | M6, M7 |
| 24 | No Mini leakage into the generic core | grep: `mini`/`lpminimk3` appear only in `devices/novation/`, tests, user-facing sentences and docs; `manager.rs`, `source.rs`, `features.rs`, `midi/`, `renderer` and `core::runtime` name no model; architecture check | A + review | M3-M7 |
| 25 | Launchpad X intact | golden transcript changed only by D-X commits; X capabilities unchanged; X E2E | A; HW (b) | all |
| 26 | Shared protocol only where justified | codec holds only commands defined identically in both manuals (section 4.1); review | review | M2 |
| 27 | Brightness | bytes, preference, reapply, restore tests | A; HW (step 9) | M7 |
| 28 | Sleep/wake | bytes, wake by press, redraw | A; HW (step 11) | M7 |
| 29 | Scrolling text if supported | bytes from p20, limits, stop | A; HW (step 10) | M7 |
| 30 | Firmware visible | `details.firmware`; Devices panel | A; H | M5, M13 |
| 31 | Old firmware explained | evidence-based notice test; docs (Components) | A + review | M7, M12 |
| 32 | Virtual Mini | virtual Mini tests; E2E with the virtual Mini | A | M9, M10 |
| 33 | Protocol tests | `protocol_tests.rs` rows of 14.1 | A | M2 |
| 34 | Transport tests | `device_tests.rs` scenarios of 14.2 | A | M4-M7 |
| 35 | Runtime E2E tests | `core/runtime/device_tests.rs` | A | M10 |
| 36 | Action Library intact | `action_library`, `catalog_contract` | A | every package |
| 37 | Feedback intact | `state_feedback`, `feedback_editor`, `state_hub`, `state_characterization` | A | every package |
| 38 | Agent Control intact | `agent_control_flows`, `agent_control_golden`, `agent_bridge`, `agents_contract` | A | every package |
| 39 | Context Engine intact | `context_engine`, `context_contract`; E2E foreground step | A | every package, M10 |
| 40 | Plugin SDK intact | `plugin_sdk`, `plugin_states`, `plugins_contract` | A | every package |
| 41 | fmt/clippy/tests | `$C fmt --check`, `$C clippy --all-targets -- -D warnings`, `$C test --no-fail-fast` | A | every package |
| 42 | TS checks | `npm run typecheck`, `npm run lint`, `npm test` (in `npm run check`) | A | every package, M13 |
| 43 | Production build | `npm run build` here; the app bundle is R5's (`tauri build` only there, disk permitting) | A; bundle per R5 | M13 |
| 44 | Hardware suite exists | `hardware_tests.rs` + `scripts/launchpad-acceptance.sh`; runs here without a device and says so | A | M11 |
| 45 | Honest physical status | every doc and commit body says NOT HARDWARE VERIFIED (b) for physical behaviour; grep review | review | M12, all |
