"""Where a secret may be kept: the system's credential store, or nowhere.

The first two steps run the app with **no credential store** (no session bus,
so no Secret Service answers): secrets are unavailable, in words, and nothing
is written to a file instead. The last two give it a real one (a private D-Bus
session with an unlocked GNOME Keyring) and a plain `secrets.json` such as an
earlier version left behind, and move it across.

The plugin is the Home Assistant example; its server is the stand-in.
"""

import glob
import json
import os
import time

from lib.mock_home_assistant import MockHomeAssistant
from suites.home_assistant import LIGHT, PAD, PLUGIN, SURFACE, files_holding, network, secrets, wait_for

NAME = "secrets"
PLUGINS = ["home-assistant"]
TOKEN = "desktop-e2e-token-from-the-plain-file-era"
REASON = "No Secret Service answered in this session."


def setup(env):
    remembered = os.path.join(env.work, "secrets-suite.port")
    port = int(open(remembered).read()) if os.path.exists(remembered) else 0
    env.ha = MockHomeAssistant(TOKEN, {LIGHT: "off"}, port)
    with open(remembered, "w") as f:
        f.write(str(env.ha.port))


def teardown(env):
    env.ha.stop()


def app_dir(d):
    return os.path.dirname(glob.glob(os.path.join(d.env.data, "*", "workspace.json"))[0])


def legacy_path(d):
    return os.path.join(app_dir(d), "secrets.json")


def notice(d):
    return d.js("return document.querySelector('section[aria-label=\"Secrets kept in plain text\"]')?.innerText || ''")


def press(d):
    d.sidebar(SURFACE)
    d.mode("Live")
    d.click_pad(*PAD, pause=1.5)


def unavailable(d):
    """Without a credential store a secret cannot be saved, the screen says why, and no file is written instead."""
    ha = d.env.ha
    d.sidebar("Plugins")
    index = d.js(
        "const c = document.querySelectorAll('article.card.plugin')[arguments[0]];"
        "const l = [...c.querySelectorAll('label.field')].find(l => l.innerText.includes('Home Assistant address'));"
        "return [...document.querySelectorAll('article.card.plugin label.field input')].indexOf(l.querySelector('input'))",
        d.plugin_card_index(PLUGIN),
    )
    d.replace(d.find_all("article.card.plugin label.field input")[index], ha.origin)
    d.keys("Enter")
    wait_for(d, "the request for the address", lambda: ha.origin in network(d))
    d.click_css(f"button[aria-label='Allow {ha.origin}']")
    wait_for(d, "Granted", lambda: "Granted" in network(d))

    text = secrets(d)
    d.check("the Secrets section says Unavailable", "Unavailable" in text and "Not set" not in text, text[:200])
    d.check("it says secrets cannot be saved here, and why", "Secrets cannot be saved on this computer." in text and REASON in text, text[:400])
    d.check("it says what to do about it", "Install and unlock one, then restart LaunchDeck." in text, text[:500])
    d.check("it says a plain file is never used instead, and that the rest works", "never written to a plain file" in text and "Everything else does." in text, text[:700])
    disabled = d.js(
        "const c = document.querySelectorAll('article.card.plugin')[arguments[0]];"
        "return [c.querySelector('.field.secret input').disabled, c.querySelector(\"button[aria-label='Save Long-lived access token']\").disabled]",
        d.plugin_card_index(PLUGIN),
    )
    d.check("there is nowhere to type a secret and nothing to press", disabled == [True, True], disabled)
    running = d.js("return !!document.querySelectorAll('article.card.plugin')[arguments[0]].querySelector('header .pill.live')", d.plugin_card_index(PLUGIN))
    d.check("the plugin itself is on, and its network access is as usable as ever", "Granted" in network(d) and running)
    d.shot("unavailable")

    d.sidebar(SURFACE)
    d.mode("Edit")
    d.assign_action(*PAD, "toggle")
    field = d.js("const l = [...document.querySelectorAll('.inspector label')].find(l => l.innerText.startsWith('Entity')); return l.htmlFor")
    d.send(d.find(f"[id='{field}']"), LIGHT)
    time.sleep(0.3)
    d.save_control()
    press(d)
    d.check("a press that needs the secret sends nothing", ha.hits() == 0, ha.hits())
    d.sidebar("Plugins")
    text = network(d)
    d.check("the card says why: no secure credential store", "no secure credential store" in text and "(secret_unavailable)" in text, text[-300:])
    d.check("no plain secrets file exists", not os.path.exists(legacy_path(d)))
    d.shot("refused")


