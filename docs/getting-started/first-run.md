# The first run

The first time LaunchDeck starts, it shows one screen instead of the app: **Welcome to LaunchDeck**. It asks what your pads are for and puts the matching starter surface on them. It changes nothing else.

Status: the screen is built and covered by Vitest (`src/features/onboarding/firstRun.test.tsx`) and by the headless acceptance test (`src-tauri/tests/release_acceptance.rs`). Nobody has gone through it in the app window yet: NOT VERIFIED, the macOS checks by hand have not been done.

## What the screen shows

**Your Launchpad.** The device LaunchDeck will use, in words: a connected Launchpad X or Launchpad Mini MK3, or the simulator when no Launchpad is plugged in. **Look again** scans for a Launchpad at once. Without one, the simulator on screen works the same way, and a Launchpad can be plugged in at any time. See [Launchpad X](../user/launchpad-x.md) and [Launchpad Mini MK3](../user/launchpad-mini-mk3.md).

**What is it for?** Five choices. Each starter's text is the surface's own description.

| Choice | What Start does |
| --- | --- |
| **Productivity** | Opens the Productivity starter in Live mode: browser, mail, calendar and files, copy, paste, undo, redo, search, screenshot and media keys. |
| **Agent Control** | Opens the Agent Control starter in Live mode: one pad per agent session, APPROVE and REJECT, and the demo. |
| **Development** | Opens the Development starter in Live mode: terminal, editor and a Git page, with harmless placeholder commands. |
| **Accessibility** | Opens the Accessibility starter in Live mode: twelve large pads in colour-coded groups. |
| **Build my own** | Creates an empty surface called "My surface", opens it in Edit mode and puts the cursor in the Action Library search. |

Productivity and Accessibility say that some of their pads press keys for you, and that macOS asks for Accessibility permission the first time one does.

The **Home** starter (Home Assistant lights and switches) is not offered here; it is in the sidebar with the others.

## The three buttons

* **Start with …** (the name changes with your choice) does what the table says, then closes the screen.
* **Skip** closes the screen and changes nothing. Every starter is in the sidebar.
* **Import a Pack instead** opens the import screen ([sharing](../user/sharing.md#import-a-file)). **Back to the welcome** returns; once an import is applied, the welcome is not shown again.

If a step fails, the screen stays up and says why, with the system's text under **Details**.

## What the first run never does

It never runs the Agent Demo, removes or changes a starter, creates an Auto Context rule, turns Auto Context on, installs a plugin or allows a network address. The surface you start with is opened the same way a click in the sidebar opens it.

## Seeing it again

**Settings, Developer, Show the welcome again** brings the screen back at once. Your surfaces stay as they are. Moving the data folder aside also shows it again, with fresh starters ([starting over](install.md#starting-over)).

## Next steps

* Click a pad on screen in Live mode: it takes the same path as a press on a Launchpad.
* Switch to **Edit**, select a pad and give it an action: [surfaces and the editor](../user/surfaces-and-editor.md) and [the Action Library](../user/action-library.md).
* Make a pad follow something: [the feedback editor](../user/feedback-editor.md).
* Let the surface follow the application in front: [Auto Context](../user/context-engine.md).
