"""The feedback editor, from an empty pad to a plugin that is taken away.

The steps build on each other: `authoring` saves the Counter pad the later
steps press, `scoped` saves the Channels pads `missing_provider` looks at.
"""

import time

NAME = "feedback"
PLUGINS = ["counter", "channels"]

SURFACE = "Development"
REST, ACTIVE, WARNING, ERROR = "#2f81f7", "#22c55e", "#f59e0b", "#ef4444"
COUNTER_PAD = (8, 8)  # column, row of the grid as drawn
CHANNEL_PAD = (7, 8)
CHANNEL_DOWN_PAD = (6, 8)
KEYBOARD_PAD = (4, 8)


def explain(d, pad):
    """Select a pad in Edit mode and read its Live feedback panel."""
    d.mode("Edit")
    d.click_pad(*pad, pause=1.0)
    return d.live_panel()


def authoring(d):
    """An empty pad gets Counter / Increment and four rules on Counter level, and is saved."""
    d.sidebar(SURFACE)
    d.mode("Edit")
    d.click_pad(*COUNTER_PAD)
    d.check("an empty pad opens as a new control", "New control" in d.inspector_text())
    d.assign_action(*COUNTER_PAD, "increment")
    text = d.inspector_text()
    d.check("the Inspector has an Action section with Increment and a separate Feedback section", "Increment" in text and "FEEDBACK" in text)
    d.check("a pad without feedback says so and offers + Add feedback", "No feedback yet" in text and "+ Add feedback" in text)
    d.check("Counter level is suggested and nothing is added by itself", "Suggested" in text and "Counter level" in text and d.count("[data-rule]") == 0)

    d.click_css("button[data-part=add-feedback]")
    time.sleep(0.4)
    d.send(d.find("input[aria-label='Search states']"), "level")
    time.sleep(0.4)
    picker = d.text_of(".picker")
    d.check("the picker finds Counter level by a word", "Counter level" in picker, picker[:200])
    d.check("the picker shows no technical id", "example.counter" not in picker, picker[:200])
    d.shot("picker")
    d.click_text(".flow button", "Cancel", pause=0.3)

    d.click_css("button[aria-label='Add feedback from Counter level']")
    time.sleep(0.6)
    d.check("the suggestion adds a first rule", d.count("[data-rule]") == 1)
    d.check("the cursor is in the new rule", d.js("return !!document.activeElement.closest('[data-rule]')"))

    d.choose("select[aria-label='Rule 1 look']", "look:idle")
    d.click_text(".rule-block button", "Add a rule for each of the", pause=0.5, starts=True)
    d.check("one click adds a rule for each other value", d.count("[data-rule]") == 4, d.count("[data-rule]"))
    values = [d.value(f"select[aria-label='Rule {i} value']") for i in range(1, 5)]
    d.check("the rules cover zero, low, high and max, in that order", values == ["zero", "low", "high", "max"], values)

    d.create_look("select[aria-label='Rule 2 look']", "Active", ACTIVE)
    d.create_look("select[aria-label='Rule 3 look']", "Warning", WARNING)
    d.create_look("select[aria-label='Rule 4 look']", "Error", ERROR)
    looks = [d.value(f"select[aria-label='Rule {i} look']") for i in range(1, 5)]
    d.check("looks made in place are shown by their rules", looks == ["look:idle", "look:active", "look:warning", "look:error"], looks)
    d.shot("rules")
    text = d.inspector_text()
    d.check("the panel explains the draft, and the state is Unknown", "Unknown" in text and "Draft" in text)
    d.save_control()
    d.check("the control is saved", "Not saved" not in d.inspector_text())


