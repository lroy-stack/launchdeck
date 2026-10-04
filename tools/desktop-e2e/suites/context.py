"""Auto Context: the surface follows the application in front.

Real X clients (three xterms with their own `WM_CLASS`), a real window manager
(openbox) and LaunchDeck's real X11 observer. A step brings an application to
the front the way a person's click would end up doing it, by asking the window
manager to activate its window, and then looks at what LaunchDeck shows.

The first step runs **without** a window manager: nothing reports the active
window, so the observer is unavailable, and LaunchDeck has to say so and work
by hand as always.
"""

import glob
import json
import os
import time

NAME = "context"
PLUGINS = []
TOOLS = ["openbox", "xterm", "wmctrl"]
WINDOW_MANAGER = True

# Three applications: two will get a rule, one will not.
ALPHA, BETA, GAMMA = "Alpha", "Beta", "Gamma"


def setup(env):
    env.clients = [
        env.spawn(f"xterm-{cls.lower()}", ["xterm", "-class", cls, "-name", cls.lower(), "-geometry", "40x6", "-e", "sleep", "100000"], env={**os.environ, "DISPLAY": env.display})
        for cls in (ALPHA, BETA, GAMMA)
    ]
    time.sleep(1.0)


def teardown(env):
    for client in env.clients:
        env.stop(client)


# -- what a step does and looks at ------------------------------------------


def front(d, cls, stay=0.0):
    """Bring an application's window to the front, as the window manager does on a click.

    `stay`: how long it then stays there before the step goes on. An application
    counts once it has been in front for 150 ms; a step that wants it to count
    before doing something else has to leave it there that long, as a person would.
    """
    d.env.x("wmctrl", "-x", "-a", f"{cls.lower()}.{cls}")
    if stay:
        time.sleep(stay)


def launchdeck_front(d):
    d.env.x("wmctrl", "-x", "-a", "launchdeck")
    time.sleep(0.5)


def shown(d):
    """The surface on screen, by the name in the Surfaces view's heading."""
    return (d.js("return document.querySelector('.content h1')?.innerText || ''") or "").strip()


def control(d):
    return d.js("return document.querySelector('[role=group][aria-label=\"Auto Context\"]')?.innerText || ''").replace("\n", " ")


def panel(d):
    return d.js("return document.querySelector('section[aria-label=\"Auto Context\"]')?.innerText || ''")


def now(d):
    return d.js("return document.querySelector('[role=group][aria-label=\"Context now\"]')?.innerText || ''").replace("\n", " ")


def leds(d):
    return d.js("return [...document.querySelectorAll('.pad')].map(p => p.style.getPropertyValue('--led')).join(',')")


def wait_for(d, what, condition, seconds=6):
    end = time.time() + seconds
    while time.time() < end:
        if condition():
            return True
        time.sleep(0.1)
    return d.check(what, False, "it did not happen in time")


def until_shown(d, surface, seconds=6):
    """Wait for a surface and return how long it took, in milliseconds."""
    start = time.time()
    while time.time() - start < seconds:
        if shown(d) == surface:
            return int((time.time() - start) * 1000)
        time.sleep(0.02)
    return None


def workspace(d):
    return json.load(open(glob.glob(os.path.join(d.env.data, "*", "workspace.json"))[0]))


def written(d):
    """Everything LaunchDeck wrote in its data folder, as one text."""
    out = ""
    for folder, _, names in os.walk(d.env.data):
        for name in names:
            try:
                out += open(os.path.join(folder, name), errors="replace").read()
            except OSError:
                pass
    return out


def choose(d, label, value):
    d.choose(f"section[aria-label='Auto Context'] select[aria-label='{label}']", value)


def add_rule(d, cls, surface):
    choose(d, "Application", f"x11:{cls}")
    choose(d, "Surface", surface)
    d.click_text("section[aria-label='Auto Context'] button", "Add rule")
    time.sleep(0.5)


