# Sharing surfaces

You can save surfaces as a file and give it to someone, and import a file someone gave you. There are two kinds of file:

* a **Surface file** (`.ldsurface`) holds one surface;
* a **Pack** (`.ldpack`) holds up to 16 surfaces and, if you want, the Auto Context rules that show them and questions for the person importing it, such as "Project folder".

A file holds your surfaces: their pages, pads, colours, looks, actions and feedback. It never holds the secrets LaunchDeck keeps for plugins, plugin settings, network permissions, your preferences or anything about your computer, and never a plugin itself: the person importing installs the plugins it needs. A password typed into a pad is another matter: LaunchDeck refuses to save the ones it recognises and lists the pads to check before you share.

Status: export and import have not yet been used in the app's window by a person, on any system: NOT VERIFIED. The words below are those of the import and export screens as built for this version.

## Export a surface or a Pack

1. Press **Export…** in the header of a surface (that surface is then chosen), or next to **Surfaces** in the sidebar.
2. Under **Surfaces**, tick the surfaces to share. One surface and nothing else makes a Surface file; more surfaces, context rules or questions make a Pack.
3. Under **About the file**, give a **Name**, **Version**, **Id**, **Author**, **Author's web address**, **License** and **Description**, or leave them empty: LaunchDeck proposes a name (the first surface's), version 1.0.0 and an id, and shows the proposals in grey.
4. Under **Context rules**, tick **Include Auto Context rules** to add your rules that show the chosen surfaces. A rule names its application as this computer knows it. For each rule you can:
   * add the id the application has on another kind of computer, so the rule works there too: choose it under **Other system**, type it in **Id on that system** (for example `com.microsoft.VSCode` on macOS, `Code.exe` on Windows, `Code` on Linux) and press **Add**;
   * or tick **Ask the person importing for the application**, and write the **Question** they will see.
5. Under **Values on this computer**, every text a pad carries into the file is listed, such as a folder or an address. Tick **Ask the person importing this** for any that differ on their computer, choose how it is **Asked as** (a path, a web address or a text) and write the **Question**. Their answer then goes into the pad, and your value stays out of the file. Values that look like a path on this computer are marked.
6. Read **What the file holds**: the file's name and size, the surfaces, the plugins it needs, every pad that runs a command, sends a request or starts a program (check that none of them holds a password or a token), what was left out, warnings, and **To fix before saving**. For example, a pad that holds a password or a token, such as an address with a password in it, cannot go into a file: ask the person importing for that value instead, or remove it from the pad.
7. Press **Save**. The file goes to your **Downloads** folder, named after the name and version, for example `development-1.0.0.ldpack`. A file already there is never overwritten; the new one gets `-2`, `-3` before the extension. LaunchDeck says where it saved the file, and **Show** opens the folder.

On macOS the system may ask once whether LaunchDeck may use your Downloads folder. If you said no, LaunchDeck says so and where to change it: System Settings, Privacy & Security, Files and Folders. (This prompt has not been seen in the app yet: NOT VERIFIED.)

## Import a file

1. Press **Import…** next to **Surfaces** in the sidebar, then **Choose a file…** and pick a `.ldpack` or `.ldsurface` file.
2. If LaunchDeck cannot use the file, it says **This file cannot be imported**, why, and **Nothing was changed.** The code and the place in the file, for a developer, are under **Details**.
3. Otherwise you see what importing would do, before anything changes. Each choice you make updates the preview.

### What the preview shows

The sections appear in this order; a section with nothing to say is left out.