def live(d):
    """Pressing the pad moves the virtual Launchpad; the plugin is turned off and on."""
    d.sidebar(SURFACE)
    d.mode("Live")
    lamp = lambda: d.led(*COUNTER_PAD)  # noqa: E731
    d.check("before the Counter reports, the pad rests", lamp() == REST, lamp())
    seen = []
    for _ in range(10):
        d.click_pad(*COUNTER_PAD)
        seen.append(lamp())
    d.check("presses 1-4 show Active, 5-8 Warning, 9 Error, and the 10th is refused", seen == [ACTIVE] * 4 + [WARNING] * 4 + [ERROR] * 2, seen)
    d.shot("full")

    d.sidebar("Productivity")
    d.sidebar(SURFACE)
    d.check("away and back, the pad shows the current look", lamp() == ERROR, lamp())

    d.sidebar("Plugins")
    d.plugin_button("Counter", "Disable")
    d.sidebar(SURFACE)
    d.check("with the plugin off, the pad rests", lamp() == REST, lamp())
    panel = explain(d, COUNTER_PAD)
    d.check("the panel says Unavailable and why", "Unavailable" in panel and "turned off" in panel.lower(), panel[:200])
    d.shot("unavailable")

    d.sidebar("Plugins")
    d.plugin_button("Counter", "Enable", pause=1.2)
    d.sidebar(SURFACE)
    panel = explain(d, COUNTER_PAD)
    d.check("turned on again, the state is Unknown and not the old value", "Unknown" in panel and "Known" not in panel.replace("Unknown", ""), panel[:200])
    d.mode("Live")
    d.check("the pad rests while the state is unknown", lamp() == REST, lamp())
    d.click_pad(*COUNTER_PAD)
    d.check("one press later the state is known and the look is Active", lamp() == ACTIVE, lamp())
    panel = explain(d, COUNTER_PAD)
    d.check("the panel says Known, decided by rule 2", "Known" in panel and "Rule 2" in panel, panel[:240])


def event(d):
    """An event rule shows a flashing look over the state look for 0.7 seconds."""
    d.sidebar(SURFACE)
    d.mode("Edit")
    d.click_pad(*COUNTER_PAD, pause=0.6)
    d.click_css("button[data-part=add-feedback]", -1)
    time.sleep(0.4)
    d.click_text(".flow .chip", "An event", pause=0.3)
    d.send(d.find("input[aria-label='Search events']"), "counter changed")
    time.sleep(0.4)
    d.check("the picker lists the event Counter changed", "Counter changed" in d.text_of(".picker"))
    d.keys("Enter")
    time.sleep(0.6)
    d.check("an event rule is added", d.count("li[data-kind=event]") == 1)
    d.choose("select[data-part=add-condition]", "level", verify=False)
    d.choose("select[aria-label='Event rule 1 condition: Level']", "high")
    d.create_look("select[aria-label='Event rule 1 look']", "Alarm", ERROR, "flash")
    d.replace(d.find("input[aria-label='Event rule 1 seconds']"), "0.7")
    time.sleep(0.3)
    d.check("the rule says in words what it will do", "Shown for 0.7 seconds" in d.text_of("li[data-kind=event]"))
    d.shot("rule")
    d.save_control()
    d.check("the control is saved", "Not saved" not in d.inspector_text())

    d.mode("Live")
    for _ in range(4):
        d.click_pad(*COUNTER_PAD)
    d.check("below High only the state shows", d.led(*COUNTER_PAD) == ACTIVE, d.led(*COUNTER_PAD))
    d.sample_led(*COUNTER_PAD)
    d.click_pad(*COUNTER_PAD, pause=1.8)
    runs = d.sampled()
    d.note(f"LED over time: {runs}")
    colours = [r[0] for r in runs]
    d.check("the pad goes Active, Alarm, Warning", colours[:1] == [ACTIVE] and ERROR in colours and colours[-1:] == [WARNING], colours)
    alarm = next((r for r in runs if r[0] == ERROR), None)
    d.check("the temporary look lasts about 0.7 seconds", alarm and 450 <= alarm[2] - alarm[1] <= 1000, alarm)
    d.check("the temporary look flashes", alarm and alarm[3])
    panel = explain(d, COUNTER_PAD)
    d.check("afterwards the state rule still decides: High, rule 3", "High" in panel and "Rule 3" in panel, panel[:300])


def overlay_explained(d):
    """While an event's look is on the pad, the panel names it and says how long is left."""
    d.sidebar(SURFACE)
    d.mode("Edit")
    d.click_pad(*COUNTER_PAD, pause=0.6)
    d.replace(d.find("input[aria-label='Event rule 1 seconds']"), "20")
    time.sleep(0.3)
    d.save_control()
    d.mode("Live")
    for _ in range(5):
        d.click_pad(*COUNTER_PAD)
    d.check("the event's look is on the pad", d.led(*COUNTER_PAD) == ERROR, d.led(*COUNTER_PAD))
    panel = explain(d, COUNTER_PAD)
    d.note(panel.replace("\n", " | ")[:400])
    d.check("the panel names the temporary look and the event rule", "Alarm" in panel and "event rule 1" in panel, panel[:300])
    d.check("the panel says how long is left", "seconds left" in panel, panel[:300])
    d.check("the panel still says what the state rules give", "Warning" in panel and "Rule 3" in panel, panel[:300])
    d.shot("overlay")
    d.replace(d.find("input[aria-label='Event rule 1 seconds']"), "0.7")
    time.sleep(0.3)
    d.save_control()


