# Testing the Launchpads without and with hardware

LaunchDeck drives the Launchpad Mini MK3 and the Launchpad X through one device layer (`src-tauri/src/devices/novation/`). This file says how that layer is tested without a unit, how to run the acceptance suite on one, which questions only a unit can answer, what the device layer was measured to cost on the host, and the rule that decides how LED changes are batched. "Plan" below is `LAUNCHPAD_MINI_MK3_PLAN.md`, the plan of the device lane; what LaunchDeck does with each model is in [launchpad-mini-mk3.md](../user/launchpad-mini-mk3.md) and [launchpad-x.md](../user/launchpad-x.md).

**Hardware status: NOT HARDWARE VERIFIED (b).** No Novation device was attached when anything here was written or run (2026-10-06, macOS 15.4.1, Apple M2). Every physical behaviour below is what Novation's manuals say and what a simulated unit does, not what a unit was seen to do.

## Without a unit

| Level | File | What runs |
| --- | --- | --- |
| Bytes | `novation/protocol_tests.rs` | every message LaunchDeck sends or reads, against the hex lines of the Programmer's Reference Manuals, each test naming its page |
| Port names | `novation/ports_tests.rs` | the port names of each system (derived from docs and community projects, not observed) parsed into units |
| One device | `novation/device_tests.rs` | the real `Mk3Device` over `FakeMidi`, an in-memory MIDI backend, with a `SimLaunchpad` behind the ports: a unit written from the manuals that parses every byte itself (`novation/testkit.rs`) |
| The runtime | `core/runtime/device_tests.rs` | the whole app on the same fake ports (below) |
| Measurements | `novation/measure_tests.rs` | ignored by default; the numbers below |
| The acceptance suite | `novation/hardware_tests.rs` | ignored by default, needs a unit (below); its own tests run the steps that ask nothing (detection, inquiry, Programmer Mode, the batching barrier) against a simulated Mini MK3 on fake ports |

`core/runtime/device_tests.rs` starts the real runtime (hardware scanning off), with its render loop, renderer, actions, state feedback and Auto Context on a scripted observer, and lets the real hardware source find a simulated Mini MK3 (then an X) on ports named as macOS names them. Through the app it checks: Look again takes the unit into Programmer mode and its LEDs show the page; a press on the unit is a `control.pressed` event with the unit's id and starts the demo agent, whose pad then pulses green (on the Mini by the unit itself, as the nearest palette colour; on the X drawn by LaunchDeck in RGB, the X never being sent a palette colour); the application in front switches the surface and the unit shows it; the unit's navigation pad goes back; an unplug is reported once and listed as unplugged; a unit power-cycled while away is taken again when plugged in and drawn whole; quitting gives the unit back dark, in the layout it showed. Nothing is ever sent to the DAW interface. A virtual Mini MK3 shows the same pulse in the palette's colour. The test waits up to two seconds for each step, since the render loop runs every 40 ms.

## With a unit: the acceptance suite

`novation/hardware_tests.rs` holds one ignored test, `launchpad_acceptance`, for a real Launchpad Mini MK3 or, when none is plugged in, a Launchpad X (plan section 15). It goes through what the app uses: the system's MIDI ports (`SystemMidi`), the port parser and `NovationHardware` for the system it runs on, and an `Mk3Device` for the unit, plus the suite's own messages on the same link for what the app never sends (readbacks, the palette, gradients, brightness levels, text). Only one link is open at a time, so it also works where a port can be opened only once (Windows).

Run it from the repository, with nothing else using the Launchpad (quit Ableton Live and Novation Components):

```sh
sh scripts/launchpad-acceptance.sh
```

| Variable | Effect |
| --- | --- |
| `LAUNCHDECK_ACCEPTANCE_REPORT` | where the report is written; by default a new `launchdeck-acceptance-<date>.md` in the system's temporary folder; the path is printed at the start and the end |
| `LAUNCHDECK_ACCEPTANCE_DAW=1` | also sends the inquiry to the unit's DAW interface (step 2); otherwise that interface is never opened |
| `LAUNCHDECK_CARGO` | the cargo to run, by default `cargo` |

