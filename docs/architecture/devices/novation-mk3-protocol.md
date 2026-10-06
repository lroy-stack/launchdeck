# The shared Novation MK3 layer

LaunchDeck drives the Launchpad Mini MK3 and the Launchpad X with one device layer, `src-tauri/src/devices/novation/`. This file says what the two models share and why, which messages LaunchDeck sends and reads (with the manual pages they come from), where Novation's documents are wrong, and how a model is described as data. The models themselves are in [launchpad-mini-mk3.md](launchpad-mini-mk3.md) and [launchpad-x.md](launchpad-x.md).

**Hardware status: NOT HARDWARE VERIFIED (b).** Every byte here is from Novation's manuals and is tested against them and against a simulated unit; none was sent to a real Launchpad (no Novation device attached, 2026-10-06).

Sources, cited below by their short names. Page numbers are the printed ones, which equal the PDF pages. This file names the commands LaunchDeck uses; for the rest, read the manuals.

| Short name | Document |
| --- | --- |
| Mini PRM | Novation, *Launchpad Mini [MK3] Programmer's Reference Manual*, 2019-11-08 |
| X PRM | Novation, *Launchpad X Programmer's Reference Manual*, 2019-11-08 |
| Mini UG | Novation, *Launchpad Mini User Guide* v2.0 (English), 2024-02-09 |

## Why one layer

The two Programmer's Reference Manuals were compared after normalising their text: for everything they both hold they match word for word, apart from the product byte (Mini `0Dh`, X `0Ch`), the port-name stems (`LPMiniMK3`, `LPX`) and the layout Live mode falls back to. The Programmer Mode diagram, the palette swatches and the flash and pulse figures are the same images in both. The X manual adds commands the Mini lacks (velocity curve, aftertouch, fader velocity, Note mode settings). So the codec is written once, parameterised by the product byte, and holds only what both define identically; what differs is data in a model profile. There is no `if model == …` anywhere in the layer.

What is left out on purpose: the X-only commands (`04h`, `0Bh`, `0Dh`, `0Fh`, `13h`, `15h`-`17h`), the commands of the DAW interface (`01h` fader setup, `10h` DAW mode, `12h` DAW clear, `14h` Session colour), and the Launchpad Pro MK3, which has another product byte, another inquiry family and three interfaces in another order.

## Files

| File | What it holds |
| --- | --- |
| `protocol.rs` | the codec: SysEx framing, the commands below and their readback queries, the inquiry and the reply parser, colourspecs and chunking, channel lighting, scrolling text. Pure, bytes in and out |
| `layout.rs` | the Programmer Mode index map and the input decoder |
| `palette.rs` | the 128 palette colours (approximate) and the entry nearest a colour |
| `profile.rs` | `ModelProfile`, a model as data, and `virtual_model`, a model's simulator |
| `launchpad_mini_mk3.rs`, `launchpad_x.rs` | the two profiles |
| `ports.rs` | the port names of each system, read into units (takes the `Platform`) |
| `hardware.rs` | `NovationHardware`, the device manager's source of Launchpads, and the list of models driven |
| `device.rs` | `Mk3Device`, the `SurfaceDevice` for a unit of any model |
| `session.rs` | what to send to take, keep and give back a unit, and when presses count. Pure |
| `settings.rs` | what brightness and scrolling text send, and the state release needs. Pure |
| `leds.rs` | the LED cache and the planner that turns a frame into lighting messages |
| `testkit.rs` | tests only: `SimLaunchpad`, a Mini or an X as bytes, written from the manuals without the codec |

The layer reaches MIDI through `devices::midi` (`MidiBackend`, `MidiLink`); `midir` is used only in `devices/midi/system.rs`. Nothing outside `devices/novation/` knows a note, a CC or a SysEx byte.

## What LaunchDeck sends and reads

`<p>` is the product byte: `0D` for the Mini MK3, `0C` for the X. Every Novation message has the header `F0 00 20 29 02 <p>`; a command sent without data is its readback query, answered with the header, the command and the current value.