def scoped(d):
    """A state that exists once per channel: scope, order by button and by Alt+arrows, and Live."""
    d.sidebar(SURFACE)
    d.mode("Edit")

    def channel_pad(pad, query, label):
        d.assign_action(*pad, query)
        d.replace(d.find_all(".inspector input")[0], label)
        field = d.js("const l = [...document.querySelectorAll('.inspector label')].find(l => l.innerText.startsWith('Channel')); return l.htmlFor")
        d.send(d.find(f"[id='{field}']"), "alpha")
        time.sleep(0.3)

    channel_pad(CHANNEL_PAD, "channel up", "Alpha up")
    d.click_text("button[data-part=add-feedback]", "+ Add feedback")
    d.send(d.find("input[aria-label='Search states']"), "channel")
    time.sleep(0.4)
    picker = d.text_of(".picker")
    d.check("the picker finds Channel status and says it is per channel", "Channel status" in picker and "Per channel" in picker, picker[:200])
    d.keys("Enter")
    time.sleep(0.5)
    d.shot("scope")
    d.send(d.find(".flow input"), "alpha")
    d.click_text(".flow button", "Add the first rule")
    d.check("the first rule is for channel alpha", d.count("li[data-kind=state]") == 1)
    header = d.text_of(".rule-run")
    d.check("the header names the instance in words and no id", "channel alpha" in header and "example.channels" not in header, header[:160])

    d.create_look("select[aria-label='Rule 1 look']", "Online", ACTIVE)
    d.click_text("button", "+ Add a rule")
    d.check("+ Add a rule adds the next value", d.count("li[data-kind=state]") == 2 and d.value("select[aria-label='Rule 2 value']") == "down")
    d.create_look("select[aria-label='Rule 2 look']", "Offline", ERROR, "pulse")
    d.click_text("button", "+ Add a rule")
    d.choose("select[aria-label='Rule 3 condition']", "hasValue")
    d.create_look("select[aria-label='Rule 3 look']", "Seen", "#3b82f6")
    d.check("nothing is marked as never used while the general rule is last", "Never used" not in d.inspector_text())

    d.click_css("button[aria-label='Move rule 3 up']")
    time.sleep(0.4)
    d.check("moving up says where the rule went", "now rule 2 of 3" in d.rule_status(), d.rule_status())
    d.click_css("button[aria-label='Move rule 2 up']")
    time.sleep(1.2)
    d.check("first of three", "now rule 1 of 3" in d.rule_status(), d.rule_status())
    d.check("Rust marks the two rules it can prove are never used", d.inspector_text().count("Never used") == 2, d.inspector_text().count("Never used"))
    d.check("the cursor stays with the rule that moved", d.js("return !!document.activeElement.closest('[data-rule=\"0\"]')"))
    d.shot("never-used")
    d.js("document.querySelector('[data-rule=\"0\"] [data-part=down]').focus()")
    d.keys(("Alt", "Down"))
    time.sleep(0.4)
    d.check("Alt+Down moves the rule down", "now rule 2 of 3" in d.rule_status(), d.rule_status())
    d.keys(("Alt", "Down"))
    time.sleep(0.4)
    d.check("and down again, to the end", "now rule 3 of 3" in d.rule_status(), d.rule_status())
    d.keys(("Alt", "Down"))
    time.sleep(1.2)
    d.check("at the end it says so and moves nothing", "already the last" in d.rule_status(), d.rule_status())
    d.check("the marks are gone with the order restored", "Never used" not in d.inspector_text())
    order = [d.value(f"select[aria-label='Rule {i} condition']") for i in (1, 2, 3)]
    d.check("the order is Up, Down, has a value again", order == ["equals", "equals", "hasValue"], order)

    def change_channel(name):
        d.click_text("button", "Change channel…")
        d.replace(d.find(".flow input"), name)
        d.click_text(".flow button", "Use this", pause=0.6)

    change_channel("beta")
    d.check("Change channel… moves all three rules to beta", d.count("li[data-kind=state]") == 3 and "channel beta" in d.text_of(".rule-run"), d.text_of(".rule-run")[:120])
    change_channel("alpha")
    d.check("and back to alpha", "channel alpha" in d.text_of(".rule-run"), d.text_of(".rule-run")[:120])
    d.save_control()
    d.check("the scoped pad is saved", "Not saved" not in d.inspector_text())

    channel_pad(CHANNEL_DOWN_PAD, "channel down", "Alpha down")
    d.save_control()
    d.check("the pad that reports the channel down is saved", "Not saved" not in d.inspector_text())

    d.mode("Live")
    lamp = lambda: d.led(*CHANNEL_PAD)  # noqa: E731
    d.check("before any report the scoped pad rests", lamp() == "#3b82f6", lamp())
    d.click_pad(*CHANNEL_PAD, pause=0.6)
    d.check("channel alpha up shows Online", lamp() == ACTIVE, lamp())
    d.click_pad(*CHANNEL_DOWN_PAD, pause=0.6)
    d.check("channel alpha down shows Offline", lamp() == ERROR, lamp())
    d.click_pad(*CHANNEL_PAD, pause=0.6)
    d.check("up again shows Online", lamp() == ACTIVE, lamp())
    d.shot("live")


