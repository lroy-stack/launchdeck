#!/bin/sh
# Builds ping.wasm with clang and wasm-ld. No other dependency.
set -e
cd "$(dirname "$0")"
clang --target=wasm32 -nostdlib -O2 -Wl,--no-entry -Wl,--strip-all -o ping.wasm ping.c
echo "built ping.wasm ($(wc -c < ping.wasm) bytes)"
