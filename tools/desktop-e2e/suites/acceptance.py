"""The release acceptance scenario, through the real window.

WRITTEN, NOT RUN: this harness is Linux only and GitHub Actions cannot run
here (account billing), so no run of this suite has been seen. The same
scenario runs headless in `src-tauri/tests/release_acceptance.rs`, which is
run. Labels and roles are read from the components as they are written
(FirstRun, ActionLibrary, the feedback editor, ContextPanel, PluginsView,
ExportView, WorkspacePanel, ImportView and PlanSections).

From a data folder that never held LaunchDeck: the first run with
Development; a pad given Open surface from the Action Library; a feedback
rule on Auto Context; a context rule for an application (an xterm whose
`WM_CLASS` is Code) followed by Auto; the Counter example installed from
the Plugins view, its pad pressed; Development exported as a Pack with the
rule; the workspace reset in Settings and the plugin's folder taken away;
the Pack imported with its rule ticked, the dependency tagged Missing and
Auto Context left off; the Counter installed again and found by Rescan; the
imported surface shown by the rule and its pads working. Every step is a
new launch of the app, so the last one is the restart.
"""

import glob
import os
import re
import shutil
import time

NAME = "acceptance"
PLUGINS = []  # the Counter is installed from the Plugins view
TOOLS = ["openbox", "xterm", "wmctrl"]
WINDOW_MANAGER = True

CODE, OTHER = "Code", "Other"
COUNTER = "example.counter"
REST, ACTIVE = "#2f81f7", "#22c55e"
TERMINAL_PAD = (1, 2)  # column, row as drawn: Development's Terminal
KEYS_PAD = (5, 2)  # free on Development
COUNTER_PAD = (8, 8)  # free on Development


def setup(env):
    env.clients = [
        env.spawn(f"xterm-{cls.lower()}", ["xterm", "-class", cls, "-name", cls.lower(), "-geometry", "40x6", "-e", "sleep", "100000"], env={**os.environ, "DISPLAY": env.display})
        for cls in (CODE, OTHER)
    ]
    # Exports go to the Downloads folder the user-dirs file names: one under .work/,
    # emptied by the first step.
    env.downloads = os.path.join(env.work, "downloads")
    config = os.path.join(env.work, "config")
    os.makedirs(env.downloads, exist_ok=True)
    os.makedirs(config, exist_ok=True)
    with open(os.path.join(config, "user-dirs.dirs"), "w") as f:
        f.write(f'XDG_DOWNLOAD_DIR="{env.downloads}"\n')
    env.extra_env = {"XDG_CONFIG_HOME": config}
    time.sleep(1.0)


def teardown(env):
    for client in env.clients:
        env.stop(client)


# -- what a step does and looks at ------------------------------------------


def front(d, cls):
    d.env.x("wmctrl", "-x", "-a", f"{cls.lower()}.{cls}")
    time.sleep(0.6)


def launchdeck_front(d):
    d.env.x("wmctrl", "-x", "-a", "launchdeck")
    time.sleep(0.5)


def control(d):
    return d.js("return document.querySelector('[role=group][aria-label=\"Auto Context\"]')?.innerText || ''").replace("\n", " ")


def page_text(d):
    return d.js("return document.querySelector('.content')?.innerText || document.body.innerText")


def wait_for(d, what, condition, seconds=8):
    end = time.time() + seconds
    while time.time() < end:
        if condition():
            return True
        time.sleep(0.1)
    return d.check(what, False, "it did not happen in time")


def data_dir(d):
    return glob.glob(os.path.join(d.env.data, "*", "workspace.json"))[0].rsplit(os.sep, 1)[0]


def check_box(d, scope, text):
    """Tick the checkbox whose label, under `scope`, says `text`."""
    index = d.js(
        "return [...document.querySelectorAll(arguments[0] + ' input[type=checkbox]')]"
        ".findIndex(b => (b.closest('label')?.innerText || '').trim().startsWith(arguments[1]))",
        scope,
        text,
    )
    if index is None or index < 0:
        raise AssertionError(f"no checkbox says {text!r} under {scope}")
    box = d.find_all(f"{scope} input[type=checkbox]")[index]
    if not d.prop(box, "checked"):
        d.click(box)
        time.sleep(0.5)


def choose_param(d, label, value):
    """Pick a value in the inspector's field labelled `label`."""
    field = d.js("return [...document.querySelectorAll('.inspector label')].find(l => l.innerText.trim().startsWith(arguments[0]))?.htmlFor", label)
    d.choose(f".inspector select[id='{field}']", value)


def install_counter(d):
    d.sidebar("Plugins")
    d.click_css("button[aria-label='Install Counter Plugin']")
    wait_for(d, "the Counter to be installed", lambda: d.count("article.card.plugin") > 0 and "Counter" in d.plugin_card_text("Counter"), 15)


# -- steps ---------------------------------------------------------------------