def test_run(d):
    """The panel follows a state that changes while it is open, without being asked."""
    d.sidebar(SURFACE)
    before = explain(d, COUNTER_PAD)
    d.check("Counter level starts Unknown", "Unknown" in before, before[:120])
    d.click_text(".inspector button", "Test run", pause=1.0)
    after = d.live_panel()
    d.check("after Test run the panel says Known, Low, rule 2, by itself", "Known" in after and "Low" in after and "Rule 2" in after, after[:200])


def keyboard(d):
    """The same authoring with the keyboard only."""
    d.sidebar(SURFACE)
    d.mode("Live")
    d.js("document.activeElement && document.activeElement.blur()")
    presses = d.tab_until("e.getAttribute('role') === 'tab' && e.innerText.trim() === 'Edit'")
    d.keys("Enter")
    time.sleep(0.8)
    d.check(f"Tab reaches the Edit tab ({presses} presses) and Enter switches mode", "Editing." in d.text_of(".stage-banner"))

    presses = d.tab_until("e.classList.contains('pad')", limit=40)
    d.check(f"Tab reaches the grid ({presses} presses)", True)
    column, row = KEYBOARD_PAD
    moves = 0
    while moves < 40 and not d.js("const e = document.activeElement; return e.style.gridColumnStart == String(arguments[0]) && e.style.gridRowStart == String(arguments[1])", column, row):
        at = d.js("const e = document.activeElement; return [Number(e.style.gridColumnStart), Number(e.style.gridRowStart)]")
        d.keys("Right" if at[0] < column else "Left" if at[0] > column else "Down" if at[1] < row else "Up")
        moves += 1
    d.check(f"the arrow keys reach the pad ({moves} presses)", moves < 40 and d.js("return document.activeElement.classList.contains('pad')"))
    d.keys("Enter")
    time.sleep(0.5)
    d.check("Enter selects the pad", "New control" in d.inspector_text())

    d.typing("/")
    time.sleep(0.2)
    d.check("/ puts the cursor in Search actions", (d.active() or {}).get("label") == "Search actions", d.active())
    d.typing("increment")
    time.sleep(0.4)
    d.keys("Enter")
    time.sleep(0.8)
    d.check("Enter gives the pad the action found", "Increment" in d.inspector_text())

    presses = d.tab_until("e.getAttribute('aria-label') === 'Add feedback from Counter level'", limit=70)
    d.check(f"Tab reaches the Inspector's suggestion in few presses ({presses})", presses <= 60, presses)
    d.keys("Enter")
    time.sleep(0.6)
    d.check("the cursor lands in the new rule's first field", (d.active() or {}).get("label") == "Rule 1 condition", d.active())
    d.keys("Tab")
    d.keys("Tab")
    d.check("Tab, Tab reaches the rule's look", (d.active() or {}).get("label") == "Rule 1 look", d.active())
    d.keys("Down")
    time.sleep(0.3)
    d.check("Down chooses a look", d.value("select[aria-label='Rule 1 look']") == "look:idle", d.value("select[aria-label='Rule 1 look']"))
    d.check("the cursor is still on the look field", (d.active() or {}).get("label") == "Rule 1 look", d.active())

    d.tab_until("e.innerText.trim() === '+ Add a rule'")
    d.keys("Enter")
    time.sleep(0.5)
    d.check("+ Add a rule puts the cursor in the new rule", (d.active() or {}).get("label") == "Rule 2 condition", d.active())
    d.keys("Tab")
    before = d.value("select[aria-label='Rule 2 value']")
    d.keys("Down")
    time.sleep(0.3)
    d.check("Down changes the value", d.value("select[aria-label='Rule 2 value']") != before)
    d.keys("Tab")
    for _ in range(12):
        if d.count(".new-look"):
            break
        d.keys("Down")
        time.sleep(0.25)
    d.check("Down reaches + Create a new look… and the form opens with the cursor in it", d.js("return !!document.activeElement.closest('.new-look')"), d.active())
    d.typing("Okay")
    d.keys("Enter")
    time.sleep(0.6)
    d.check("Enter makes the look and the rule shows it", d.value("select[aria-label='Rule 2 look']") == "look:okay", d.value("select[aria-label='Rule 2 look']"))

    d.keys(("Alt", "Up"))
    time.sleep(0.4)
    d.check("Alt+Up from the look field moves the rule and says so", "now rule 1 of 2" in d.rule_status(), d.rule_status())
    d.keys(("Alt", "Down"))
    time.sleep(0.4)
    d.tab_until("e.innerText.trim() === 'Add control'")
    d.keys("Enter")
    time.sleep(1.0)
    d.check("Enter on Add control saves", "Not saved" not in d.inspector_text())

    d.keys(("Shift", "Tab"))
    time.sleep(0.2)
    ring = d.js("const e = document.activeElement, s = getComputedStyle(e); return [e.matches(':focus-visible'), s.outlineStyle, s.boxShadow]")
    d.shot("focus")
    d.check("the control that has the focus shows it", ring[0] and (ring[1] != "none" or ring[2] != "none"), ring)