def legacy_without_store(d):
    """A plain secrets.json from an earlier version is reported, not used, and left as it is."""
    ha = d.env.ha
    path = legacy_path(d)
    planted = json.dumps({"schemaVersion": 1, "secrets": {"example.home-assistant": {"token": TOKEN}}})
    with open(path, "w") as f:
        f.write(planted)
    os.chmod(path, 0o600)

    d.sidebar("Plugins")
    wait_for(d, "the notice about plain secrets", lambda: notice(d) != "")
    text = notice(d)
    d.check("a notice lists it by plugin and name, never by value", "Home Assistant" in text and "Long-lived access token" in text and TOKEN not in d.js("return document.documentElement.innerHTML"), text[:300])
    d.check("it says the plain copy is not used", "It is not used" in text, text[:300])
    d.check("it cannot be moved without a credential store, and says so", "It cannot be moved, because this computer has no secure credential store." in text and d.count("section[aria-label='Secrets kept in plain text'] button.primary") == 0, text[:600])
    d.check("the secret is still unavailable", "Unavailable" in secrets(d))
    d.shot("legacy-no-store")

    before = ha.hits()
    press(d)
    d.check("the plain value authenticates nothing", ha.hits() == before, (before, ha.hits()))
    d.check("and the file is byte for byte what it was", open(path).read() == planted)


def migrate(d):
    """With a credential store the plain secret is still not used until it is moved; moving it is one explicit step."""
    ha = d.env.ha
    path = legacy_path(d)
    d.sidebar("Plugins")
    wait_for(d, "the notice about plain secrets", lambda: notice(d) != "")
    text = secrets(d)
    d.check("the store is the system keyring now", "system's credential store (Secret Service keyring)" in text, text[:300])
    d.check("the secret reads Not set: the plain copy is not adopted by itself", "Not set" in text, text[:300])
    before = ha.hits()
    press(d)
    d.check("a press still sends nothing", ha.hits() == before, (before, ha.hits()))
    d.sidebar("Plugins")
    d.check("because the secret is not set", "(secret_missing)" in network(d), network(d)[-200:])
    d.check("the plain file has not been touched", os.path.exists(path) and TOKEN in open(path).read())
    d.shot("legacy-with-store")

    d.click_text("section[aria-label='Secrets kept in plain text'] button", "Move to Secret Service keyring")
    wait_for(d, "the notice to go", lambda: notice(d) == "")
    d.check("after Move the notice is gone", notice(d) == "")
    toast = d.js("return [...document.querySelectorAll('.toast')].map(t => t.innerText).join(' | ')")
    d.check("it says what it did", "1 secret was moved into Secret Service keyring. The plain file was overwritten and removed." in toast, toast)
    d.check("the secret reads Set", "Not set" not in secrets(d) and "Set" in secrets(d))
    d.check("the plain file is gone", not os.path.exists(path))
    d.check("and the token is readable in no file, the keyring's included", files_holding(d, TOKEN) == [], files_holding(d, TOKEN))
    trail = open(os.path.join(app_dir(d), "audit.jsonl")).read()
    d.check("the audit trail records the move, without the value", "secrets.migrated" in trail and TOKEN not in trail, trail[-200:])
    d.shot("moved")

    press(d)
    wait_for(d, "the light to turn on", lambda: ha.state(LIGHT) == "on")
    d.check("a press now reaches Home Assistant with the token", ha.state(LIGHT) == "on" and ha.requests[-1][3], ha.requests[-2:])


migrate.keyring = True


def survives_restart(d):
    """After a restart the secret comes from the credential store."""
    ha = d.env.ha
    d.sidebar("Plugins")
    d.check("the secret is still Set and nothing plain came back", "Not set" not in secrets(d) and notice(d) == "" and not os.path.exists(legacy_path(d)))
    was = ha.state(LIGHT)
    press(d)
    wait_for(d, "the light to change", lambda: ha.state(LIGHT) != was)
    d.check("and still authenticates", ha.state(LIGHT) != was and ha.requests[-1][3], ha.requests[-2:])


survives_restart.keyring = True

STEPS = [unavailable, legacy_without_store, migrate, survives_restart]