def first_run(d):
    """A clean start shows the first run; Development is chosen and opens in Live."""
    shutil.rmtree(d.env.downloads, ignore_errors=True)
    os.makedirs(d.env.downloads)
    d.check("the first run is shown", "Welcome to LaunchDeck" in page_text(d) or d.count(".first-run") == 1)
    names = d.js("return [...document.querySelectorAll('label.card.purpose h3')].map(h => h.innerText.trim())")
    d.click(d.find_all("label.card.purpose input[type=radio]")[names.index("Development")])
    d.click_text("button", "Start with Development", pause=1.2)
    d.check("the first run is over", d.count(".first-run") == 0)
    d.check("Development is shown", "Development" in d.text_of(".content h1"), d.text_of(".content h1"))
    d.check("in Live", d.js("return document.querySelector('[role=tablist][aria-label=Mode] [aria-selected=true]')?.innerText.trim()") == "Live")
    d.check("the virtual Launchpad shows Development", d.led(*TERMINAL_PAD) not in ("", "transparent"), d.led(*TERMINAL_PAD))
    d.shot("first-run")


def library(d):
    """An empty pad gets Open surface from the Action Library; pressed, it opens Productivity."""
    d.sidebar("Development")
    d.mode("Edit")
    d.assign_action(*KEYS_PAD, "open surface")
    choose_param(d, "Surface", "productivity")
    d.save_control()
    d.check("the control is saved", "Not saved" not in d.inspector_text())
    d.mode("Live")
    d.click_pad(*KEYS_PAD, pause=1.0)
    d.check("the pad opened Productivity", "Productivity" in d.text_of(".content h1"), d.text_of(".content h1"))


def feedback(d):
    """A feedback rule on that pad: Active while Auto Context is on."""
    d.sidebar("Development")
    d.mode("Edit")
    d.click_pad(*KEYS_PAD, pause=0.8)
    d.click_css("button[data-part=add-feedback]")
    time.sleep(0.4)
    d.send(d.find("input[aria-label='Search states']"), "Auto Context")
    time.sleep(0.4)
    d.keys("Enter")
    time.sleep(0.6)
    d.check("a rule is added", d.count("[data-rule]") == 1, d.count("[data-rule]"))
    d.choose("select[aria-label='Rule 1 value']", "auto")
    d.create_look("select[aria-label='Rule 1 look']", "Active", ACTIVE)
    d.save_control()
    d.check("the control is saved", "Not saved" not in d.inspector_text())
    d.mode("Live")
    d.check("Auto Context is off: the pad rests", d.led(*KEYS_PAD) != ACTIVE, d.led(*KEYS_PAD))


def context(d):
    """A rule shows Development for Code; with Auto on, Code in front brings it back."""
    front(d, CODE)
    front(d, OTHER)
    launchdeck_front(d)
    d.sidebar("Settings")
    d.choose("section[aria-label='Auto Context'] select[aria-label='Application']", f"x11:{CODE}")
    d.choose("section[aria-label='Auto Context'] select[aria-label='Surface']", "development")
    d.click_text("section[aria-label='Auto Context'] button", "Add rule")
    d.click_css("section[aria-label='Auto Context'] [role=radio]")  # On
    wait_for(d, "Auto Context to be on", lambda: "On" in control(d))
    d.sidebar("Productivity")
    front(d, CODE)
    wait_for(d, "Code to show Development", lambda: "Development" in d.text_of(".content h1"))
    d.check("Code in front shows Development", "Development" in d.text_of(".content h1"), d.text_of(".content h1"))
    wait_for(d, "the pad's Auto look", lambda: d.led(*KEYS_PAD) == ACTIVE)
    d.shot("context")


def counter(d):
    """The Counter example installed from the Plugins view; its pad counts and shows its level."""
    install_counter(d)
    d.sidebar("Development")
    d.mode("Edit")
    d.assign_action(*COUNTER_PAD, "increment")
    d.click_css("button[aria-label='Add feedback from Counter level']")
    time.sleep(0.6)
    d.choose("select[aria-label='Rule 1 look']", "look:idle")
    d.click_text(".rule-block button", "Add a rule for each of the", pause=0.5, starts=True)
    d.create_look("select[aria-label='Rule 2 look']", "Active", ACTIVE)
    d.save_control()
    d.mode("Live")
    d.check("before the Counter reports, the pad rests", d.led(*COUNTER_PAD) == REST, d.led(*COUNTER_PAD))
    d.click_pad(*COUNTER_PAD)
    d.check("one press shows Active", d.led(*COUNTER_PAD) == ACTIVE, d.led(*COUNTER_PAD))


