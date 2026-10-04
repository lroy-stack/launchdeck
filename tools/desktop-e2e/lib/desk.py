"""What a step uses to drive LaunchDeck: the WebDriver session plus the words of the app.

Everything here acts the way a person would (a real click, a real key press)
unless the name says it only reads (`led`, `text_of`, `value`).
"""

import os
import time

from .webdriver import DriverError, WebDriver


class Desk(WebDriver):
    def __init__(self, base, shots_dir, step_name):
        super().__init__(base)
        self.shots_dir = shots_dir
        self.step_name = step_name
        self.checks = []  # (name, passed, detail)

    # -- results -----------------------------------------------------------

    def check(self, name, ok, detail=""):
        """Record one thing that must hold. A failed check does not stop the step."""
        ok = bool(ok)
        self.checks.append((name, ok, str(detail)))
        print(("    ok    " if ok else "    FAIL  ") + name + (f"   [{str(detail)[:300]}]" if detail and not ok else ""), flush=True)
        return ok

    def note(self, text):
        print("          " + str(text), flush=True)

    def shot(self, name):
        path = os.path.join(self.shots_dir, f"{self.step_name}-{name}.png")
        try:
            self.screenshot(path)
        except DriverError:
            pass
        return path

    # -- reading the page --------------------------------------------------

    def text_of(self, css):
        return self.js("return document.querySelector(arguments[0])?.innerText || ''", css)

    def count(self, css):
        return self.js("return document.querySelectorAll(arguments[0]).length", css)

    def value(self, css, nth=0):
        return self.js("return document.querySelectorAll(arguments[0])[arguments[1]]?.value ?? null", css, nth)

    def inspector_text(self):
        return self.text_of(".inspector")

    def active(self):
        """What has the focus, in a few words."""
        return self.js(
            """const e = document.activeElement; if (!e) return null;
            return {tag: e.tagName.toLowerCase(), role: e.getAttribute('role') || '', label: e.getAttribute('aria-label') || '',
                    part: (e.dataset && e.dataset.part) || '', text: (e.innerText || '').trim().slice(0, 40),
                    value: ('value' in e) ? String(e.value).slice(0, 30) : ''}"""
        )

    def dump(self, root=".inspector", limit=120):
        """The controls under `root`, one line each. For writing a new step, not for checks."""
        return self.js(
            """const r = document.querySelector(arguments[0]); if (!r) return ['nothing matches ' + arguments[0]];
            return [...r.querySelectorAll('button,input,select,textarea,[role=combobox],[role=option],[role=status],h3,h4,[role=group],section')]
              .slice(0, arguments[1]).map(e => {
                const t = e.tagName.toLowerCase(), l = e.getAttribute('aria-label') || '', ro = e.getAttribute('role') || '';
                const txt = (e.innerText || '').trim().replace(/\\s+/g, ' ').slice(0, 70);
                const v = ('value' in e && t !== 'button') ? (' =' + e.value) : '';
                const off = (e.disabled || e.getAttribute('aria-disabled')) ? ' [disabled]' : '';
                return t + (ro ? '/' + ro : '') + (e.dataset.part ? '#' + e.dataset.part : '') + ' "' + l + '" ' + txt + v + off; });""",
            root,
            limit,
        )

    # -- acting ------------------------------------------------------------

    def click_css(self, css, nth=0):
        self.click(self.find_all(css)[nth])

    def click_text(self, css, text, pause=0.4, starts=False):
        """Click the first element matching `css` whose text is `text` (or starts with it)."""
        index = self.js(
            "const t = arguments[1], s = arguments[2]; return [...document.querySelectorAll(arguments[0])]"
            ".findIndex(b => s ? b.innerText.trim().startsWith(t) : b.innerText.trim() === t)",
            css,
            text,
            starts,
        )
        if index is None or index < 0:
            have = self.js("return [...document.querySelectorAll(arguments[0])].map(b => b.innerText.trim().slice(0, 30)).filter(Boolean).slice(0, 25)", css)
            raise DriverError(f"nothing in {css!r} says {text!r}; there is {have}")
        self.click(self.find_all(css)[index])
        time.sleep(pause)

    def choose(self, select_css, value, nth=0, verify=True):
        """Pick an option of a <select> by clicking the option, the way a pointer would."""
        select = self.find_all(select_css)[nth]
        options = [o for o in self.children(select, "option") if self.attr(o, "value") == value]
        if not options:
            have = [self.attr(o, "value") for o in self.children(select, "option")]
            raise DriverError(f"{select_css}: no option {value!r}; there is {have}")
        self.click(options[0])
        time.sleep(0.35)
        if verify:
            got = self.value(select_css, nth)
            if got != value:
                raise DriverError(f"{select_css}: wanted {value!r}, it holds {got!r}")

    def tab_until(self, condition, limit=60, back=False):
        """Press Tab (or Shift+Tab) until `condition`, JavaScript about the focused element `e`, holds.

        Returns how many presses it took.
        """
        script = "const e = document.activeElement; return !!(e && (" + condition + "))"
        for presses in range(limit + 1):
            if self.js(script):
                return presses
            self.keys(("Shift", "Tab") if back else "Tab")
            time.sleep(0.04)
        raise DriverError(f"not reached in {limit} Tab presses: {condition}; the focus is on {self.active()}")

    # -- LaunchDeck: getting around ----------------------------------------

    def sidebar(self, name):
        """Open a view or a surface from the sidebar by its name."""
        index = self.js(
            "const n = arguments[0]; return [...document.querySelectorAll('button')].findIndex(b => {"
            " const t = b.innerText.trim().replace(/\\s+/g, ' '); return t.endsWith(n) && t.length <= n.length + 3; })",
            name,
        )
        if index is None or index < 0:
            raise DriverError(f"the sidebar has nothing called {name!r}")
        self.click(self.find_all("button")[index])
        time.sleep(0.8)

    def mode(self, which):
        """`Live` or `Edit`."""
        self.click_text("[role=tablist][aria-label=Mode] button", which, pause=0.6)

    # -- LaunchDeck: the grid ------------------------------------------------

    def pad_index(self, col, row):
        index = self.js(
            "const c = String(arguments[0]), r = String(arguments[1]);"
            "return [...document.querySelectorAll('.pad')].findIndex(b => b.style.gridColumnStart == c && b.style.gridRowStart == r)",
            col,
            row,
        )
        if index is None or index < 0:
            raise DriverError(f"no pad at column {col}, row {row}")
        return index

    def click_pad(self, col, row, pause=0.5):
        self.click(self.find_all(".pad")[self.pad_index(col, row)])
        time.sleep(pause)

    def led(self, col, row):
        """The colour the virtual Launchpad shows on a pad, as #rrggbb, or `transparent`."""
        return (
            self.js(
                "const c = String(arguments[0]), r = String(arguments[1]);"
                "const b = [...document.querySelectorAll('.pad')].find(b => b.style.gridColumnStart == c && b.style.gridRowStart == r);"
                "return b ? b.style.getPropertyValue('--led') : ''",
                col,
                row,
            )
            or ""
        ).strip().lower()

    def sample_led(self, col, row, every_ms=25):
        """Start recording a pad's colour and animation; `sampled()` returns what was seen."""
        self.js(
            """window.__e2eSamples = []; const t0 = performance.now();
            const pad = [...document.querySelectorAll('.pad')].find(b => b.style.gridColumnStart == String(arguments[0]) && b.style.gridRowStart == String(arguments[1]));
            window.__e2eTimer = setInterval(() => window.__e2eSamples.push([Math.round(performance.now() - t0),
              pad.style.getPropertyValue('--led').trim().toLowerCase(), /anim-/.test(pad.className)]), arguments[2]);""",
            col,
            row,
            every_ms,
        )

    def sampled(self):
        """The runs of colour since `sample_led`: [colour, first ms, last ms, animated]."""
        samples = self.js("clearInterval(window.__e2eTimer); return window.__e2eSamples")
        runs = []
        for at, colour, animated in samples:
            if not runs or runs[-1][0] != colour:
                runs.append([colour, at, at, animated])
            else:
                runs[-1][2] = at
                runs[-1][3] = runs[-1][3] or animated
        return runs

    # -- LaunchDeck: the editor ---------------------------------------------

    def assign_action(self, col, row, query):
        """Select a pad in Edit mode and give it the first action the Library finds for `query`."""
        self.click_pad(col, row)
        self.replace(self.find("input[aria-label='Search actions']"), query)
        time.sleep(0.4)
        self.click_text(".library-detail button", "Assign to", pause=0.6, starts=True)

    def save_control(self):
        index = self.js("return [...document.querySelectorAll('.inspector .btn.primary')].findIndex(b => ['Save', 'Add control'].includes(b.innerText.trim()))")
        self.click(self.find_all(".inspector .btn.primary")[index])
        time.sleep(1.0)

    def create_look(self, select_css, name, colour, animation="none"):
        """Make a look from a rule's look field, without leaving the rule."""
        self.choose(select_css, "__new__", verify=False)
        self.send(self.find(".new-look input"), name)
        self.replace(self.find(".new-look input.mono"), colour)
        if animation != "none":
            self.choose("select[aria-label='New look animation']", animation)
        self.click_text(".new-look button", "Create look")

    def live_panel(self):
        return self.text_of(".live-feedback")

    def rule_status(self):
        """The line that announces what just happened to the rules."""
        return self.text_of(".feedback .rule-status")

    # -- LaunchDeck: plugins --------------------------------------------------

    def plugin_card_index(self, plugin):
        index = self.js(
            "const n = arguments[0]; return [...document.querySelectorAll('article.card.plugin')].findIndex(c => (c.querySelector('h3')?.innerText || '').includes(n))",
            plugin,
        )
        if index is None or index < 0:
            raise DriverError(f"the Plugins view has no card for {plugin!r}")
        return index

    def plugin_button(self, plugin, label, pause=1.0):
        """Click a button of one plugin's card in the Plugins view."""
        card = self.find_all("article.card.plugin")[self.plugin_card_index(plugin)]
        for b in self.children(card, "button"):
            if (self.text(b) or "").strip() == label:
                self.click(b)
                time.sleep(pause)
                return
        raise DriverError(f"{plugin}: no button says {label!r}")

    def plugin_card_text(self, plugin):
        return self.js("return document.querySelectorAll('article.card.plugin')[arguments[0]].innerText", self.plugin_card_index(plugin))
