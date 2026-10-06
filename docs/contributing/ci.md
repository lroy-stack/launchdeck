# Continuous integration

Three GitHub Actions workflows live in `.github/workflows/`. **None of them has ever run** (NOT VERIFIED): GitHub Actions does not start jobs for this repository because of the account's billing. The one attempt, run `37301909131` on 2026-10-05, ended with "The job was not started because recent account payments have failed or your spending limit needs to be increased" before any job was evaluated ([record](../verification/macos-2026-10-05.md#section-5-continuous-integration)). What is written below is what the files say; nothing about their results is known.

What has been checked: the YAML parses and `actionlint` reported nothing on `ci.yml` (2026-10-05). Every gate the workflows run is also an npm script, and those scripts run here on macOS.

## The workflows

| Workflow | Runs on | Triggers | What it does |
| --- | --- | --- | --- |
| `ci.yml`, job `test` | `ubuntu-24.04`; also `macos-latest` and `windows-latest` (Windows may fail without failing the run) | Linux alone on a push to `main`; all three on a pull request and when started by hand | `npm ci`, then one step per gate: architecture, versions, docs links, typecheck, lint, Rust format, clippy, Vitest, the web build, the Rust tests. Linux only, after the tests: the real Secret Service test (`scripts/test-secret-service.sh`) and the SDK contract (the example plugins and the network test plugin rebuilt with the current SDK, the plugin tests run again, and a `git diff --stat` that informs and never fails) |
| `bundle.yml` | the same three systems | a `v*` tag, or started by hand | `npx tauri build`: `.deb` and `.rpm` (inspected with `dpkg-deb` and `rpm -qip`), `.app` and `.dmg` (inspected with `codesign -dv` and `hdiutil verify`), NSIS on Windows (may fail). The packages are uploaded as artifacts and kept 14 days. Nothing is signed with a certificate; no secret is used |
| `desktop-e2e.yml` | `ubuntu-24.04` | started by hand, and Mondays at 05:23 UTC | A release build without bundles, `tauri-driver`, then every suite of `tools/desktop-e2e/run.py` (acceptance, context, feedback, home_assistant, secrets) under Xvfb with a window manager and a throwaway keyring; screenshots and logs kept on failure |

All three read the repository only (`permissions: contents: read`), cancel a run superseded by a newer one on the same ref, pin Rust 1.97.0 to match `rust-toolchain.toml`, use Node 22, and cache the Rust build with `Swatinem/rust-cache`.

`ubuntu-24.04` is pinned rather than `ubuntu-latest`, which moves to 26.04 in November 2026: nothing has been built against its WebKitGTK yet.

## Why the triggers are split

Minutes on macOS and Windows runners cost several times a Linux minute, and the repository is private, so they are billed. A push to `main` therefore runs Linux only; macOS and Windows run on pull requests and by hand. Bundles and the desktop suites are slow (a release build from a cold cache, then minutes of suites), so they run by hand, on a tag, or weekly.

## What a green run would and would not show

* Linux: the whole gate, the real Secret Service and the plugin SDK contract. Nothing in 0.3.0 has been built on Linux, so a first run may find real failures.
* macOS: the same gate on macOS 26 on Apple silicon (this repository has been built here on macOS 15.4.1 only).
* Windows: LaunchDeck has never been built for Windows. The first run is expected to list failures; `continue-on-error` keeps them visible without blocking the other systems until they are fixed. Remove it once Windows is green.
* The desktop suites run Linux only: `tauri-driver` does not support macOS, so the app window on macOS is checked by a person ([manual checks](manual-checks-macos.md)).
* No workflow touches hardware, a real Home Assistant or the macOS Keychain.

## When Actions can run again

1. Start `ci.yml` by hand on the branch (or open a pull request), then `bundle.yml` and `desktop-e2e.yml` by hand.
2. Record the results under [docs/verification/](../verification/README.md), with run ids.
3. Remove NOT VERIFIED (reason (a)) only for what actually ran and passed: in README "Verified where", the CHANGELOG and the user guides.