The script runs `cargo test --lib launchpad_acceptance -- --ignored --nocapture --test-threads=1` in `src-tauri/`. It is never part of `npm run check` or CI: it needs a unit and a person.

**Answering.** Questions appear in the terminal; answer on the unit: **the top right `>` button (lit green) is yes, the bottom right button (Stop/Solo/Mute, lit red) is no.** A question not answered within 60 s is recorded as skipped and the suite goes on. Steps that ask for an action wait 60 s too (unplug, plug in, press to wake); pressing every control (step 4) ends 60 s after the last press.

**Without a unit** the suite lists the ports it sees, prints `No Launchpad Mini MK3 or Launchpad X found: NOT HARDWARE VERIFIED` and passes. **With a unit** it runs every step and fails if a step failed (a check failed or the owner answered no where yes was expected); skipped steps do not fail it. The unit is given back at the end.

| Step | What it does | Answers (plan section 1) |
| --- | --- | --- |
| 1. Detection | every input and output port, name and id, in the system's order; how each name reads (model, unit key, interface role); the units found; the pair chosen; on Windows, which naming style | Q1, Q16 |
| 2. Inquiry | the Universal Device Inquiry on the MIDI interface: reply bytes, family, firmware digits, reply time; the layout the unit shows before LaunchDeck takes it; with `LAUNCHDECK_ACCEPTANCE_DAW=1`, the same inquiry on the DAW interface | Q3, Q15 |
| 3. Programmer Mode | LaunchDeck takes the unit as the app does (transcript, time to owned); `0Eh` reads back `01`; the layout read back in Programmer mode | Q4 |
| 4. Inputs | every pad and button lit dim; each press and release turns it green; the raw bytes of each, the channels, the press values and the release form; every press must reach LaunchDeck as input | Q5, Q6 |
| 5. Palette | entries 0-63, then 64-127 on the grid, row by row from the top left, to compare with the manual's chart | Q8, plan 8.3 |
| 6. RGB | a 16-step gradient per channel, a white gradient; exact white, red, green, blue | Q8 |
| 7. Flash and pulse | the unit's own flash and pulse: about twice and once a second? then a static colour over the flash: does the flash stop? | Q8, plan 8.5 |
| 8. Batching | the readback barrier (below) for 1, 8, 64 and 81 LEDs as one RGB batch, one palette batch, per-LED SysEx and per-LED notes, 15 runs each; the recommendation for `NOTE_LIGHTING_MAX_CHANGES` | Q9, plan 8.4 |
| 9. Brightness | levels 0, 64, 127 with readback, then a sweep in nine steps; the unit's own level is given back | Q10 |
| 10. Scrolling text | "Hello World" looping at 7 pads per second, left to right, in an RGB colour, stopped (the pads under it come back?); a non-looping text (does the unit send anything when it ends?); texts of 64 and 200 characters | Q11 |
| 11. Sleep and wake | LaunchDeck puts the LEDs to sleep (`09h 00`, read back); a press wakes the unit: is the press sent as MIDI, does LaunchDeck keep it from the input, did the unit keep its LEDs, are they all back after LaunchDeck draws | Q12 |
| 12. LED feedback | external feedback off: does lighting by SysEx and by note still show? internal feedback off and on in Programmer mode; the feedback found is given back | Q13 |
| 13. Unplug and plug in again | while held: unplug, plug in; `0Eh` reads back `00` (Live mode) before LaunchDeck takes the unit again; time from the ports' return to owned; is the frame back? | Q14, plan 10 |
| 14. Safe release | LaunchDeck gives the unit back: `0Eh` reads `00`, the layout from step 2 is restored; the owner checks the unit's normal mode and that holding Session opens Setup, presses a pad, and answers once LaunchDeck has taken the unit again; given back once more | Q14 |
| 15. Summary | each step's result, the firmware, the system (macOS version, Linux kernel), the Windows naming style, the measured numbers | |

