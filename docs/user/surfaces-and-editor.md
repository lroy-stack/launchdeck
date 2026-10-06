# Surfaces and the editor

A **surface** is a named layout for your Launchpad: a set of **pages**, each with **pads** (the editor calls them controls). The pads on screen are a simulated Launchpad; a real one, when connected, shows the same page.

Status: everything on this page is built and covered by Vitest and the Rust tests on macOS. Nobody has used it in the app window yet for 0.3.0 (NOT VERIFIED: the manual checks on the Mac have not been done).

## Finding your way

The sidebar has **Overview**, **Agents**, **Surfaces**, **Plugins** and **Settings**, your Launchpad's status (click it for **Settings, Devices**), the Auto Context switch, and the list of surfaces. Click a surface to open it. **+** next to *Surfaces* makes a new, empty surface and opens it in Edit mode; **Import…** and **Export…** are there too ([sharing](sharing.md)). Surfaces you imported carry an **Imported** badge.

The top row of every starter surface switches surfaces, so a Launchpad works without the screen. The tray menu in the menu bar has the surfaces too.

## Live and Edit

Every surface is shown in one of two modes, switched at the top right:

* **Live.** Press pads to run their actions: click on the simulated Launchpad, or press the real one. The pages are listed on the left, and on the right what the surface shows (for Agent Control, the agents' states) and recent activity. A page with no pads says so and offers **Edit**.
* **Edit.** Actions are paused. The [Action Library](action-library.md) is on the left, the pages are a strip above the pads, and the **Inspector** is on the right. Auto Context does not switch surfaces while you edit.

**Labels** shows or hides the pad labels on screen.

## Editing a surface

In Edit mode the surface's header has its **icon**, **name** and **description** as fields: change one and press Enter, or leave the field, to save; Escape undoes. **Delete surface** asks first, and names the Auto Context rules that show the surface: they are removed with it, and a fallback that shows it goes back to keeping the surface on screen. The last surface cannot be deleted.

Pages, in the strip above the pads:

* **+ Add page** asks for a name and opens the new page.
* **⋯** next to a page has **Rename** and **Delete**. Deleting asks first. A surface keeps at least one page.

To bring back the starter surfaces as they were, use **Settings, Workspace, Reset to starter surfaces**. It replaces all your surfaces.

## Editing a pad

Select a pad on screen (an empty cell is fine), or press a pad on the real Launchpad: it is selected in the Inspector. The Inspector edits a draft; nothing changes until you press **Save** (**Add control** for a new pad). **Revert** returns to what is saved, and **Delete** removes the pad.

* **Label** and **Description.** The label is 1 to 40 characters.
* **Width** and **Height** let a pad cover several cells, up to 8 by 8 (the Accessibility starter uses 2 by 2). Pads cannot overlap.
* **Action.** What happens on **press** and on **long press** (held for half a second). Give each one an action from the [Action Library](action-library.md), then fill in its settings. **Test run** runs the draft once without saving. Shell commands and web requests are marked **Privileged**.
* **Resting look.** The pad's icon, colour and animation (**Solid**, **Pulse** or **Flash**) when nothing else applies.
* **Feedback.** Rules that change the pad's look: following a value a plugin or LaunchDeck reports, or showing a look for a while when something happens. See [the feedback editor](feedback-editor.md).

If something is missing or wrong, the Inspector marks the field and says what to fix before **Save** is possible. A pad whose plugin is not installed keeps its settings and is marked as needing that plugin; nothing is deleted.

Pads without feedback rules show what their action does: running while it runs, then success for 2.5 seconds or error for 4 seconds.

## The starter surfaces

| Surface | For | What is on it |
| --- | --- | --- |
| **Agent Control** | Watching and steering AI agents | One pad per session coloured by status, large APPROVE and REJECT pads, Pause, Resume, Retry and Stop that light only when they can work, task pads, Run Agent Demo. See [Agent Control](agent-control.md). |
| **Development** | Build, test, ship | Terminal, editor, a Git page. The commands are harmless `echo` placeholders to replace with yours. A "Failing" pad shows the error flash. |
| **Productivity** | Everyday shortcuts | Browser, mail, calendar and files; copy, paste, undo, redo and search; screenshot and play/pause. Which file manager and screenshot shortcut a pad uses depends on the system. |
| **Accessibility** | Fewer, larger, high-contrast targets | Twelve 2x2 pads in colour-coded groups (navigate, edit, scroll and zoom, volume) and a Help page. A usability aid, not a medical device. |
| **Home** | Lights and switches through Home Assistant | Six pads that toggle example lights and switches through the Home Assistant example plugin. Until that plugin is installed (**Plugins**, **Install**), given its address and token, and each pad given your own entity, the pads read as needing the plugin and nothing is sent anywhere. |

Pads that press keys (Productivity, Accessibility) need **Accessibility** permission on macOS, which macOS asks for the first time.

## Tips

* Keep a surface to one job. Use pages for depth, not for unrelated jobs.
* Use colour as meaning (green good, red bad) and animation for what needs attention.
* For accessibility, prefer pads of 2x2 or larger, high-contrast colours, and labels as well as icons.
* To react to an outside tool, add an event rule in the Inspector (**Feedback, Add feedback, An event**). A plugin that reports a value is better read with a state rule: it is right even for a pad made after the value was reported.
* To have a surface appear when you switch to an application, add a rule under **Settings, Auto Context** rather than a pad. See [Auto Context](context-engine.md).
* To give a surface to someone, export it: [sharing surfaces](sharing.md).

Your surfaces are saved on every change in the app's data folder ([where](../getting-started/install.md#where-launchdeck-keeps-its-data)). If the file cannot be read when LaunchDeck starts, it starts with the starters, keeps your file as `workspace.json.bak`, and says so in a banner with **Show folder**. How the file is built, for developers: [the surface model](../architecture/surface-format.md).
