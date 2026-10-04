#!/bin/sh
# Rebuilds the example plugin modules from source.
#   hello, counter  AssemblyScript through the SDK (needs Node and npm)
#   c-ping          plain C (needs clang with the wasm32 target and wasm-ld)
# The built .wasm files are committed, so this is only needed after changing
# a plugin or the SDK. Pass names to build some: scripts/build-example-plugins.sh hello
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/examples/plugins"
want="${*:-hello counter c-ping}"
for name in $want; do
  case "$name" in
    c-ping)
      sh "$name/build.sh"
      ;;
    hello|counter)
      (cd "$name" && npm install --no-audit --no-fund --silent && npm run --silent build)
      echo "built $name ($(wc -c < "$name/$name.wasm") bytes)"
      ;;
    *)
      echo "unknown example: $name" >&2
      exit 1
      ;;
  esac
done
