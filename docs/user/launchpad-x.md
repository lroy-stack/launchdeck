# Using a Launchpad X

LaunchDeck works with a Novation Launchpad X over USB. No driver is needed. Its 64 pads, the 8 buttons along the top and the 8 down the right are pads of your surface, and the logo is a light.

**Hardware status: NOT HARDWARE VERIFIED.** LaunchDeck was written and tested without a Launchpad X attached: it follows Novation's manual and is tested against a simulated unit. Everything below is what LaunchDeck is built to do, not what was seen on a unit. If you have one, [trying it](#trying-it-on-real-hardware) and reporting what you see is the most useful thing you can do. Without one, the simulator on screen works the same way.

## Connecting

1. Quit anything else that uses the Launchpad: Ableton Live, Novation Components, another controller program.
2. Plug it in. Within a few seconds **Settings, Devices** lists **Launchpad X** as **Connected**, and the pads show the surface on screen. If it does not appear, press **Look again**.

While connected, LaunchDeck switches the unit to its **Programmer Mode**, in which the pads only report presses and show what LaunchDeck draws; the unit's own Setup menu is unavailable meanwhile. **Disconnect**, or quitting LaunchDeck, gives the unit back in the mode and layout it had, with its LEDs, feedback setting and brightness as before.

The sidebar shows the Launchpad's status. In **Settings, Devices** each device has one line: its name, status, firmware version and whether Programmer Mode was confirmed. **Technical details** at the bottom lists ids and what each device can do.

| Status | Meaning | What to do |
| --- | --- | --- |
| **Connected** | LaunchDeck draws on it and reads its pads. | |
| **Found, not connected** | It is there, and LaunchDeck is not using it. | **Connect** |
| **Unplugged** | It was connected and is gone. It stays listed and reconnects by itself when plugged in again. | Plug it in |
| **Could not connect** | With the reason, for example another program holds its port, or the unit is in its bootloader. | Follow the sentence, then **Connect** or **Look again** |
| **Not supported yet** | Another Launchpad model. LaunchDeck never opens it. | |

If another program switches the unit away (a DAW, for example), LaunchDeck takes it back; after three times within 30 seconds it leaves the unit to the other program and says so.

## In Settings, Devices

For a connected Launchpad X:

* **Brightness.** One slider for every Launchpad that can change its brightness, in percent. **Device's own level** leaves each unit at its own setting. LaunchDeck gives each unit its own level back when it lets go of it.
* **Sleep** turns the unit's LEDs off until you press **Wake** (or press a pad on the unit, which only wakes it).
* **Scroll a test text** scrolls "LaunchDeck" across the pads once; **Stop the text** stops it.
* A list of what the unit can do: 64 pads, buttons, a logo that lights, any colour, how hard a pad is pressed, brightness, sleep, scrolling text, firmware version.

## Colours and animation

Pads show exact colours. Pulsing and flashing pads are animated by LaunchDeck itself, about 25 times a second, so a slow USB connection would show as stutter. The pads are velocity-sensitive; pressure (aftertouch) is not used.

## Trying it on real hardware

1. Connect it as above.
2. Open the Agent Control surface in Live mode and press **Run Agent Demo**. The grid should mirror the pads on screen, with the top row changing colour as the four simulated agents move through their states.
3. Look at pulse and flash: running agents pulse, approvals and failures flash. Watch for stutter.
4. Check the edges: the top row, the right column and the logo should light and respond. If they do not, LaunchDeck's numbers for them are wrong; please report it.
5. Try Brightness, Sleep and Wake, and Scroll a test text in **Settings, Devices**, then unplug and plug in again.
6. If you can, run the acceptance suite from a checkout of the repository, `sh scripts/launchpad-acceptance.sh`. It asks you questions about what the unit shows and writes a report ([hardware testing](../contributing/hardware-testing.md)).

## Limitations

* Not tested on a unit (above).
* If LaunchDeck crashes or is killed, the unit stays in Programmer Mode until LaunchDeck starts again or the unit is unplugged and plugged in again.
* Only one program at a time: on Windows a port another program holds cannot be opened at all.
* The editor shows the first connected device. Several Launchpads can be connected, and all show the same page.
* With two identical units, which one is "Launchpad X" and which "Launchpad X (2)" may change after a replug.
* On Linux, LaunchDeck needs the ALSA sequencer (`libasound2`).
* Settings scrolls a fixed test text; there is no field for your own text.

How LaunchDeck talks to the unit, byte by byte, with what is verified in the manual and what is not: [the Launchpad X device layer](../architecture/devices/launchpad-x.md).
