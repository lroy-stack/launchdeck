#!/usr/bin/env python3
"""Drives the real LaunchDeck window through WebDriver and checks what a person would see.

    python3 tools/desktop-e2e/run.py                  every suite, on the release build
    python3 tools/desktop-e2e/run.py feedback         one suite
    python3 tools/desktop-e2e/run.py --app debug feedback --from keyboard

See README.md next to this file.
"""

import argparse
import importlib
import os
import tempfile
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)

from lib.desk import Desk  # noqa: E402
from lib.webdriver import DriverError, DriverGone  # noqa: E402

SUITES = ["feedback", "home_assistant", "secrets", "context"]
# The password of the throwaway keyring a suite may ask for. It protects nothing.
KEYRING_PASSWORD = "launchdeck-desktop-e2e"
DEV_URL = "http://127.0.0.1:1420/"


def port_open(port, host="127.0.0.1"):
    with socket.socket() as s:
        s.settimeout(0.5)
        return s.connect_ex((host, port)) == 0


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def which(name, *more):
    for candidate in (name, *more):
        found = shutil.which(candidate) or (candidate if os.path.isabs(candidate) and os.path.exists(candidate) else None)
        if found:
            return found
    return None


class Environment:
    """Everything a run needs outside the app: a display, the driver, a data folder. All of it under `.work/`."""

    def __init__(self, args):
        self.args = args
        self.work = os.path.join(HERE, ".work")
        self.data = os.path.join(self.work, "data")
        self.cache = os.path.join(self.work, "cache")
        self.plugins = os.path.join(self.work, "plugins")
        self.aside = os.path.join(self.work, "plugins-aside")
        self.shots = os.path.join(self.work, "shots")
        self.logs = os.path.join(self.work, "logs")
        self.repo = REPO
        self.processes = []  # started here, stopped here
        self.display = None
        self.driver_port = None
        self.driver = None
        self.extra_env = {}
        # Whether the next launch gets a credential store (a private D-Bus
        # session with an unlocked GNOME Keyring). Set per suite or per step.
        self.keyring = False
        # The session's sockets, and whatever the desktop portals mount there.
        # Not under `.work/`: that kind of folder belongs in the system's
        # temporary space, and a mount inside the repository helps nobody.
        self.runtime_dir = None
        # A window manager on the display, for the steps that need the desktop
        # to say which window is active. Without one the app sees none.
        self.window_manager = None
        self.app = self.find_app(args.app)
        for d in (self.work, self.shots, self.logs):
            os.makedirs(d, exist_ok=True)

    # -- what is needed ----------------------------------------------------

    def find_app(self, which_app):
        builds = {"release": os.path.join(REPO, "src-tauri", "target", "release", "launchdeck"), "debug": os.path.join(REPO, "src-tauri", "target", "debug", "launchdeck")}
        if which_app in builds:
            path = builds[which_app]
            if not os.path.exists(path):
                hint = "npx tauri build --no-bundle" if which_app == "release" else "cargo build --manifest-path src-tauri/Cargo.toml"
                sys.exit(f"There is no {which_app} build at {path}. Build it first: {hint}")
            self.needs_dev_server = which_app == "debug"
            return path
        if not os.path.exists(which_app):
            sys.exit(f"No application at {which_app}")
        self.needs_dev_server = False
        return os.path.abspath(which_app)

    def check_tools(self):
        self.tauri_driver = which("tauri-driver", os.path.expanduser("~/.cargo/bin/tauri-driver"))
        self.native_driver = which("WebKitWebDriver", "/usr/bin/WebKitWebDriver")
        missing = []
        if not self.tauri_driver:
            missing.append("tauri-driver (cargo install tauri-driver --locked)")
        if not self.native_driver:
            missing.append("WebKitWebDriver (Debian/Ubuntu: apt install webkit2gtk-driver)")
        if not self.args.display and not os.environ.get("DISPLAY") and not which("Xvfb"):
            missing.append("Xvfb (apt install xvfb), or a DISPLAY to use")
        if self.needs_keyring:
            for tool, package in (("dbus-run-session", "dbus"), ("gnome-keyring-daemon", "gnome-keyring")):
                if not which(tool):
                    missing.append(f"{tool} (apt install {package}), for the suites that need a credential store")
        for tool in sorted(self.needs_tools):
            if not which(tool):
                missing.append(f"{tool} (apt install {tool}), for the suites that put other applications on the display")
        if missing:
            sys.exit("Missing:\n  " + "\n  ".join(missing))

    # -- processes ---------------------------------------------------------

    def spawn(self, name, command, env=None, cwd=None):
        log = open(os.path.join(self.logs, name + ".log"), "w")
        process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, env=env, cwd=cwd, start_new_session=True)
        self.processes.append((name, process))
        return process

    def stop(self, process):
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                return
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
        self.processes = [(n, p) for n, p in self.processes if p is not process]

    def start_display(self):
        if self.args.display:
            self.display = self.args.display
            return
        if os.environ.get("DISPLAY") and self.args.use_display:
            self.display = os.environ["DISPLAY"]
            return
        number = next(n for n in range(90, 200) if not os.path.exists(f"/tmp/.X11-unix/X{n}"))
        self.display = f":{number}"
        self.spawn("xvfb", ["Xvfb", self.display, "-screen", "0", "1500x1000x24", "-nolisten", "tcp"])
        for _ in range(50):
            if os.path.exists(f"/tmp/.X11-unix/X{number}"):
                return
            time.sleep(0.1)
        sys.exit("Xvfb did not start; see " + os.path.join(self.logs, "xvfb.log"))

    def set_window_manager(self, on):
        """Run a window manager (openbox) on the display, or stop it."""
        if on and not self.window_manager:
            self.window_manager = self.spawn("openbox", ["openbox"], env={**os.environ, "DISPLAY": self.display})
            time.sleep(1.0)
        elif not on and self.window_manager:
            self.stop(self.window_manager)
            self.window_manager = None
            time.sleep(0.4)

    def x(self, *command, check=False):
        """Run an X client (wmctrl, xprop, ...) on the run's display and return what it printed."""
        done = subprocess.run(list(command), env={**os.environ, "DISPLAY": self.display}, capture_output=True, text=True, check=check)
        return done.stdout

    def start_dev_server(self):
        if not self.needs_dev_server or port_open(1420):
            return
        self.spawn("vite", ["npm", "run", "dev", "--silent"], cwd=REPO)
        for _ in range(120):
            try:
                urllib.request.urlopen(DEV_URL, timeout=1).read(1)
                return
            except OSError:
                time.sleep(0.25)
        sys.exit("The debug build loads its interface from the dev server, which did not start; see " + os.path.join(self.logs, "vite.log"))

    def start_driver(self):
        """A new tauri-driver (and with it a new WebKitWebDriver) for each launch of the app."""
        self.stop_driver()
        self.driver_port, native_port = free_port(), free_port()
        env = dict(os.environ)
        env.update({"DISPLAY": self.display, "XDG_DATA_HOME": self.data, "XDG_CACHE_HOME": self.cache, "LAUNCHDECK_PLUGINS": self.plugins})
        env.update(self.extra_env)
        command = [self.tauri_driver, "--port", str(self.driver_port), "--native-port", str(native_port), "--native-driver", self.native_driver]
        # Without a session bus the app finds no credential store, which is a state worth testing too.
        for name in ("DBUS_SESSION_BUS_ADDRESS", "GNOME_KEYRING_CONTROL"):
            env.pop(name, None)
        if self.keyring:
            # A bus of its own and a keyring unlocked on it: a real Secret Service
            # that keeps its (encrypted) files under the run's data folder.
            if not self.runtime_dir:
                self.runtime_dir = tempfile.mkdtemp(prefix="launchdeck-e2e-run-")
            os.chmod(self.runtime_dir, 0o700)
            env.update({"XDG_RUNTIME_DIR": self.runtime_dir, "LD_E2E_KEYRING_PASSWORD": KEYRING_PASSWORD})
            unlock = 'printf %s "$LD_E2E_KEYRING_PASSWORD" | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1; exec "$@"'
            command = ["dbus-run-session", "--", "sh", "-c", unlock, "sh", *command]
        self.driver = self.spawn("tauri-driver", command, env=env)
        for _ in range(80):
            if port_open(self.driver_port):
                return
            time.sleep(0.1)
        sys.exit("tauri-driver did not start; see " + os.path.join(self.logs, "tauri-driver.log"))

    def stop_driver(self):
        if self.driver:
            self.stop(self.driver)
            self.driver = None

    def stop_all(self):
        for _, process in list(self.processes):
            self.stop(process)
        if self.runtime_dir:
            shutil.rmtree(self.runtime_dir, ignore_errors=True)
            self.runtime_dir = None

    # -- the app's world -----------------------------------------------------

    def fresh(self, plugins):
        """An app that has never run: no workspace, no settings, and only these example plugins installed."""
        for d in (self.data, self.cache, self.plugins, self.aside):
            shutil.rmtree(d, ignore_errors=True)
            os.makedirs(d)
        for name in plugins:
            self.install_plugin(name)

    def install_plugin(self, name):
        source = os.path.join(REPO, "examples", "plugins", name)
        target = os.path.join(self.plugins, name)
        os.makedirs(target, exist_ok=True)
        copied = [f for f in os.listdir(source) if f == "plugin.json" or f.endswith(".wasm")]
        if "plugin.json" not in copied or len(copied) < 2:
            sys.exit(f"examples/plugins/{name} has no built module; run: sh scripts/build-example-plugins.sh {name}")
        for f in copied:
            shutil.copy2(os.path.join(source, f), os.path.join(target, f))

    def set_plugin_aside(self, name, aside=True):
        """Take a plugin's folder out of the plugins folder, or put it back (then press Rescan in the app)."""
        a, b = os.path.join(self.plugins, name), os.path.join(self.aside, name)
        source, target = (a, b) if aside else (b, a)
        if os.path.exists(source):
            shutil.move(source, target)

    def open(self, step_name):
        """Start the app and return the session that drives it."""
        self.start_driver()
        desk = Desk(f"http://127.0.0.1:{self.driver_port}", self.shots, step_name)
        desk.env = self
        desk.start(self.app)
        time.sleep(2)
        return desk