def export(d):
    """Development exported as a Pack with its rule, into Downloads."""
    d.click_text("button.side-link", "Export…", pause=1.0)
    check_box(d, ".artifacts", "Development")
    check_box(d, ".artifacts", "Include Auto Context rules")
    text = page_text(d)
    d.check("the Pack needs the Counter", "Plugins it needs" in text and "Counter" in text, text[-600:])
    d.click_text(".import-actions button", "Save", pause=1.5)
    saved = d.text_of(".import-actions [role=status]")
    found = re.search(r"Saved (\S+) in (.+)\.$", saved.strip().splitlines()[0]) if saved else None
    d.check("it says where it was saved", found is not None, saved)
    if found:
        path = os.path.join(found.group(2), found.group(1))
        d.check("the file is there, in the Downloads folder of this run", os.path.isfile(path) and found.group(2) == d.env.downloads, path)
    d.shot("export")


def reset(d):
    """Settings > Workspace resets to the starters; the plugin's folder is taken away and Rescan forgets it."""
    d.sidebar("Settings")
    d.click_text("button", "Reset to starter surfaces")
    d.click_text("button.danger", "Reset", pause=1.0)
    folder = os.path.join(data_dir(d), "plugins", COUNTER)
    d.check("the Counter was installed in the app's plugins folder", os.path.isdir(folder), folder)
    shutil.move(folder, os.path.join(d.env.aside, COUNTER))
    d.sidebar("Plugins")
    d.click_text("button", "Rescan", pause=1.2)
    d.check("the Counter is gone", d.count("button[aria-label='Install Counter Plugin']") == 1)
    d.check("Auto Context is off after the reset", "Off" in control(d), control(d))


def import_pack(d):
    """The Pack imported with its rule ticked: Counter Missing, Auto Context left off."""
    d.click_text("button.side-link", "Import…", pause=1.0)
    packs = sorted(glob.glob(os.path.join(d.env.downloads, "*.ldpack")), key=os.path.getmtime)
    d.check("the exported Pack is there", len(packs) == 1, packs)
    # A hidden file input takes a path from WebDriver: no file chooser is opened.
    d.send(d.find("input[type=file][aria-label='Pack or Surface file']"), packs[-1])
    wait_for(d, "the plan", lambda: "Plugins needed" in page_text(d), 10)
    text = page_text(d)
    d.check("the dependency is tagged Missing", "Missing" in text and "Not installed." in text, text[-800:])
    d.check("the surface says it needs the Counter", "Needs Counter" in text, text[:800])
    check_box(d, ".artifacts", f"{CODE} →")
    time.sleep(0.6)
    d.shot("plan")
    d.click_text(".import-actions button", "Import", pause=1.5)
    report = d.text_of(".artifacts [role=status]")
    d.check("one surface and one rule are added", "Added 1 surface and 1 rule." in report, report)
    d.check("the pads that need the Counter are named", "need a plugin" in report or "needs a plugin" in report, report)
    d.check("Auto Context stays off, and it says so", "Auto Context is off" in report and "Off" in control(d), (report, control(d)))


def recover(d):
    """The Counter installed again and found by Rescan; Auto on, Code shows the imported surface and its pads work."""
    install_counter(d)
    d.click_text("button", "Rescan", pause=1.0)
    d.sidebar("Settings")
    d.click_css("section[aria-label='Auto Context'] [role=radio]")  # On
    rules = d.js("return [...document.querySelectorAll('ol.context-rules select[aria-label=\"Rule 1 surface\"]')].map(s => s.value)")
    d.check("the imported rule shows the imported surface", rules == ["development-2"], rules)
    d.sidebar("Productivity")
    front(d, OTHER)
    front(d, CODE)
    wait_for(d, "the imported surface", lambda: d.led(*COUNTER_PAD) == REST)
    d.click_pad(*COUNTER_PAD)
    d.check("the Counter pad works again", d.led(*COUNTER_PAD) == ACTIVE, d.led(*COUNTER_PAD))
    d.check("the Auto look is on the Library pad", d.led(*KEYS_PAD) == ACTIVE, d.led(*KEYS_PAD))
    d.shot("recovered")


def restart(d):
    """After a restart: no first run, the rule, Auto on, the Counter and its pad as they were."""
    d.check("no first run", d.count(".first-run") == 0)
    d.sidebar("Settings")
    d.check("Auto Context is on", "On" in control(d) and "Off" not in control(d), control(d))
    listed = d.js("return [...document.querySelectorAll('ol.context-rules > li')].map(li => li.getAttribute('aria-label'))")
    d.check("with the imported rule", listed == [f"Rule 1: {CODE}"], listed)
    d.sidebar("Plugins")
    d.check("the Counter is installed", "Counter" in d.plugin_card_text("Counter"))
    d.sidebar("Productivity")
    front(d, OTHER)
    front(d, CODE)
    wait_for(d, "the imported surface", lambda: d.led(*COUNTER_PAD) == REST)
    d.click_pad(*COUNTER_PAD)
    d.check("its pad works", d.led(*COUNTER_PAD) == ACTIVE, d.led(*COUNTER_PAD))
    d.shot("restart")


STEPS = [first_run, library, feedback, context, counter, export, reset, import_pack, recover, restart]
