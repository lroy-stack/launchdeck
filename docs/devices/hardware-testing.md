# Testing the Launchpads without and with hardware

LaunchDeck drives the Launchpad Mini MK3 and the Launchpad X through one device layer (`src-tauri/src/devices/novation/`). This file says how that layer is tested without a unit, what the device layer was measured to cost on the host, and the rule that decides how LED changes are batched.

**Hardware status: NOT HARDWARE VERIFIED.** No Novation device was attached when anything here was written or run (2026-10-06, macOS 15.4.1, Apple M2). Every physical behaviour below is what Novation's manuals say and what a simulated unit does, not what a unit was seen to do.

## Without a unit

| Level | File | What runs |
| --- | --- | --- |
| Bytes | `novation/protocol_tests.rs` | every message LaunchDeck sends or reads, against the hex lines of the Programmer's Reference Manuals, each test naming its page |
| Port names | `novation/ports_tests.rs` | the port names of each system (derived from docs and community projects, not observed) parsed into units |
| One device | `novation/device_tests.rs` | the real `Mk3Device` over `FakeMidi`, an in-memory MIDI backend, with a `SimLaunchpad` behind the ports: a unit written from the manuals that parses every byte itself (`novation/testkit.rs`) |
| The runtime | `core/runtime/device_tests.rs` | the whole app on the same fake ports (below) |
| Measurements | `novation/measure_tests.rs` | ignored by default; the numbers below |

`core/runtime/device_tests.rs` starts the real runtime (hardware scanning off), with its render loop, renderer, actions, state feedback and Auto Context on a scripted observer, and lets the real hardware source find a simulated Mini MK3 (then an X) on ports named as macOS names them. Through the app it checks: Look again takes the unit into Programmer mode and its LEDs show the page; a press on the unit is a `control.pressed` event with the unit's id and starts the demo agent, whose pad then pulses green (on the Mini by the unit itself, as the nearest palette colour; on the X drawn by LaunchDeck in RGB, the X never being sent a palette colour); the application in front switches the surface and the unit shows it; the unit's navigation pad goes back; an unplug is reported once and listed as unplugged; a unit power-cycled while away is taken again when plugged in and drawn whole; quitting gives the unit back dark, in the layout it showed. Nothing is ever sent to the DAW interface. A virtual Mini MK3 shows the same pulse in the palette's colour. The test waits up to two seconds for each step, since the render loop runs every 40 ms.

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