def run_suite(env, suite, first=None, only=None):
    """Run the steps of one suite in order. Returns (step, check, passed, detail) for every check."""
    steps = [(f.__name__, f) for f in suite.STEPS]
    names = [n for n, _ in steps]
    for wanted in (first, only):
        if wanted and wanted not in names:
            sys.exit(f"{suite.NAME} has no step called {wanted}; it has: {', '.join(names)}")
    start = names.index(first or only) if (first or only) else 0
    if start == 0:
        env.fresh(suite.PLUGINS)
    env.extra_env = {}
    setup = getattr(suite, "setup", None)
    teardown = getattr(suite, "teardown", None)
    results = []
    try:
        if setup:
            setup(env)
        for name, step in steps[start:]:
            title = (step.__doc__ or name).strip().splitlines()[0]
            print(f"\n  {suite.NAME} / {name}: {title}", flush=True)
            env.keyring = getattr(step, "keyring", getattr(suite, "KEYRING", False))
            env.set_window_manager(getattr(step, "window_manager", getattr(suite, "WINDOW_MANAGER", False)))
            desk = env.open(f"{suite.NAME}-{name}")
            try:
                step(desk)
            except DriverGone:
                raise
            except Exception as e:  # a step that cannot go on is a failed check, with a picture
                desk.check("the step ran to its end", False, f"{type(e).__name__}: {e}")
                desk.shot("failed")
            finally:
                desk.quit()
                env.stop_driver()
            if desk.drops:
                print(f"          (the driver's connection was reset {desk.drops} time(s); the requests were sent again)")
            results += [(name, check, ok, detail) for check, ok, detail in desk.checks]
            if only:
                break
    finally:
        if teardown:
            teardown(env)
        env.set_window_manager(False)
    return results


