# Desktop end-to-end checks

These drive the **real LaunchDeck window**: the compiled application, its WebKitGTK web view, the Rust runtime and real WebAssembly plugins. A step clicks, types and presses keys through WebDriver the way a person would, and checks what is on screen and what the virtual Launchpad shows.

They are slow (minutes), need a Linux desktop stack, and are **not** part of `npm test` or `cargo test`. Run them before a release, or after changing anything a component test cannot see: focus, real key handling, the web view, the path from a press to an LED.

## What you need

| Tool | Why | Install |
| --- | --- | --- |
| A built LaunchDeck | The thing under test | `npx tauri build --no-bundle` (release), or `cargo build --manifest-path src-tauri/Cargo.toml` (debug) |
| `tauri-driver` | Starts the app and speaks WebDriver for it | `cargo install tauri-driver --locked` |
| `WebKitWebDriver` | The web view's own driver | Debian/Ubuntu: `apt install webkit2gtk-driver` |
| `Xvfb` | A display when there is none | `apt install xvfb` |
| Python 3.9+ | Runs the steps. Standard library only. | |

Linux only: `tauri-driver` has no macOS support, and on Windows it needs a different native driver that this harness does not set up.

The example plugins must be built (`examples/plugins/*/*.wasm`; they are committed, and `sh scripts/build-example-plugins.sh` rebuilds them).

## Running

```bash
python3 tools/desktop-e2e/run.py                 # every suite, on the release build
python3 tools/desktop-e2e/run.py feedback        # one suite
python3 tools/desktop-e2e/run.py --app debug     # the debug build (starts the dev server it loads from)
python3 tools/desktop-e2e/run.py --list          # suites and their steps
```

The exit code is 0 when every check passed. Each check prints `ok` or `FAIL` with what was found instead.

While writing or debugging a step:

```bash
python3 tools/desktop-e2e/run.py feedback --only keyboard    # one step, on the data the earlier steps left
python3 tools/desktop-e2e/run.py feedback --from scoped      # from a step to the end
python3 tools/desktop-e2e/run.py feedback --display :0       # in a window you can watch
```

`--only` and `--from` do not start from a clean app, so run the suite once from the start first.

## What a run touches

Everything a run writes is in `tools/desktop-e2e/.work/` (ignored by git), and nothing outside the repository:

| Folder | Holds |
| --- | --- |
| `data/`, `cache/` | The app's data folder (`XDG_DATA_HOME`, `XDG_CACHE_HOME`): workspace, settings, grants. Emptied at the start of a suite. |
| `plugins/` | The plugins the suite installs, copied from `examples/plugins` (`LAUNCHDECK_PLUGINS`). |
| `shots/` | Screenshots, named `<suite>-<step>-<name>.png`. A step that cannot go on leaves `…-failed.png`. |
| `logs/` | Output of Xvfb, the driver and the app. |

The run starts its own Xvfb on a free display and its own driver on free ports, and stops them when it ends. Your own LaunchDeck data is never read. Two things are shared with the machine: the app's WebSocket port (47821), so close a LaunchDeck that is running, and port 1420 for `--app debug`.

## How it is laid out

```text
run.py            the runner: display, driver, data folder, suites, summary
lib/webdriver.py  a small W3C WebDriver client
lib/desk.py       the app's own words on top of it: sidebar, mode, pads, LEDs, the editor, plugins
suites/*.py       the checks
```

A **suite** is a module with a `NAME`, the example `PLUGINS` it needs, and `STEPS`, a list of functions. The runner empties the data folder, installs the plugins, and runs the steps in order. Every step gets a **new launch of the app** on the data the earlier steps saved, so plugins start from scratch each time, as they do for a user. A suite may define `setup(env)` and `teardown(env)` for something that lives beside the app, such as a local server.

A step is a function of one argument, the `Desk`:

```python
def live(d):
    """Pressing the pad moves the virtual Launchpad."""   # the first line is printed as the title
    d.sidebar("Development")
    d.mode("Live")
    d.click_pad(8, 8)
    d.check("one press shows Active", d.led(8, 8) == "#22c55e", d.led(8, 8))
```

* `d.check(name, ok, detail)` records one thing that must hold. A failed check does not stop the step, so one run reports everything that is wrong.
* A step that raises is recorded as a failed check with a screenshot.
* To add a suite, add `suites/<name>.py` and its name to `SUITES` in `run.py`.

`d.dump(".inspector")` lists the controls under a selector with their names and values, which is the quick way to find out what to click when writing a step.

## Things worth knowing

* **Act like a person.** `d.replace(field, text)` selects all and types, because the driver's own "clear" empties a field without telling React. `d.choose(select, value)` clicks the option.
* **Pads are found by where they are drawn**: `d.click_pad(column, row)` counts from 1, with the top row of round buttons as row 1. `d.led(column, row)` is the colour the virtual Launchpad shows.
* **The driver drops a connection now and then.** `tauri-driver`'s connection to WebKitWebDriver is sometimes reset before a request is delivered. The client sends the request again and the run says how often it happened. If the driver stops answering altogether, the suite is run once more from the start.
* **Focus rings need a real key press.** `:focus-visible` is not set by `element.focus()` from a script, so a check of a focus ring has to get there with `d.keys("Tab")`.

## Suites

| Suite | What it covers |
| --- | --- |
| `feedback` | The feedback editor with the Counter and Channels plugins: authoring from an empty pad, saving, the virtual Launchpad following a state, a plugin turned off and on and taken away, an event's temporary look, scoped states, reordering by button and by Alt+arrows, and the whole authoring again with the keyboard only (which also covers the grid's arrow-key navigation). |