# -- steps ---------------------------------------------------------------------


def unavailable(d):
    """Where nothing reports the active window, LaunchDeck says so and works by hand as always."""
    d.sidebar("Settings")
    text = now(d)
    d.check("the observer is said to be Not available, with the reason", "Not available" in text and "no window manager that reports the active window" in text, text[:300])
    d.check("the application in front is said to be unknown here", "Not known on this system." in text, text[:400])
    d.check("the sidebar shows Auto Context, off", "Auto Context" in control(d) and "Off" in control(d), control(d))

    d.click_css("[role=group][aria-label='Auto Context'] [role=switch]")
    wait_for(d, "the control to say it is not available here", lambda: "Not available here" in control(d))
    d.check("switched on, it says it cannot work here instead of pretending", "Not available here" in control(d) and "Auto Context is on and cannot work here." in now(d), control(d))
    d.check("no rule can be written from an application it cannot see, and it says why", "which it cannot do on this system" in panel(d))
    d.shot("unavailable")

    # Everything by hand is exactly as before.
    for surface in ("Development", "Productivity"):
        d.sidebar(surface)
        d.check(f"{surface} opens from the sidebar", shown(d) == surface, shown(d))
    d.mode("Edit")
    d.mode("Live")
    d.check("and editing and going back to Live changes nothing by itself", shown(d) == "Productivity", shown(d))
    d.check("it never reads Manual override: there is nothing to override", "Manual override" not in control(d), control(d))
    d.click_css("[role=group][aria-label='Auto Context'] [role=switch]")
    wait_for(d, "Auto Context to be off again", lambda: "Off" in control(d))


unavailable.window_manager = False


def rules(d):
    """With a window manager the observer works; rules are written by picking applications it has seen."""
    d.sidebar("Settings")
    wait_for(d, "the observer to be available", lambda: "Available" in now(d))
    text = now(d)
    d.check("the observer is Available and says how it watches", "Watching through the X11 window manager." in text, text[:200])
    d.check("LaunchDeck in front is said not to count as an application", "LaunchDeck itself, which never counts as an application." in text, text[:400])
    d.note(f"windows on the display: {d.env.x('wmctrl', '-lx').strip().splitlines()}")

    # The three applications come to the front once each; then LaunchDeck again.
    for cls in (ALPHA, BETA, GAMMA):
        front(d, cls)
        time.sleep(0.6)
    launchdeck_front(d)
    wait_for(d, "the applications to be offered", lambda: d.count("section[aria-label='Auto Context'] select[aria-label='Application'] option") == 3)
    offered = d.js("return [...document.querySelectorAll(\"section[aria-label='Auto Context'] select[aria-label='Application'] option\")].map(o => o.textContent)")
    d.check("the applications seen in front are offered, newest first, by name and stable identity", offered == ["Gamma (x11:Gamma)", "Beta (x11:Beta)", "Alpha (x11:Alpha)"], offered)
    d.check("LaunchDeck itself is not among them", not any("aunch" in o for o in offered), offered)
    d.check("the last application is the one rules are asked about", "last application" in now(d).lower() and "x11:Gamma" in now(d), now(d)[:400])

    add_rule(d, ALPHA, "development")
    add_rule(d, BETA, "productivity")
    listed = d.js("return [...document.querySelectorAll('ol.context-rules > li')].map(li => li.getAttribute('aria-label'))")
    d.check("two rules, numbered in the order they are tried", listed == ["Rule 1: Alpha", "Rule 2: Beta"], listed)
    saved = workspace(d).get("context", {})
    d.check("the rules are saved by stable identity, not by name alone", [r["application"] for r in saved.get("rules", [])] == [{"namespace": "x11", "id": "Alpha", "displayName": "Alpha"}, {"namespace": "x11", "id": "Beta", "displayName": "Beta"}], saved)
    d.check("the fallback is explicit: keep the current surface", d.value("select[aria-label='When no rule matches']") == "" and "An application without a rule changes nothing." in panel(d))
    d.check("nothing has switched yet: Auto Context is off", "Off" in control(d), control(d))
    d.shot("rules")

    d.click_css("section[aria-label='Auto Context'] [role=radio]")  # On
    wait_for(d, "Auto Context to be on", lambda: workspace(d).get("context", {}).get("auto") is True)
    d.check("switching it on is saved", workspace(d)["context"]["auto"] is True)
    d.check("and the sidebar says On", "On" in control(d) and "Off" not in control(d), control(d))


