"""Network permission, plugin settings, secrets and a pad that follows Home Assistant.

The plugin is the example in `examples/plugins/home-assistant`. Home Assistant
is a small stand-in on a loopback port (`lib/mock_home_assistant.py`); the
permission checks, the broker and the secret store are the app's own.
"""

import glob
import os
import time

from lib.mock_home_assistant import MockHomeAssistant

NAME = "home_assistant"
PLUGINS = ["home-assistant"]
# The token goes into a real credential store: the run's own keyring, not yours.
KEYRING = True

PLUGIN = "Home Assistant"
TOKEN = "desktop-e2e-long-lived-token"
LIGHT = "light.kitchen"
SURFACE = "Development"
PAD = (8, 8)
REST, ON, OFF, PENDING, FAILED = "#f5c518", "#22c55e", "#64748b", "#a855f7", "#ef4444"
DEFAULT_ADDRESS = "http://homeassistant.local:8123"


def setup(env):
    # The address the earlier steps saved names a port, so `--from` and `--only`
    # need the stand-in on the same one as the run that left the data.
    remembered = os.path.join(env.work, "home-assistant.port")
    port = int(open(remembered).read()) if os.path.exists(remembered) else 0
    env.ha = MockHomeAssistant(TOKEN, {LIGHT: "off"}, port)
    with open(remembered, "w") as f:
        f.write(str(env.ha.port))


def teardown(env):
    env.ha.stop()


def data_file(d, name):
    found = glob.glob(os.path.join(d.env.data, "*", name))
    return found[0] if found else None


def read(d, name):
    path = data_file(d, name)
    return open(path).read() if path else ""


def files_holding(d, text):
    """Every file under the run's data folder (the keyring's files included) that holds `text` in the clear."""
    found = []
    for folder, _, names in os.walk(d.env.data):
        for name in names:
            path = os.path.join(folder, name)
            try:
                if text.encode() in open(path, "rb").read():
                    found.append(os.path.relpath(path, d.env.data))
            except OSError:
                pass
    return found


def network(d):
    return d.js(
        "const c = document.querySelectorAll('article.card.plugin')[arguments[0]];"
        "return c.querySelector('section[aria-label^=\"Network access\"]')?.innerText || ''",
        d.plugin_card_index(PLUGIN),
    )


def secrets(d):
    return d.js(
        "const c = document.querySelectorAll('article.card.plugin')[arguments[0]];"
        "return c.querySelector('section[aria-label^=\"Secrets\"]')?.innerText || ''",
        d.plugin_card_index(PLUGIN),
    )


def badge(d):
    return d.js("return document.querySelector('nav [aria-label*=\"permission request\"]')?.getAttribute('aria-label') || ''")


def wait_for(d, what, condition, seconds=8):
    end = time.time() + seconds
    while time.time() < end:
        if condition():
            return True
        time.sleep(0.2)
    return d.check(what, False, "it did not happen in time")