**The readback barrier** (step 8): the suite sends the lighting messages, then the Programmer-mode query (`F0 00 20 29 02 <product> 0E F7`), and times from the first byte sent to the reply. The unit answers in order, so the reply comes after it processed everything sent before it. The query alone is timed too, so that the time the lighting adds can be read off. The recommendation follows the rule below: notes win only where they are faster than one palette batch by more than 1 ms, counted up from 1 LED.

**The report** is Markdown: a head (start time in UTC, report path, unit), then per step a heading with its result (`pass`, `FAIL` or `skipped`), the open questions it answers, what it printed (indented), its checks as a list (`[pass]`, `[FAIL]`, `[skipped]`, or `[noted: …]` for an answer or an observation nothing is expected of), and its transcript (`+12.345 s -> F0 7E 7F 06 01 F7` sent, `<-` received, the first 60 messages of the step), and at the end the summary table and the numbers. It is rewritten after every step, so a run cut short still leaves one. The next hardware milestone commits a report from a Mini MK3 under `docs/verification/` (plan section 15.3).

**Status.** Written 2026-10-06 and run here once, on macOS 15.4.1 with no Launchpad attached: it listed no MIDI port, printed the NOT HARDWARE VERIFIED line and passed. Steps 2 to 14 have never run on a unit; the steps that ask nothing ran only against the simulated Mini MK3 of the suite's own test. NOT HARDWARE VERIFIED.

## Open questions

What Novation's documents leave open (plan section 1, questions Q1 to Q17), what LaunchDeck assumes until a unit answers, and the step of the suite that answers it. "Not covered" means the suite does not answer it, so it stays open after a run.