def switching(d):
    """App A shows surface A, app B shows surface B, an application without a rule changes nothing."""
    d.sidebar("Surfaces")
    took = {}
    front(d, ALPHA)
    took[ALPHA] = until_shown(d, "Development")
    d.check("Alpha in front shows Development", shown(d) == "Development", shown(d))
    development = leds(d)
    front(d, BETA)
    took[BETA] = until_shown(d, "Productivity")
    d.check("Beta in front shows Productivity", shown(d) == "Productivity", shown(d))
    d.check("the virtual Launchpad shows another surface's pads", leds(d) != development and leds(d) != "")
    d.note(f"from asking the window manager to the surface on screen: {took} ms (includes the 150 ms settle and this script's polling)")
    d.check("each switch took well under a second", all(t is not None and t < 1000 for t in took.values()), took)
    d.shot("beta")

    # An application without a rule: the fallback is to keep.
    front(d, GAMMA)
    time.sleep(1.0)
    d.check("Gamma has no rule, and the surface stays", shown(d) == "Productivity", shown(d))
    # The same application reported again changes nothing.
    front(d, BETA)
    time.sleep(0.5)
    front(d, BETA)
    time.sleep(0.5)
    d.check("Beta again is still Productivity", shown(d) == "Productivity", shown(d))

    front(d, GAMMA)
    time.sleep(0.6)
    launchdeck_front(d)
    d.sidebar("Settings")
    text = now(d)
    d.check("Settings says why: no rule names Gamma, and the fallback keeps the surface", "No rule names Gamma, and the fallback is to keep the current surface." in text, text[-300:])
    front(d, ALPHA)
    wait_for(d, "the decision for Alpha", lambda: "Rule 1 matches Alpha" in now(d))
    d.check("and for Alpha: the rule that matched and what it did", "Rule 1 matches Alpha: Development was activated." in now(d) and "x11:Alpha" in now(d), now(d)[-300:])
    marked = d.js("return [...document.querySelectorAll('ol.context-rules > li')].map(li => li.className.includes('matched'))")
    d.check("the rule that decided is marked in the list", marked == [True, False], marked)
    d.shot("decision")


def override(d):
    """A surface chosen by hand is not switched back until another application comes to the front or Resume Auto is pressed."""
    d.sidebar("Surfaces")
    front(d, ALPHA, stay=0.6)
    until_shown(d, "Development")
    launchdeck_front(d)
    time.sleep(0.6)
    d.check("LaunchDeck coming to the front changes nothing", shown(d) == "Development", shown(d))

    d.sidebar("Smart Home")
    d.check("a surface picked by hand is shown", shown(d) == "Smart Home", shown(d))
    wait_for(d, "the control to say Manual override", lambda: "Manual override" in control(d))
    d.check("the sidebar says Manual override and offers Resume Auto", "Manual override" in control(d) and "Resume Auto" in control(d), control(d))
    d.shot("override")

    # Back to the same application, and time passes.
    front(d, ALPHA)
    time.sleep(1.5)
    d.check("back in Alpha, it is not switched back", shown(d) == "Smart Home", shown(d))
    d.check("and it still says Manual override", "Manual override" in control(d), control(d))

    launchdeck_front(d)
    d.click_text("[role=group][aria-label='Auto Context'] button", "Resume Auto")
    took = until_shown(d, "Development")
    d.check("Resume Auto brings Alpha's surface back at once", shown(d) == "Development" and took is not None and took < 1000, (shown(d), took))
    wait_for(d, "the control to say On", lambda: "Manual override" not in control(d))
    d.check("and the sidebar says On again", "On" in control(d) and "Resume Auto" not in control(d), control(d))

    # Override again; this time a different application comes to the front.
    d.sidebar("Smart Home")
    wait_for(d, "Manual override", lambda: "Manual override" in control(d))
    front(d, BETA)
    until_shown(d, "Productivity")
    d.check("a different application ends the override and its rule applies", shown(d) == "Productivity" and "Manual override" not in control(d), (shown(d), control(d)))