def prompt(d):
    """A plugin that asks for the network says exactly where, and gets nothing by asking."""
    ha = d.env.ha
    d.sidebar("Plugins")
    text = network(d)
    d.check("the card says the plugin requests network access to the exact address", f"{PLUGIN} requests network access to:" in text and DEFAULT_ADDRESS in text, text[:300])
    d.check("it says how far that address reaches", "Your local network" in text, text[:300])
    d.check("it says what for, in the plugin's words", "Connect to your Home Assistant instance." in text)
    d.check("the status is Needs approval, in words", "Needs approval" in text)
    d.check("the sidebar says one request is waiting", badge(d) == "1 permission request waiting", badge(d))
    d.shot("prompt")

    # The address is a setting. Typing another one asks for that one instead.
    index = d.js(
        "const c = document.querySelectorAll('article.card.plugin')[arguments[0]];"
        "const l = [...c.querySelectorAll('label.field')].find(l => l.innerText.includes('Home Assistant address'));"
        "return [...document.querySelectorAll('article.card.plugin label.field input')].indexOf(l.querySelector('input'))",
        d.plugin_card_index(PLUGIN),
    )
    d.replace(d.find_all("article.card.plugin label.field input")[index], ha.origin)
    d.keys("Enter")
    wait_for(d, "the prompt to name the new address", lambda: ha.origin in network(d))
    text = network(d)
    d.check("after typing the address, the request is for exactly that origin", f"requests network access to:" in text and ha.origin in text and DEFAULT_ADDRESS not in text.split("no longer asks for")[0], text[:300])
    d.check("a loopback address is said to be this computer", "This computer" in text, text[:300])
    d.check("typing an address grants nothing", "Needs approval" in text)
    time.sleep(1.5)
    d.check("and nothing has reached the server", ha.hits() == 0, ha.hits())
    d.check("the address is saved with the plugin's settings", ha.origin in read(d, "plugins.json"))
    d.shot("address")


def deny_allow(d):
    """Deny is remembered, Allow grants exactly one address, and both survive a restart."""
    ha = d.env.ha
    d.sidebar("Plugins")
    d.check("after a restart the request is still waiting", "Needs approval" in network(d) and ha.origin in network(d))
    d.click_css(f"button[aria-label='Deny {ha.origin}']")
    wait_for(d, "Denied", lambda: "Denied" in network(d))
    text = network(d)
    d.check("Deny shows Denied and the prompt goes", "Denied" in text and "requests network access to:" not in text, text[:200])
    d.check("a denied request is not counted as waiting", badge(d) == "", badge(d))
    time.sleep(1.2)
    d.check("nothing reached the server while denied", ha.hits() == 0, ha.hits())
    d.shot("denied")

    d.click_css(f"button[aria-label='Allow {ha.origin}']")
    wait_for(d, "Granted", lambda: "Granted" in network(d))
    text = network(d)
    d.check("Allow shows Granted with a way to revoke it", "Granted" in text and d.count(f"button[aria-label='Revoke {ha.origin}']") == 1, text[:200])
    grants = read(d, "permissions.json")
    d.check("the grant is in permissions.json, by exact origin", ha.origin in grants and '"granted"' in grants, grants[:200])
    d.check("and not in the workspace", "granted" not in read(d, "workspace.json") and ha.origin not in read(d, "workspace.json"))
    trail = read(d, "audit.jsonl")
    d.check("the audit trail has the request, the denial and the grant", all(k in trail for k in ("permission.requested", "permission.denied", "permission.granted")), trail[-300:])
    d.shot("granted")


def secret(d):
    """The token is typed once, kept out of sight, and written to no file a surface or a plugin can carry."""
    d.sidebar("Plugins")
    text = secrets(d)
    d.check("the secret is listed by its name, as Not set", "Long-lived access token" in text and "Not set" in text, text[:300])
    d.check("it says the plugin never receives it and where it is kept", "never handed to the plugin" in text and "system's credential store (Secret Service keyring)" in text, text[:300])
    d.check("it says the only place it is sent", f"Only ever sent to {d.env.ha.origin}" in text, text[:300])
    field = d.find("article.card.plugin .field.secret input[type=password]")
    d.send(field, TOKEN)
    d.click_css("button[aria-label='Save Long-lived access token']")
    wait_for(d, "the secret to be Set", lambda: "Not set" not in secrets(d))
    d.check("after Save it reads Set and the field is empty", "Set" in secrets(d) and d.value("article.card.plugin .field.secret input[type=password]") == "")
    d.check("the value is nowhere on the page", TOKEN not in d.js("return document.documentElement.innerHTML"))
    d.shot("secret")

    d.check("no plain secrets file was written", data_file(d, "secrets.json") is None)
    keyring = os.path.join(d.env.data, "keyrings")
    d.check("the system keyring holds something now", os.path.isdir(keyring) and any(os.path.getsize(os.path.join(keyring, f)) > 0 for f in os.listdir(keyring)), keyring)
    d.check("the token is readable in no file at all: not the workspace, the settings, the grants, the audit trail or the keyring's own files", files_holding(d, TOKEN) == [], files_holding(d, TOKEN))
    logs = "".join(open(p, errors="replace").read() for p in glob.glob(os.path.join(d.env.logs, "*.log")))
    d.check("nor in any log", TOKEN not in logs)