| Question | Assumed until answered | Answered by | If the answer differs |
| --- | --- | --- | --- |
| The port names `midir` reports on each system, and which pair is the MIDI interface (Q1, Q2) | the names of `novation/ports_tests.rs`, derived from documents and community projects | step 1, on each system it runs on | a new fixture and a parser fix |
| Whether the DAW interface answers the inquiry (Q2) | not needed: LaunchDeck never opens it | step 2 with `LAUNCHDECK_ACCEPTANCE_DAW=1` | nothing; recorded |
| The X's inquiry family, and its bootloader's (Q3) | `03 01` or the manual's `13 01`; bootloader `03 11`, inferred from the Mini's manual | step 2 on an X (the bootloader: not covered) | the X profile's families |
| How the four firmware digits map to Novation's version names (Q15) | not mapped: the digits are shown as they are | step 2 records them; the mapping needs Novation | wording only |
| How long a unit takes to answer (plan 9.1) | 500 ms, then LaunchDeck goes on without the answer | steps 2 and 3 (reply times) | `REPLY_TIMEOUT` in `novation/session.rs` |
| The values a Mini pad and button send on press, the release form, the channel (Q6) | above 0 is a press; 0 or a Note Off a release; any channel | step 4 | the decoder in `novation/layout.rs` |
| Whether the palette table matches the LEDs (plan 8.3) | `novation/palette.rs`, read off the manual's swatch picture | step 5 (by eye) | the table, in one commit |
| The flash and pulse periods at the default tempo (Q8) | 500 ms and 1000 ms | step 7 (by eye) | the Mini profile's features |
| Whether a static RGB colour stops a running flash, or only changes its colour A (plan 8.5) | it stops it | step 7 | the planner first sends palette 0 to an LED leaving a flash, one commit with a test |
| Whether per-LED notes are processed faster than one lighting SysEx (Q9, plan 8.4) | one SysEx per render | step 8 (the readback barrier) | the batching rule below |
| How 0-127 maps onto the 8 brightness levels of the Setup menu (Q10) | linear: percent × 127 / 100 | step 9 (how many levels are visible) | the mapping in `novation/settings.rs` |
| Whether a brightness set by SysEx survives switching the unit off (Q10) | unknown; LaunchDeck sets it again on every connect anyway | not covered | nothing |
| The longest text, characters outside ASCII, whether a scroll reports its end, the width of a character (Q11) | 64 printable ASCII characters; no end message; a scroll that does not loop is over after six columns a character | step 10 | LaunchDeck's text limits and the estimate in `novation/settings.rs` |
| Whether the press that wakes a sleeping unit is sent as MIDI, and whether the LEDs are kept while asleep (Q12) | either way: that press and its release are not passed on, and the frame is sent whole on waking | step 11 | the sleep handling in `novation/session.rs` |
| Whether external LED feedback off also hides the lighting SysEx; what internal feedback does in Programmer Mode (Q13) | external feedback is switched on while LaunchDeck draws; internal is left alone | step 12 | the configuration in `novation/session.rs` |
| Whether Programmer Mode leaves Legacy mode (Q13) | yes | not covered | handle Legacy mode |
| What the unit shows after a replug; that release gives the person's layout back with Setup working (Q14) | Live mode after power-on; the saved layout restored | steps 13 and 14 | the release sequence |
| What a unit does when its host crashes (Q14) | it stays in Programmer Mode until a host gives it back or it is replugged | not covered (the suite never crashes) | the limitation in [launchpad-mini-mk3.md](../user/launchpad-mini-mk3.md#limitations) |
| What the bootloader's USB device ID changes; how macOS lists two identical units (Q16) | nothing LaunchDeck reads; the k-th MIDI input pairs with the k-th MIDI output | not covered: needs two units | the pairing in `novation/ports.rs` |
| How a DAW next to LaunchDeck takes the unit, and whether 2 s, 3 times and 30 s suit the heartbeat and the takeover guard (Q13, plan 9.1) | `HEARTBEAT` 2 s; `TAKEOVER_LIMIT` 3 within `TAKEOVER_WINDOW` 30 s | not covered: needs a DAW running | the constants in `novation/session.rs` |

After the first run on a Mini MK3: commit its report under `docs/verification/`, settle each answered question in its own commit citing the report, and replace NOT HARDWARE VERIFIED with what was seen, naming the firmware and the system. Native flash and pulse on the Launchpad X are a one-line change of its profile, left for after a run on an X.

## Measurements

`novation/measure_tests.rs`, run by name:

```sh
cargo test --lib novation::measure_tests -- --ignored --nocapture --test-threads=1
```

**Host-side, fake transport; not device latency.** The times are the cost on the computer of encoding the messages and handing them to `FakeMidi`, an in-memory transport: no USB, no driver, no unit. They say nothing about how long a Launchpad takes to show the LEDs. They come from the test build (unoptimised, as `cargo test` builds it; no release build was made) on a machine where another build was running, so they are useful only to compare the strategies with each other: a second run gave the same order with some values up to twice as large (the 81-LED batch of RGB took 32.4 µs instead of 17.0 µs).

Recorded 2026-10-06, macOS 15.4.1, Apple M2, Rust 1.97.0, debug build. Colours differ per LED; the Mini MK3's product byte (the X's messages are the same length).