| Purpose | Bytes | Mini PRM | X PRM |
| --- | --- | --- | --- |
| Who are you (Universal Device Inquiry) | `F0 7E 7F 06 01 F7` | p6 | p6 |
| … answer (17 bytes) | `F0 7E 00 06 02 00 20 29 <family 2> 00 00 <four digits> F7` | p6 | p6 |
| Which layout / select layout | `F0 00 20 29 02 <p> 00 F7` / `… 00 <layout> F7` | p7 | p7 |
| Programmer Mode / Live mode / which | `… 0E 01 F7` / `… 0E 00 F7` / `… 0E F7` | p7-8 | p7-8 |
| Lighting, up to 81 colourspecs | `… 03 <colourspec>… F7` | p14 | p15 |
| Scrolling text / stop | `… 07 <loop> <speed> 01 <r> <g> <b> <ASCII…> F7` / `… 07 F7` | p20 | p23 |
| Brightness / which | `… 08 <0-127> F7` / `… 08 F7` | p22 | p25 |
| LEDs asleep or awake / which | `… 09 00 F7` (sleep), `… 09 01 F7` (wake) / `… 09 F7` | p21 | p24 |
| LED feedback / which | `… 0A <internal> <external> F7` / `… 0A F7` | p22 | p25 |

Colourspecs (Mini PRM p14, X PRM p15), as LaunchDeck uses them:

| Type | Bytes | Used for |
| --- | --- | --- |
| `03` RGB | `03 <index> <r> <g> <b>`, each 0-127 | every static colour, and off (`0 0 0`), on both models |
| `02` pulse | `02 <index> <palette>` | native pulse (the Mini) |
| `01` flash | `01 <index> <colour B> <colour A>` | native flash (the Mini), with A = 0: between the colour and off |
| `00` static palette | `00 <index> <palette>` | encoded and tested; not sent |

