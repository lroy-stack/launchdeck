# Using a Launchpad Mini MK3

LaunchDeck works with a Novation Launchpad Mini MK3 over USB. No driver is needed. Its 64 pads, the 8 buttons along the top (the arrows, Session, Drums, Keys, User) and the 8 down the right are pads of your surface, and the Novation logo is a light.

**Hardware status: NOT HARDWARE VERIFIED.** LaunchDeck was written and tested without a Mini MK3 attached: it follows Novation's Programmer's Reference Manual and User Guide, and is tested against a simulated unit. Everything below is what LaunchDeck is built to do, not what was seen on a unit. If you have one, [trying it](#trying-it-on-a-unit) and reporting what you see is the most useful thing you can do.

## Connecting

1. Quit anything else that uses the Launchpad: Ableton Live, Novation Components, another controller program.
2. Plug it in. Within a few seconds **Settings, Devices** lists **Launchpad Mini MK3** as **Connected**, and the pads show the surface on screen. If it does not appear, press **Look again**.

LaunchDeck first asks the unit what it is, so a unit in its bootloader or another model is never driven by mistake. While connected, the unit is in its **Programmer Mode**: the pads only report presses and show what LaunchDeck draws, and the unit's Setup menu is unavailable. **Disconnect**, or quitting LaunchDeck, gives it back in the layout it had, with its LEDs, feedback setting and brightness as before.

The statuses in **Settings, Devices** (Connected, Found, not connected, Unplugged, Could not connect, Not supported yet) and what to do about each are the same as for the [Launchpad X](launchpad-x.md#connecting). Its line also shows the firmware version and whether Programmer Mode was confirmed. If the unit's firmware is too old for brightness or sleep, its line says so; Novation Components can update it. LaunchDeck never updates firmware itself.

If another program switches the unit away (a DAW, for example), LaunchDeck takes it back; after three times within 30 seconds it leaves the unit to the other program and says so. Use one or the other.

## In Settings, Devices

For a connected Mini MK3:

* **Brightness.** One slider for every Launchpad that can change its brightness, in percent. **Device's own level** leaves each unit at its own setting. LaunchDeck gives each unit its own level back when it lets go of it.
* **Sleep** turns the LEDs off until you press **Wake**, or press a pad on the unit (that press only wakes it; it does not run the pad).
* **Scroll a test text** scrolls "LaunchDeck" across the pads once; **Stop the text** stops it.
* A list of what the unit can do, including its own flash and pulse and their pace.

## Colours and animation

Pads that do not move show exact colours. **Pulsing and flashing pads are animated by the unit itself**, smoothly, in the nearest colour of its fixed palette of 128: a pulsing pad may be a slightly different shade from the same colour standing still. The unit flashes every half second and pulses every second. A program that sends MIDI clock to the unit (a DAW) changes that pace until the unit is switched off. The simulator on screen animates at the Launchpad X's pace instead (0.7 and 1.4 seconds).

The Mini's pads are not velocity-sensitive: every press counts as a full press.

## Trying it on a unit

1. Connect it as above.
2. Open the Agent Control surface in Live mode and press **Run Agent Demo**. The grid should mirror the pads on screen; running agents pulse, approvals and failures flash.
3. Check the colours of static pads against the screen, and whether pulsing pads look close enough.
4. Check the top row, the right column and the logo.
5. Try Brightness, Sleep and Wake, and Scroll a test text in **Settings, Devices**, then unplug and plug in again, and switch the unit off and on while it is connected.
6. If you can, run the acceptance suite from a checkout of the repository, `sh scripts/launchpad-acceptance.sh`. It asks you questions about what the unit shows and writes a report ([hardware testing](../contributing/hardware-testing.md)).

## Limitations

* Not tested on a unit (above).
* If LaunchDeck crashes or is killed, the unit stays in Programmer Mode until LaunchDeck starts again or the unit is unplugged and plugged in again (it always starts in its normal mode).
* Pulsing and flashing colours are the nearest palette colour, and their pace follows any MIDI clock the unit received.
* Only one program at a time: on Windows a port another program holds cannot be opened at all.
* With two identical units, which one is "Launchpad Mini MK3" and which "Launchpad Mini MK3 (2)" may change after a replug.
* The simulator on screen is a Launchpad X; there is no simulated Mini in the app.
* On Linux, LaunchDeck needs the ALSA sequencer (`libasound2`).
* Settings scrolls a fixed test text; there is no field for your own text.

How LaunchDeck talks to the unit, byte by byte, with the manual pages it follows and the questions only a unit can answer: [the Launchpad Mini MK3 device layer](../architecture/devices/launchpad-mini-mk3.md).