* At the top, the file: its name, version, kind, description, author and license, and **Publisher: not verified**. LaunchDeck checks that the file was not damaged or edited since it was written, but not who wrote it: the author is only a name the file gives. Import files from people you trust.
* **Compatibility**: whether every pad fits each of your devices, and how many do not. A pad that does not fit is still imported.
* **Already imported**: if you imported this file before, each earlier copy with its version, and whether you changed it since (**changed here**). Choose **Import as a new copy** (proposed) or **Replace copy 1**, **Replace copy 2**… (see [Importing again](#importing-again)).
* **Surfaces to add**: each surface with how many pages and pads. If you already have a surface with the same id, the new one gets another one (**id changed**); if you have one with the same name, the preview says so and both keep the name. Imported surfaces are marked **Imported** in the sidebar.
* **Context rules**: the rules the Pack offers, each **unchecked**. A rule is added only if you tick it. Each shows the application's id it matches on this computer: the name beside it comes from the file and is only for reading. Under each, what would happen: added, or not added because one of your own rules already names that application (yours stays), because the file does not name the application for your kind of computer, or because you already have as many rules as LaunchDeck keeps. An import never turns Auto Context on, and never changes your own rules.
* **Plugins needed**: each plugin the pads use, with its id, named as installed here (a missing plugin is named as the file names it), marked **Compatible**, **Disabled** (installed but off, or failed to start), **Incompatible** (another version) or **Missing**. None of them stops the import (see [Missing plugins](#missing-plugins)).
* **Questions**: what the Pack asks you, such as a folder (a full path), an address, a choice from a list, a text, or an application (chosen from the applications LaunchDeck has seen in front since it started). A problem with an answer is shown under it.
* **Pads that run things**: every pad that runs a command, sends a request or starts a program, with exactly what it runs, and every pad that gives an agent an instruction (a task, a message, or approving, rejecting or retrying what the agent asked). For the first three, choose **Remove what they run** (proposed: the pads stay, with their labels and looks, and do nothing when pressed) or **Keep them** (they run when pressed). Instructions to an agent are listed apart, under **Instructions to an agent**, and are kept whichever you choose: read them before you import.
* **Warnings**, such as a pad that opens a surface the file does not include (it opens a surface of that id on your computer, if you have one).
* **Details**: the ids and digests of the file, and how its surface ids become ids here.

### Import, or cancel

**Import** is available once nothing is waiting for you; what is still waiting is listed above it (a choice to make, a question to answer). **Cancel** leaves everything as it was. Nothing runs while you look at a file or import it: no command, no request, no program, no plugin.

After importing, LaunchDeck says what it added and which pads need a plugin, and, if it added rules while Auto Context is off, that they apply once you turn it on. **Open** followed by the surface's name takes you to the first imported surface, and **Import another file…** starts again. If something changed in the meantime so that the import would now do something else, you see **Something changed since this preview; check it again.** with the preview as it is now; nothing was imported yet. If the import cannot be saved, nothing is changed and the reason is shown.

## Missing plugins

A pad whose plugin is missing, off or in another version is imported as it is. It shows as unavailable until the plugin runs, and feedback that follows the plugin sees its values as not available until then. You do not need to import again:

* if the plugin is one of the examples that come with LaunchDeck, press **Install example** next to it in the import preview, or **Install** under **Example plugins** on the **Plugins** screen;
* otherwise put the plugin's folder in the plugins folder (**Plugins**, **Show plugins folder**) and press **Rescan**.

Once the plugin runs, its pads work. A plugin that needs the network still asks for each address on the **Plugins** screen; a file never gives one permission. LaunchDeck never downloads or installs a plugin by itself.

## Importing again

When you import a file you imported before, choose:

* **Import as a new copy**: another set of surfaces, next to the first.
* **Replace copy 1** (or another copy): each surface of that copy is replaced in place by the file's version, keeping its place in the sidebar. Surfaces the new file adds join that copy; surfaces it no longer has are kept as they are. If you changed one of the surfaces being replaced, tick **Replace surfaces changed here since they were imported** to confirm that your changes will be overwritten.

An older version, or the very same file, can be imported the same way; the preview says which it is. Nothing is merged and nothing updates by itself: LaunchDeck never checks for a newer version of a file.

## Removing an imported surface

An imported surface is deleted like any other. Open it, switch to Edit mode, press **Delete surface** in the surface's header and confirm with **Delete**. The confirmation names what goes with it: Auto Context rules that show the surface are removed, and a fallback that shows it goes back to keeping the surface on screen. Pads on other surfaces that open it keep their setting and, when pressed, say the surface is missing. The only surface left cannot be deleted.

To remove everything at once instead, **Settings**, **Workspace**, **Reset to starter surfaces** replaces all your surfaces, imported or not, with the starter ones.

More: [what the files hold and how they are protected](../artifacts/README.md).