def edit_mode(d):
    """Editing suspends automatic switching; going back to Live applies the current context once."""
    d.sidebar("Surfaces")
    front(d, ALPHA, stay=0.6)
    until_shown(d, "Development")
    launchdeck_front(d)
    d.mode("Edit")
    wait_for(d, "the control to say Paused", lambda: "Paused while editing" in control(d))
    d.check("in Edit the sidebar says Paused while editing", "Paused while editing" in control(d), control(d))
    d.sidebar("Accessibility")
    d.check("choosing what to edit is not an override", shown(d) == "Accessibility" and "Manual override" not in control(d), (shown(d), control(d)))

    front(d, BETA)
    time.sleep(1.5)
    d.check("another application comes to the front and nothing is switched", shown(d) == "Accessibility", shown(d))
    launchdeck_front(d)
    d.sidebar("Settings")
    text = now(d)
    d.check("the context kept up, and says what it would do", "x11:Beta" in text and "Rule 2 matches Beta: it calls for Productivity." in text and "Not applied while editing." in text, text[-400:])
    d.shot("paused")

    d.sidebar("Surfaces")
    d.check("still the surface being edited", shown(d) == "Accessibility", shown(d))
    d.mode("Live")
    took = until_shown(d, "Productivity")
    d.check("back in Live, Beta's surface is applied", shown(d) == "Productivity" and took is not None, (shown(d), took))
    time.sleep(1.0)
    d.check("once: it stays there", shown(d) == "Productivity" and "On" in control(d), (shown(d), control(d)))


def restart(d):
    """After a restart the rules and the choice of Auto are there, and nothing else was kept."""
    saved = workspace(d)["context"]
    d.check("the rules and Auto are in the workspace", saved["auto"] is True and [r["surfaceId"] for r in saved["rules"]] == ["development", "productivity"], saved)
    d.check("and the fallback as it was chosen", saved["fallback"] == {"kind": "keep"}, saved["fallback"])
    text = written(d)
    d.check("Gamma was in front several times and is in no file: no history is kept", "Gamma" not in text)
    d.check("Alpha and Beta appear only as the rules that name them", text.count('"Alpha"') == 2 and text.count('"Beta"') == 2, (text.count('"Alpha"'), text.count('"Beta"')))

    d.sidebar("Settings")
    wait_for(d, "the observer to be available", lambda: "Available" in now(d))
    d.check("Auto Context is on after the restart", "On" in control(d) and "Off" not in control(d), control(d))
    listed = d.js("return [...document.querySelectorAll('ol.context-rules > li')].map(li => li.getAttribute('aria-label'))")
    d.check("with its two rules", listed == ["Rule 1: Alpha", "Rule 2: Beta"], listed)
    offered = d.js("return [...document.querySelectorAll(\"section[aria-label='Auto Context'] select[aria-label='Application'] option\")].map(o => o.textContent)")
    d.check("what was seen in front before the restart is forgotten", "Gamma (x11:Gamma)" not in offered, offered)

    d.sidebar("Surfaces")
    front(d, ALPHA)
    until_shown(d, "Development")
    d.check("and the rules work: Alpha shows Development", shown(d) == "Development", shown(d))
    d.shot("restart")


STEPS = [unavailable, rules, switching, override, edit_mode, restart]
