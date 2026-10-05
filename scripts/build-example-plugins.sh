#!/bin/sh
# Builds the example plugin modules from source.
#
#   scripts/build-example-plugins.sh [--check] [name...]
#
# With no name it builds the reference examples, hello counter channels
# home-assistant: AssemblyScript through the SDK in sdk/assemblyscript. They
# need Node 20+ and npm 10+, and the npm registry (or npm's cache) for
# AssemblyScript. Dependencies come from the committed lockfiles through
# `npm ci`, which never rewrites them.
#
# Built only when named:
#   c-ping      plain C, no SDK. Needs clang with the wasm32 target and wasm-ld;
#               Apple's clang (Xcode, Command Line Tools) has neither, so on a
#               Mac use LLVM from Homebrew or build on Linux.
#   net-probe   the plugin LaunchDeck's own network tests use; not an example.
#
# --check builds each named example (default: the four above) in a temporary
# copy of the repository, leaving the committed files alone, and compares the
# SHA-256 of the module it built, and of the committed one, with
# examples/plugins/SHA256SUMS. It exits 1 on any difference. c-ping and
# net-probe have no recorded checksum.
#
# The built .wasm files are committed, so building is only needed after
# changing a plugin, the SDK or a lockfile. Commit the new module together with
# its line in SHA256SUMS (the build prints the checksum).
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
examples="$root/examples/plugins"
sums="$examples/SHA256SUMS"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

# Installs from the lockfile and builds, in the folder given.
build_as() {
  (cd "$1" && npm ci --no-audit --no-fund --loglevel=error && npm run --silent build)
}

# Copies a folder's sources, without node_modules or built modules.
copy_sources() {
  mkdir -p "$2"
  (cd "$1" && tar -cf - --exclude node_modules --exclude '*.wasm' .) | (cd "$2" && tar -xf -)
}

build() {
  case "$1" in
    c-ping)
      sh "$examples/c-ping/build.sh"
      ;;
    hello|counter|channels|home-assistant)
      build_as "$examples/$1"
      echo "built $1 ($(wc -c < "$examples/$1/$1.wasm" | tr -d ' ') bytes, sha256 $(sha256 "$examples/$1/$1.wasm"))"
      ;;
    net-probe)
      build_as "$root/src-tauri/tests/fixtures/plugins/net-probe"
      echo "built net-probe"
      ;;
    *)
      echo "unknown example: $1" >&2
      exit 1
      ;;
  esac
}

# Prints "ok" or what differs; returns 1 on a difference.
check() {
  case "$1" in
    hello|counter|channels|home-assistant) ;;
    c-ping|net-probe)
      echo "$1: not checked, it has no recorded checksum" >&2
      return 1
      ;;
    *)
      echo "unknown example: $1" >&2
      return 1
      ;;
  esac
  file="$1/$1.wasm"
  expected="$(awk -v f="$file" '$2 == f { print $1 }' "$sums")"
  if [ -z "$expected" ]; then
    echo "$1: no checksum for $file in examples/plugins/SHA256SUMS" >&2
    return 1
  fi
  copy="$tmp/examples/plugins/$1"
  copy_sources "$examples/$1" "$copy"
  if ! build_as "$copy" >/dev/null; then
    echo "$1: the build failed" >&2
    return 1
  fi
  if [ ! -f "$copy/$1.wasm" ]; then
    echo "$1: the build did not write $1.wasm" >&2
    return 1
  fi
  built="$(sha256 "$copy/$1.wasm")"
  committed="$(sha256 "$examples/$file")"
  status=0
  if [ "$built" != "$expected" ]; then
    echo "$1: the build gives $built, SHA256SUMS says $expected" >&2
    status=1
  fi
  if [ "$committed" != "$expected" ]; then
    echo "$1: the committed $file is $committed, SHA256SUMS says $expected" >&2
    status=1
  fi
  [ "$status" -eq 0 ] && echo "ok $1 $built"
  return "$status"
}

mode=build
if [ "$1" = "--check" ]; then
  mode=check
  shift
fi
[ $# -gt 0 ] || set -- hello counter channels home-assistant

if [ "$mode" = check ]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  # The examples depend on the SDK by a relative path; keep the layout.
  copy_sources "$root/sdk/assemblyscript" "$tmp/sdk/assemblyscript"
  failed=0
  for name in "$@"; do
    check "$name" || failed=1
  done
  exit "$failed"
fi

for name in "$@"; do
  build "$name"
done