def pad(d):
    """A pad toggles the light, shows pending while the request is out, and follows the entity's state."""
    ha = d.env.ha
    d.sidebar(SURFACE)
    d.mode("Edit")
    d.assign_action(*PAD, "toggle")
    text = d.inspector_text()
    d.check("the Library offers Home Assistant / Toggle", "Toggle" in text and "Home Assistant" in text, text[:200])
    field = d.js("const l = [...document.querySelectorAll('.inspector label')].find(l => l.innerText.startsWith('Entity')); return l.htmlFor")
    d.send(d.find(f"[id='{field}']"), LIGHT)
    time.sleep(0.3)

    d.click_css("button[aria-label='Add feedback from Entity state']")
    time.sleep(0.5)
    d.send(d.find(".flow input"), LIGHT)
    d.click_text(".flow button", "Add the first rule")
    d.check("the entity is the instance the rules follow", f"entity {LIGHT}" in d.text_of(".rule-run"), d.text_of(".rule-run")[:120])
    d.create_look("select[aria-label='Rule 1 look']", "On", ON)
    d.click_text("button", "+ Add a rule")
    d.create_look("select[aria-label='Rule 2 look']", "Off", OFF)
    values = [d.value(f"select[aria-label='Rule {i} value']") for i in (1, 2)]
    d.check("the rules are On and Off", values == ["on", "off"], values)

    for name, colour, animation in (("Pending", PENDING, "pulse"), ("Failed", FAILED, "flash")):
        d.click_text(".looks button", "+ New look")
        d.send(d.find(".looks .new-look input"), name)
        d.replace(d.find(".looks .new-look input.mono"), colour)
        d.choose(".looks select[aria-label='New look animation']", animation)
        d.click_text(".looks .new-look button", "Create look")
    looks = d.text_of(".looks")
    d.check("Pending and Failed are explained as the pad's own action", "Shown while this pad’s action is running" in looks and "Shown for a moment when this pad’s action fails" in looks, looks[-300:])
    panel = d.live_panel()
    d.check("a draft asks Home Assistant nothing: the entity reads Unknown until the pad is saved", "Unknown" in panel and ha.hits() == 0, (panel[-120:], ha.hits()))
    d.shot("editor")
    d.save_control()
    d.check("the control is saved", "Not saved" not in d.inspector_text())
    wait_for(d, "the saved pad's entity to be read", lambda: "Known" in d.live_panel())
    panel = d.live_panel()
    d.check("once saved, the panel reads the entity from Home Assistant: Known, Off", "Known" in panel and "Off" in panel, panel[-200:])
    d.check("with the token, and for that entity only", ha.requests and all(r[3] and r[1] == f"/api/states/{LIGHT}" for r in ha.requests), ha.requests[:3])

    d.mode("Live")
    lamp = lambda: d.led(*PAD)  # noqa: E731
    wait_for(d, "the pad to show Off", lambda: lamp() == OFF)
    d.check("the pad shows Off, from Home Assistant's state", lamp() == OFF, lamp())

    ha.delay = 0.6
    before = len(ha.service_calls())
    d.sample_led(*PAD)
    d.click_pad(*PAD, pause=3.0)
    runs = d.sampled()
    ha.delay = 0.0
    d.note(f"LED over time: {runs}")
    colours = [r[0] for r in runs]
    d.check("the pad goes Off, Pending, On", colours[:1] == [OFF] and PENDING in colours and colours[-1:] == [ON], colours)
    waited = next((r for r in runs if r[0] == PENDING), None)
    d.check("pending lasts as long as the request is out", waited and waited[2] - waited[1] >= 800, waited)
    d.check("the call reached Home Assistant", ha.service_calls()[before:] == [f"light/toggle {LIGHT}"], ha.service_calls())
    d.check("and the light is on there", ha.state(LIGHT) == "on")

    ha.set(LIGHT, "off")
    wait_for(d, "a change made in Home Assistant to reach the pad", lambda: lamp() == OFF, seconds=10)
    d.check("a change made elsewhere arrives without a press", lamp() == OFF, lamp())
    d.shot("live")


