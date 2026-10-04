#!/bin/sh
# Rebuilds the example plugin modules from source.
#   hello, counter, channels, home-assistant
#                   AssemblyScript through the SDK (needs Node and npm)
#   c-ping          plain C (needs clang with the wasm32 target and wasm-ld)
#   net-probe       the plugin LaunchDeck's own network tests use (not an example;
#                   built only when named)
# The built .wasm files are committed, so this is only needed after changing
# a plugin or the SDK. Pass names to build some: scripts/build-example-plugins.sh hello
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/examples/plugins"
want="${*:-hello counter channels home-assistant c-ping}"
for name in $want; do
  case "$name" in
    c-ping)
      sh "$name/build.sh"
      ;;
    hello|counter|channels|home-assistant)
      (cd "$name" && npm install --no-audit --no-fund --silent && npm run --silent build)
      echo "built $name ($(wc -c < "$name/$name.wasm") bytes)"
      ;;
    net-probe)
      (cd "$root/src-tauri/tests/fixtures/plugins/net-probe" && npm install --no-audit --no-fund --silent && npm run --silent build)
      echo "built $name"
      ;;
    *)
      echo "unknown example: $name" >&2
      exit 1
      ;;
  esac
done