def main():
    parser = argparse.ArgumentParser(description="Real-desktop checks for LaunchDeck.")
    parser.add_argument("suites", nargs="*", help=f"which suites to run (default: all of {', '.join(SUITES)})")
    parser.add_argument("--app", default="release", help="release (default), debug, or the path of a LaunchDeck executable")
    parser.add_argument("--from", dest="first", help="start at this step, on the data the earlier steps left")
    parser.add_argument("--only", help="run this one step, on the data the earlier steps left")
    parser.add_argument("--display", help="use this X display instead of starting Xvfb, e.g. :0 to watch")
    parser.add_argument("--use-display", action="store_true", help="use $DISPLAY instead of starting Xvfb")
    parser.add_argument("--list", action="store_true", help="list suites and steps, run nothing")
    args = parser.parse_args()

    wanted = args.suites or SUITES
    modules = []
    for name in wanted:
        if name not in SUITES:
            sys.exit(f"No suite called {name}; there is: {', '.join(SUITES)}")
        modules.append(importlib.import_module(f"suites.{name}"))
    if args.list:
        for m in modules:
            print(m.NAME)
            for f in m.STEPS:
                print(f"  {f.__name__:22} {(f.__doc__ or '').strip().splitlines()[0]}")
        return 0
    if (args.first or args.only) and len(modules) != 1:
        sys.exit("--from and --only need exactly one suite")

    env = Environment(args)
    env.needs_tools = {tool for m in modules for tool in getattr(m, "TOOLS", [])}
    env.needs_keyring = any(getattr(m, "KEYRING", False) or any(getattr(f, "keyring", False) for f in m.STEPS) for m in modules)
    env.check_tools()
    results = []
    try:
        env.start_display()
        env.start_dev_server()
        print(f"LaunchDeck desktop checks\n  app      {env.app}\n  display  {env.display}\n  work     {env.work}")
        for module in modules:
            for attempt in (1, 2):
                try:
                    results += [(module.NAME, *r) for r in run_suite(env, module, args.first, args.only)]
                    break
                except DriverGone as e:
                    env.stop_driver()
                    if attempt == 2 or args.first or args.only:
                        results.append((module.NAME, "-", "the driver kept answering", False, str(e)))
                        break
                    print(f"\n  The driver stopped answering ({e}). Running {module.NAME} again from the start.")
    finally:
        env.stop_all()

    failed = [r for r in results if not r[3]]
    print(f"\n{len(results) - len(failed)} of {len(results)} checks passed.")
    for suite, step, check, _, detail in failed:
        print(f"  FAILED  {suite} / {step}: {check}" + (f"   [{detail[:300]}]" if detail else ""))
    if failed:
        print(f"Screenshots and logs are in {env.work}")
    return 1 if failed or not results else 0


if __name__ == "__main__":
    sys.exit(main())