def missing_provider(d):
    """Rules survive their plugin being turned off and being taken away."""
    d.sidebar(SURFACE)
    explain(d, CHANNEL_PAD)
    d.check("the scoped pad has its three rules", d.count("li[data-kind=state]") == 3)

    d.sidebar("Plugins")
    d.plugin_button("Channels", "Disable")
    d.sidebar(SURFACE)
    explain(d, CHANNEL_PAD)
    text = d.inspector_text()
    d.check("plugin off: the rules stay and say Unavailable with the reason", d.count("li[data-kind=state]") == 3 and "Unavailable" in text and "turned off" in text.lower())
    d.shot("off")
    d.sidebar("Plugins")
    d.plugin_button("Channels", "Enable")

    def rescan():
        d.sidebar("Plugins")
        d.click_text("button", "Rescan", pause=1.5)

    d.env.set_plugin_aside("channels", True)
    try:
        rescan()
        d.sidebar(SURFACE)
        explain(d, CHANNEL_PAD)
        text = d.inspector_text()
        d.check("plugin gone: the rules are kept and say the state is no longer offered", d.count("li[data-kind=state]") == 3 and "no longer offered" in text)
        buttons = d.js("return [...document.querySelectorAll('.inspector button')].map(b => b.innerText.trim())")
        d.check("Replace state… and Remove these rules are offered", "Replace state…" in buttons and "Remove these rules" in buttons)
        d.shot("gone")
        d.click_text(".inspector button", "Remove these rules", pause=0.5)
        d.check("Remove these rules empties the draft and says how many", d.count("li[data-kind=state]") == 0 and "Removed 3 rules" in d.inspector_text())
        d.click_text(".inspector button", "Revert", pause=0.5)
        d.check("Revert brings them back", d.count("li[data-kind=state]") == 3)
    finally:
        d.env.set_plugin_aside("channels", False)
    rescan()
    d.sidebar(SURFACE)
    explain(d, CHANNEL_PAD)
    d.check("plugin back: the rules read it again", "no longer offered" not in d.inspector_text())


STEPS = [authoring, live, event, overlay_explained, scoped, test_run, keyboard, missing_provider]