All the changes of one render go in one lighting message, chunked at 81 colourspecs (one per LED). Lighting by notes on channels 1, 2 and 3 (static, flashing, pulsing; Mini PRM p12-13, X PRM p13-14) is encoded and tested but not sent: `NOTE_LIGHTING_MAX_CHANGES` in `leds.rs` is 0 until a measurement on hardware says otherwise ([hardware-testing.md](../../contributing/hardware-testing.md#the-batching-rule)).

The text command's speed is in pads per second; a value of `40h` or more is negative (the value minus `80h`) and scrolls left to right. LaunchDeck sends the colour as RGB (`01 r g b`), never the palette form.

**Indices.** An LED's index is its Programmer Mode number (Mini PRM p10, X PRM p10): pads `10 × r + c` with `r` and `c` from 1 at the bottom left (11-88), top buttons 91-98, right buttons 89, 79, … 19 from the top, the logo 99. 81 indices, one full lighting message.

**Input.** Pads send Note On (velocity 0 for a release; Note Off is accepted), buttons send CCs, on the MIDI interface (Mini PRM p6, p10). `layout.rs` masks the channel, ignores pressure (`A0h`, `D0h`), clock and anything outside the map, and reports the velocity only for a model whose profile has it (the X); the Mini's presses read 127. Bytes from a callback are split into whole messages first (`devices/midi/framing.rs`); a SysEx goes to the session as a reply, never to input.

**What LaunchDeck never sends:** DAW mode (`10h`), MIDI clock, anything to the DAW interface, the bootloader or firmware commands, and the X-only commands.

The order in which the session sends these (the probe, Programmer Mode, configuration, heartbeat, release) is in [launchpad-mini-mk3.md](launchpad-mini-mk3.md#taking-the-unit-and-giving-it-back).

## The palette

Flash and pulse take palette entries only, and the palette is fixed: 128 colours, published only as a swatch picture without numbers (Mini PRM p11, X PRM p12, the same image), and no MK3 command redefines it. `palette.rs` reads each swatch's fill colour off that picture and removes the picture's grey floor, `c = round((s − 0x61) × 255 / 0x9E)` clamped to 0-255. The result agrees with every colour the manuals name (5 red, 13 yellow, 21 bright green and 23 dim green, 37 turquoise, 45 blue, 0 off) but was never measured on LEDs: it is approximate, NOT HARDWARE VERIFIED. The nearest entry to a colour minimises `2Δr² + 4Δg² + 3Δb²` over entries 1-127, the first index winning a tie; entry 0 (off) only for black. The virtual Mini shows a pulsing or flashing LED in the same entry, through the same function, so it cannot drift from what the hardware is sent.

## Errors in Novation's documents

LaunchDeck and its tests follow the evidence, not these lines:

1. **The X's inquiry family.** X PRM p6 (and the Pro MK3's guide) prints `13 01`, the Mini's family. Ableton's shipped scripts use `03 01` for the X, and a captured reply of a real X reads `F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7` (community project `ruohki/lunchpad`). The X profile accepts both; the product byte of the readback replies then decides.
2. **"Hold the Capture MIDI button"** to enter the Mini's bootloader (Mini PRM p5, Mini UG p21): the Mini has no such button. LaunchDeck never enters the bootloader.
3. **An aftertouch sentence** in Mini PRM p15, copied from the X manual: the Mini has no aftertouch (its command summary, p23, has no aftertouch command).
4. **"Velocity, aftertouch and fader" Setup pages** in Mini UG p18, copied from the X guide: the Mini's pads are not velocity-sensitive.
5. **The decimal line of the text example** (Mini PRM p20, X PRM p23) prints 104 for `6Ch`. The tests use the hex line.

Also not documented anywhere, and so not assumed: the values a Mini pad and button send on press, the output channel of device-to-host messages, how the four firmware digits map to Novation's version names, the longest text, whether a scroll reports its end, what the device ID changes, and what a unit does when its host crashes. [hardware-testing.md](../../contributing/hardware-testing.md#open-questions) lists them with the step of the acceptance suite that answers each.

## Model profiles

A model is a `ModelProfile` (`profile.rs`), data only:

| Field | Mini MK3 | X |
| --- | --- | --- |
| `model`, `name` | `launchpad-mini-mk3`, Launchpad Mini MK3 | `launchpad-x`, Launchpad X |
| `product` | `0D` | `0C` |
| `families` (inquiry) | `13 01` | `03 01`, and `13 01` (the misprint above) |
| `stem_tokens` (port names) | `lpminimk3` | `lpx` |
| `name_tokens` (USB product name) | `launchpad mini mk3` | `launchpad x` (so `Launchpad XL` is not an X) |
| `interfaces` | DAW, MIDI | DAW, MIDI |
| `live_layout` (where Live mode lands) | `05`, Custom mode 2, Keys (Mini PRM p8) | `01`, Note mode (X PRM p7) |
| `capabilities` | velocity off, native pulse and flash | velocity on, animation synthesised by LaunchDeck |
| `features` | palette, flash 500 ms, pulse 1000 ms | no palette, no fixed periods |

Both have an 8 × 8 grid, 8 top and 8 right buttons, the logo, RGB, no pressure, Programmer Mode, batched lighting, brightness, sleep, scrolling text and a firmware version. The meaning of each field is in [capabilities.md](capabilities.md).

A bootloader answers with the first byte of one of the profile's families and `11` as the second (`13 11` for the Mini, Mini PRM p6; for the X this is inferred). A family not in the profile's list, on the profile's ports, is another model and is turned down.

`hardware.rs` keeps two lists: the models recognised on the ports and the models driven. A model recognised but not driven is listed as "Not supported yet"; other Novation names (`launchpad`, `lppromk3`) are listed the same way, once per unit, and never opened. Adding a model is described in [capabilities.md](capabilities.md#adding-a-device-model).