| LEDs | Strategy | Bytes | USB-MIDI packets | Messages per tick | Encode + send, median of 1000 |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 LED | (a) one `03h` batch, RGB | 13 | 5 | 1 | 2.4 µs |
| 1 LED | (b) one `03h` batch, palette | 11 | 4 | 1 | 2.2 µs |
| 1 LED | (c) per-LED `03h`, RGB | 13 | 5 | 1 | 2.9 µs |
| 1 LED | (d) per-LED channel notes | 3 | 1 | 1 | 0.5 µs |
| 8 LEDs (a row) | (a) one `03h` batch, RGB | 48 | 16 | 1 | 2.8 µs |
| 8 LEDs (a row) | (b) one `03h` batch, palette | 32 | 11 | 1 | 2.2 µs |
| 8 LEDs (a row) | (c) per-LED `03h`, RGB | 104 | 40 | 8 | 8.2 µs |
| 8 LEDs (a row) | (d) per-LED channel notes | 24 | 8 | 8 | 2.6 µs |
| 64-pad page switch | (a) one `03h` batch, RGB | 328 | 110 | 1 | 14.7 µs |
| 64-pad page switch | (b) one `03h` batch, palette | 200 | 67 | 1 | 10.8 µs |
| 64-pad page switch | (c) per-LED `03h`, RGB | 832 | 320 | 64 | 57.5 µs |
| 64-pad page switch | (d) per-LED channel notes | 192 | 64 | 64 | 16.6 µs |
| 81-LED redraw | (a) one `03h` batch, RGB | 413 | 138 | 1 | 17.0 µs |
| 81-LED redraw | (b) one `03h` batch, palette | 251 | 84 | 1 | 12.3 µs |
| 81-LED redraw | (c) per-LED `03h`, RGB | 1053 | 405 | 81 | 67.8 µs |
| 81-LED redraw | (d) per-LED channel notes | 243 | 81 | 81 | 20.7 µs |

(a) is what LaunchDeck sends: every change of one render in one lighting SysEx, static colours as exact RGB, native pulse and flash as palette entries. A USB-MIDI event packet is 4 bytes on the bus and carries one 3-byte channel message or 3 bytes of a SysEx, so an n-byte SysEx takes ⌈n/3⌉ packets. Bytes and packets are exact; only the times are measured.

Other numbers from the same run:

* **Connect to owned** (from opening the ports to the clear frame, renders 40 ms of fake time apart): a Mini MK3 or an X that answers, 2 renders (80 ms); a Mini MK3 whose answers never arrive, 26 renders (1040 ms, the two 500 ms reply timeouts), then drawn with Programmer Mode unconfirmed. A real unit's reply time is unknown.
* **Decode to `DeviceInput`** (a press's bytes handed to the input callback until the input is in the runtime's channel): median 1.7 µs of 1000 (0.8 µs in the second run).
* **Through the runtime** (`core/runtime/device_tests.rs`, printed with `--nocapture`): press on the unit to `control.pressed` on the event bus 43 µs (Mini MK3) and 75 µs (X); a replugged unit, from the rescan to its LEDs showing the page again, 75 ms (Mini MK3) and 88 ms (X), which is the two renders of acquisition plus the test's 10 ms polling. In the app the rescan itself runs every 3 s, so a replugged unit comes back within about 3 s plus those two renders.

## The batching rule

Rule (LAUNCHPAD_MINI_MK3_PLAN.md section 8.4): **every change of one render goes into one `03h` lighting message, unless the hardware measurement shows per-LED channel notes faster by more than 1 ms for palette-only changes of at most k LEDs; then `NOTE_LIGHTING_MAX_CHANGES` in `novation/leds.rs` becomes k, in one commit.** It is 0 today.

Why the host numbers cannot decide it: notes carry palette entries only, so they can stand in for a batch only when every changed LED is off, pulsing or flashing; static colours need RGB, which only SysEx carries. For such palette-only changes, notes take fewer bytes and packets than a palette batch (1 packet against 4 for one LED, 64 against 67 for 64), but one message per LED instead of one per render, and every difference above is a few microseconds on the host. What matters is how long the unit takes to process one SysEx against several notes, which only a unit can show: the acceptance suite (plan section 15, step 8) sends each strategy followed by a Programmer-mode query and times the reply, which arrives after the unit processed everything before it. Until it has run on a Mini MK3, the rule keeps one batch per render. NOT HARDWARE VERIFIED.
