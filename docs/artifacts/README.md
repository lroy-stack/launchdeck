# Surface files and Packs

LaunchDeck shares surfaces as files. A file holds the surfaces themselves, their pages, pads, looks, actions and feedback, so another person can import them and use them on their own computer. It never holds code: plugins are shared separately, as plugins (see the [Plugin SDK](../plugin-sdk/README.md)).

| | Surface file | Pack |
| --- | --- | --- |
| Extension | `.ldsurface` | `.ldpack` |
| Holds | one surface | 1 to 16 surfaces |
| Context rules (which surface to show for which application) | no | optional, and added on import only when the person selects them |
| Inputs (values the importer gives: a folder, an address, an application) | no | optional |

Both are one UTF-8 JSON document of at most 2 MiB, in format version 1, an alpha format. A file names the plugins its pads use, with the versions it accepts, and the device it needs (grid size, pads above and beside it, colour); the importer is told what fits and what is missing, and nothing is installed for them.

## What a file never contains

* **Secrets, grants and plugin settings.** Export never reads them. A value shaped like a password or token in a pad's arguments blocks the export, unless the person asks the importer to give their own.
* **Values the importer is asked for.** An argument bound to an input is written as the input's default, or empty, never as it is on the exporter's computer.
* **Anything about the computer or the person**: preferences, Auto Context's switch and fallback, device ids, times, logs, agent sessions, the hook token, the audit trail, where an imported surface came from.
* **Runtime state**: values, temporary looks, pending actions, plugin status.
* **Code, files or archives**: no scripts, no embedded files, no compression. A file that carries a `signatures` or `assets` member is refused.

## What importing does

The person picks a file and sees exactly what importing it would do before anything changes: the surfaces it adds (renamed when an id is taken), the plugins it needs and whether they are here, the context rules it offers (none selected), the values it asks for, and every pad that runs a command, sends a request or starts a program, shown in full, which the person keeps or removes. Nothing runs while a file is read or imported. The import is written in one step, or not at all.

## Pages

* [FORMAT.md](FORMAT.md): the specification of format version 1, from the envelope to the limits and the rejection codes.
* [security.md](security.md): the threat model, what is checked in which order, and the hostile tests.
* [Sharing surfaces](../user/sharing.md): exporting and importing in the app.
* Examples: `examples/artifacts/development.ldpack`, and the export preview, import plans and rejection the app returns for it, in the same folder.
* Code: `src-tauri/src/artifacts/` (one module per step: limits, JSON pre-pass and canonical writer, format, inspect, export, plan, apply, service); commands in `src-tauri/src/commands/artifacts.rs`.

Status: implemented and tested on macOS only, in this milestone; CI, Linux and Windows NOT VERIFIED. Export and import have not been used in the app's window by a person: NOT VERIFIED.
