# The Action Library

The Action Library is a panel on the left of the Surface editor, in Edit mode. The physical surface stays in the middle and the inspector stays on the right. In Live mode the panel is replaced by the page list, so nothing here costs space while performing.

## Finding an action

Press `/` anywhere on the page (outside a text field) to jump to the search. The panel opens first if it is collapsed.

Search is local: React filters the catalog it already holds, so typing never waits for Rust. It splits the query into words, and **every word has to match** something about the action. A word is scored by where it matches, best first:

| Where the word matches | Weight |
| --- | --- |
| The start of the name | highest |
| The start of a word in the name | high |
| A keyword (exactly, then by prefix) | high |
| The category, the source ("plugin", "built-in", the plugin's name) | medium |
| The description (a word start counts more than the middle of a word) | low |
| The id | lowest |
| A typo: one inserted, missing, wrong or swapped letter in a name word or keyword, or skipped letters in a name that starts the way the word does (`kbshrtct` finds *Keyboard shortcut*) | lowest |

Words under four letters only match as written, because three letters are a subsequence of almost anything. Accents and case are ignored. A name that starts with the whole query is boosted and an available action ranks a little above an unavailable one with the same score. Nothing is dropped for being unavailable.

* With no query the list is grouped by category.
* With a query it is one list ordered by score; each row still shows its source.
* The **Category** select narrows to one category and works together with the search.
* A line under the search says how many actions were found, or that nothing matched, and *Show everything* resets the search and category.

Each row shows a glyph (the action's own, or its category's initial), its name, where it comes from (*Built-in*, *Agent Control*, *Plugin: Hello Plugin*), a *Privileged* label if it is, and an *Unavailable* label if it cannot run now. Selecting a row shows its description, its settings and the reason it cannot run, if that applies.

If providers are not running, a note at the bottom says how many and why, with a link to Plugins. If the catalog has problems (a duplicate id, a bad schema), they are listed there too.

## Giving an action to a pad

Three ways. All of them end in the same place: a *draft* in the inspector, saved with **Save**. Nothing about the pad changes until then.

1. **Select, then assign.** Select a pad (an empty cell is fine), pick a row, choose *Press* or *Long press*, and press **Assign to “Pad name”**. `Enter` in the search and a double click on a row do the same. With no pad selected, the button is disabled and the hint says to select one; `Enter` and a double click show the same request as a notice.
2. **Drag.** Drag a row onto any cell in Edit mode. An empty cell becomes a new pad in the draft. A pad that already has an action asks first (below). A drop on the inspector's *On press* or *On long press* section assigns to that slot.
3. **From the inspector.** An empty slot offers *Browse the library*, and an assigned one offers *Change*. Both move the cursor to the search.

Dragging uses HTML5 drag and drop. Tauri's own file-drop handler would swallow those events inside the desktop app, so it is turned off (`"dragDropEnabled": false` in `tauri.conf.json`); LaunchDeck does not use file drops. Dropping something that is not an action is ignored.

### A new pad takes the action's look

A pad that nobody has touched yet takes the action's suggested label, colour and icon when you assign. Any other pad keeps its own. For an existing pad the inspector offers **Use suggested look** when the action has hints that differ from the pad, and applies them only when pressed. Looks and feedback rules are never touched.

### Replacing on purpose

Assigning to a slot that already holds a different action changes nothing until you confirm. The inspector names what is lost:

> This replaces “Open URL” and its 1 setting with “Keyboard shortcut”. The pad’s label, colour, icon, looks and feedback rules are not changed.

**Replace** applies it to the draft and **Keep current** (or `Escape`) cancels. Assigning the action that is already there does nothing, and says so, so its settings are not reset.

Even after Replace, nothing is saved until **Save**, and **Revert** returns to the stored pad.

## Configuring

The inspector shows one section per slot with the action's name, source and description, a **Privileged** warning where it applies, any unavailable reason, and the form drawn from the action's [parameters](parameters.md). Errors appear under the field, and the slot cannot be saved while any is open. Help text sits under each field.

**Test run** executes the draft through the real engine, as if the pad were pressed, and shows *Running…*, *Done* (with the returned value, shortened) or *Failed: the message* beside the button. It saves nothing and is disabled while the settings have issues. Shell and HTTP run here only because a person pressed the button for a pad they are editing.

## Feedback while running

Execution feedback is the existing runtime feedback, not a second system:

| Stage | Event | On the pad |
| --- | --- | --- |
| Pending | `action.started` | The pad's `running` look, if it has one and no feedback rules. |
| Success | `action.completed` | `success` for 2.5 s. |
| Failure | `action.failed`, with the message | `error` for 4 s. |

A pad with feedback rules is driven only by its rules. The event payload carries `action` as `provider.action`, the control and the page.

## Keyboard and accessibility

Everything works from the keyboard alone.

| Keys | Effect |
| --- | --- |
| `/` | Focus the search (not while typing in a field). |
| `↓` `↑` | Move the highlighted row. |
| `PageDown` `PageUp` | Move six rows. |
| `Enter` | Assign the highlighted row to the selected pad. |
| `Escape` | Clear the search; with nothing to clear, leave the field. |
| `Escape` on a replace confirmation | Keep the current action. |

* The search is a `combobox` over a `listbox` of `option` rows, with the highlighted row announced through `aria-activedescendant`. The result count is a polite live region.
* Focus rings stay visible. The collapse button and the rail are real buttons with names.
* State is always a word: *Unavailable*, *Privileged*, *Plugin: …*, *(needed)*, *Failed: …*. Colour only supports it.
* Form fields have labels, `aria-required`, `aria-invalid` and `aria-describedby` for help and errors.
* Buttons, form fields and rows in the library and the assignment editor are at least 32 px tall; rows are 44 px.
* Drag is optional: every drag has the select-and-assign equivalent.

The panel can be collapsed to a narrow rail with a count, and keeps its search and category while the surface is edited.
