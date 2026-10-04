#!/bin/sh
# Runs the tests that need a real credential store against a real one, without
# touching yours: a private D-Bus session, a GNOME Keyring started for it with
# a throwaway password, and a data folder that is deleted afterwards.
#
# Linux only. Needs dbus (dbus-run-session) and gnome-keyring:
#   apt install dbus gnome-keyring
#
# On macOS and Windows the store is always there; run the test directly if you
# accept that it writes and removes one entry ("LaunchDeck plugin example.home-assistant"):
#   cargo test --test secret_store -- --ignored
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
for tool in dbus-run-session gnome-keyring-daemon; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing: $tool (apt install dbus gnome-keyring)" >&2; exit 2; }
done
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/data" "$work/run"
chmod 700 "$work/run"
export XDG_DATA_HOME="$work/data" XDG_RUNTIME_DIR="$work/run"
cd "$root/src-tauri"
# Build first, so the session only lives while the test runs.
CARGO_INCREMENTAL=0 cargo test --test secret_store --no-run
dbus-run-session -- sh -c '
  printf %s "launchdeck-test" | gnome-keyring-daemon --unlock --components=secrets >/dev/null 2>&1
  CARGO_INCREMENTAL=0 cargo test --test secret_store -- --ignored --nocapture
  status=$?
  # The keyring file exists and holds no value in the clear.
  if grep -rqa "plain-file-era-token\|a-second-value" "$XDG_DATA_HOME/keyrings" 2>/dev/null; then
    echo "FAIL: a secret is readable in the keyring files" >&2; status=1
  fi
  ls "$XDG_DATA_HOME/keyrings" >/dev/null 2>&1 || { echo "FAIL: the keyring wrote nothing" >&2; status=1; }
  exit $status
'
