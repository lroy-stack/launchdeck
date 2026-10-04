"""A small W3C WebDriver client for the real LaunchDeck window.

It talks to `tauri-driver`, which starts the application and forwards to
WebKitWebDriver. Only the standard library is used.
"""

import base64
import http.client
import json
import time
import urllib.error
import urllib.request

KEYS = {
    "Backspace": "",
    "Tab": "",
    "Enter": "",
    "Shift": "",
    "Control": "",
    "Alt": "",
    "Escape": "",
    "Space": "",
    "End": "",
    "Home": "",
    "Left": "",
    "Up": "",
    "Right": "",
    "Down": "",
}

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class DriverError(RuntimeError):
    """The driver answered with an error."""


class DriverGone(RuntimeError):
    """The driver stopped answering. The step is worth running again."""


class WebDriver:
    def __init__(self, base="http://127.0.0.1:4444"):
        self.base = base
        self.sid = None
        # How often tauri-driver lost its connection to WebKitWebDriver and the request was sent again.
        self.drops = 0

    # -- transport ---------------------------------------------------------

    def _req(self, method, path, body=None, timeout=60):
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={"Content-Type": "application/json"})
        for attempt in (1, 2, 3):
            try:
                with urllib.request.urlopen(request, timeout=timeout) as response:
                    return json.loads(response.read().decode() or "{}").get("value")
            except urllib.error.HTTPError as e:
                raise DriverError(f"{method} {path}: {e.read().decode()[:400]}") from None
            except (http.client.RemoteDisconnected, ConnectionResetError, BrokenPipeError, urllib.error.URLError) as e:
                # tauri-driver's connection to WebKitWebDriver is reset now and then, before the
                # request is delivered. Asking again is safe; three failures in a row are not that.
                self.drops += 1
                if attempt == 3:
                    raise DriverGone(f"{method} {path}: {e}") from None
                time.sleep(0.3)

    def _s(self, path):
        return f"/session/{self.sid}{path}"

    def status(self):
        return self._req("GET", "/status", timeout=5)

    def start(self, application):
        value = self._req("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {"application": application}}}}, timeout=120)
        self.sid = value["sessionId"] if isinstance(value, dict) and "sessionId" in value else value
        return self

    def quit(self):
        if not self.sid:
            return
        try:
            self._req("DELETE", f"/session/{self.sid}", timeout=15)
        except (DriverError, DriverGone):
            pass
        self.sid = None

    # -- the page ----------------------------------------------------------

    def js(self, script, *args):
        return self._req("POST", self._s("/execute/sync"), {"script": script, "args": list(args)})

    def find_all(self, css):
        found = self._req("POST", self._s("/elements"), {"using": "css selector", "value": css})
        return [e[next(iter(e))] for e in found]

    def find(self, css, wait=8):
        end = time.time() + wait
        while True:
            found = self.find_all(css)
            if found:
                return found[0]
            if time.time() > end:
                raise DriverError(f"not found: {css}")
            time.sleep(0.2)

    def children(self, element, css):
        found = self._req("POST", self._s(f"/element/{element}/elements"), {"using": "css selector", "value": css})
        return [e[next(iter(e))] for e in found]

    def ref(self, element):
        return {ELEMENT: element}

    def click(self, element):
        try:
            self._req("POST", self._s(f"/element/{element}/click"), {})
        except DriverError as e:
            if "not interactable" not in str(e):
                raise
            self.js("arguments[0].scrollIntoView({block:'center'})", self.ref(element))
            time.sleep(0.2)
            self._req("POST", self._s(f"/element/{element}/click"), {})

    def text(self, element):
        # Through the page, not the driver's "Get Element Text": a stale element there has taken WebKitWebDriver down.
        try:
            return self.js("return arguments[0].innerText ?? arguments[0].textContent ?? ''", self.ref(element))
        except DriverError as e:
            if "stale" in str(e).lower():
                return ""
            raise

    def attr(self, element, name):
        return self._req("GET", self._s(f"/element/{element}/attribute/{name}"))

    def prop(self, element, name):
        return self._req("GET", self._s(f"/element/{element}/property/{name}"))

    def send(self, element, text):
        """Type into an element (it takes the focus first)."""
        self._req("POST", self._s(f"/element/{element}/value"), {"text": text})

    # -- the keyboard ------------------------------------------------------

    def keys(self, *sequence):
        """Press keys in order. A tuple such as ("Alt", "Down") is a chord: hold the first ones, tap the last."""
        actions = []
        for item in sequence:
            chord = list(item) if isinstance(item, tuple) else [item]
            for k in chord:
                actions.append({"type": "keyDown", "value": KEYS.get(k, k)})
            for k in reversed(chord):
                actions.append({"type": "keyUp", "value": KEYS.get(k, k)})
        self._req("POST", self._s("/actions"), {"actions": [{"type": "key", "id": "kb", "actions": actions}]})
        self._req("DELETE", self._s("/actions"))

    def typing(self, text):
        """Type text into whatever has the focus."""
        actions = [a for ch in text for a in ({"type": "keyDown", "value": ch}, {"type": "keyUp", "value": ch})]
        self._req("POST", self._s("/actions"), {"actions": [{"type": "key", "id": "kb", "actions": actions}]})
        self._req("DELETE", self._s("/actions"))

    def replace(self, element, text):
        """Type over what a field holds the way a person does: select it all, delete it, type.

        The driver's own "clear" empties the field without telling React, which then
        appends to the old text.
        """
        self.click(element)
        self.keys(("Control", "a"))
        self.keys("Backspace")
        if text:
            self.typing(text)

    def screenshot(self, path):
        png = self._req("GET", self._s("/screenshot"))
        with open(path, "wb") as f:
            f.write(base64.b64decode(png))
