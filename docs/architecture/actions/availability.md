# Availability

Whether an action can run is part of its definition, not a filter on the list. An action that cannot run now stays in the library with a reason, because a person has to be able to put *Approve* on a pad while nothing is waiting for approval.

```json
{ "status": "available" }
{ "status": "unavailable", "cause": "state",    "reason": "Nothing is waiting for approval." }
{ "status": "unavailable", "cause": "provider", "reason": "The plugin is turned off in Plugins." }
```

## Where it comes from

| `cause` | Meaning | Source | Fixed by |
| --- | --- | --- | --- |
| `provider` | The provider is off or broken, so none of its actions can run. | The plugin host status: *disabled*, or *error* with the plugin's own failure text. | Turning the plugin on or reloading it in **Plugins**. |
| `state` | The action works, but not in the current situation. | The provider's `availability(action)`. Agent Control answers from the function the pads already use to dim themselves, for the focused track. | Nothing to fix: the situation changes. |

`provider` wins when both apply. The reason is a sentence for a person, not a code. Providers that do not override `availability` are always available.

Built-in providers are never `provider`-unavailable. Agent Control's commands are `state`-unavailable when the agent does not support them, is offline or monitor-only, no track is focused, nothing is pending, or the track is in the wrong status.

## What a person sees

* **In the library.** The row carries an *Unavailable* label (text, not only colour) and the reason is in its tooltip and in the detail under the list. Search still finds it, and ranks an available match slightly higher. Selecting and dragging it work as for any other row.
* **In the editor.** The assignment shows *Unavailable now* with the reason. For a `provider` cause it says the assignment is kept and links to **Plugins**. For a `state` cause it says the action can still be assigned and runs when the situation allows.
* **On the surface.** A pad whose assigned action cannot run because of its **provider** (or because the provider is gone) shows a `!` flag with an accessible name and tooltip. A pad that is only waiting for the right moment is not flagged: *Approve* with nothing pending is normal, and the Agent Control pads already dim themselves.
* **Test run.** Disabled while the settings have issues or the provider is not running, with a line that says why.

## Missing

An assignment can name a provider or an action the catalog does not have: the plugin was uninstalled, a newer version of it removed the action, or a file came from another machine. That is *missing*, which is different from unavailable: there is no definition to show.

* The assignment is kept exactly as saved. Loading, displaying and saving a surface that contains it all work, and nothing is deleted.
* The inspector shows the id it expected (`acme.notes.notes.add`), says whether the provider or only the action is gone, and lists the saved settings as read-only text so none of them is lost.
* The person can remove it, or pick another action in the library to replace it, with the same confirmation as any replacement.
* If the provider comes back, the assignment works again with no step in between.
* Saving a control only validates a slot that **changed**, so a pad can be renamed or recoloured while its plugin is missing.

Actions the migration did not recognise live under the provider id `legacy`, and are shown the same way.

## At press time

The catalog is information for the editor, not the thing that decides. Pressing a pad runs `prepare` against the live registry and then the provider, whatever the catalog said when the editor opened.

| Situation | Result |
| --- | --- |
| Provider missing | `action.failed` with `` `acme.notes` is not installed or was removed ``. |
| Action missing | `action.failed` with `` `acme.notes` has no action `notes.add` ``. |
| Plugin disabled or in error | `action.failed` with the message the Plugins screen shows. |
| State not right (Approve with nothing pending) | The provider refuses with its reason and `action.failed` follows. |
| Arguments no longer valid | `action.failed` with the field messages; the provider never runs. |

An unbound pad then shows the usual automatic error state; a pad with feedback rules follows its rules.

## Staying current

The catalog in the snapshot is evaluated when it is built. React refreshes it:

* when the plugin list changes (enable, disable, reload, rescan, settings), which can change `provider` availability;
* when the person switches to **Edit**, because what is available may have changed while they were away;
* about 0.4 s after Agent Control state changes while editing, because `state` availability moved. A burst of updates is one refresh.

In Live mode Agent Control changes do not refresh the catalog, because only the editor shows it.