def revoke(d):
    """Revoking stops every request at once, the pad says the action failed, and granting again recovers."""
    ha = d.env.ha
    lamp = lambda: d.led(*PAD)  # noqa: E731
    d.sidebar(SURFACE)
    d.mode("Live")
    wait_for(d, "the pad to show Off after a restart", lambda: lamp() == OFF)
    d.check("after a restart the grant, the address and the token still work", lamp() == OFF, lamp())

    d.sidebar("Plugins")
    d.click_css(f"button[aria-label='Revoke {ha.origin}']")
    wait_for(d, "Needs approval", lambda: "Needs approval" in network(d))
    d.check("Revoke puts the request back to Needs approval", "requests network access to:" in network(d))
    time.sleep(0.8)
    before = ha.hits()

    d.sidebar(SURFACE)
    wait_for(d, "the pad to rest", lambda: lamp() == REST)
    d.check("what can no longer be read is not shown: the pad rests", lamp() == REST, lamp())
    d.sample_led(*PAD)
    d.click_pad(*PAD, pause=2.5)
    colours = [r[0] for r in d.sampled()]
    d.check("a press shows the Failed look, which ends by itself", FAILED in colours and colours[-1:] == [REST], colours)
    time.sleep(1.0)
    d.check("no request reached Home Assistant without the grant", ha.hits() == before, (before, ha.hits()))
    d.check("and the light did not change", ha.state(LIGHT) == "off")

    d.sidebar("Plugins")
    text = network(d)
    d.check("the card says why the last request did not go through", "Last request did not go through." in text and "waiting for approval" in text and "(permission_denied)" in text, text[-300:])
    d.shot("revoked")

    d.click_css(f"button[aria-label='Allow {ha.origin}']")
    wait_for(d, "Granted", lambda: "Granted" in network(d))
    d.sidebar(SURFACE)
    wait_for(d, "the pad to show Off again", lambda: lamp() == OFF, seconds=10)
    d.check("granted again, the pad follows the light again", lamp() == OFF, lamp())
    d.click_pad(*PAD, pause=1.5)
    wait_for(d, "the pad to show On", lambda: lamp() == ON)
    d.check("and a press works", lamp() == ON and ha.state(LIGHT) == "on", (lamp(), ha.state(LIGHT)))


def remove_secret(d):
    """Without its token the plugin fails clearly and sends nothing."""
    ha = d.env.ha
    d.sidebar("Plugins")
    d.click_css("button[aria-label='Remove Long-lived access token']")
    wait_for(d, "Not set", lambda: "Not set" in secrets(d))
    d.check("Remove leaves the secret Not set", "Not set" in secrets(d))
    time.sleep(0.8)
    before = ha.hits()
    d.sidebar(SURFACE)
    d.mode("Live")
    d.click_pad(*PAD, pause=2.0)
    d.check("nothing is sent without the token", ha.hits() == before, (before, ha.hits()))
    d.sidebar("Plugins")
    text = network(d)
    d.check("the card says the secret is not set", "is not set" in text and "(secret_missing)" in text, text[-300:])
    d.shot("no-secret")


STEPS = [prompt, deny_allow, secret, pad, revoke, remove_secret]
