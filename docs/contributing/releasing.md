# Releasing

How a LaunchDeck release is made, in order. It was written for 0.3.0, the first external alpha; the release record of 0.3.0 is the first one made this way. What each version number means, and who may change it, is in [versions](../architecture/versions.md).

A release is the owner's decision. An agent or a contributor may prepare every step up to the tag, but creates a tag only when the owner asks, and never pushes a tag or a branch.

## 1. The version

The app's version has one source: `version` in `src-tauri/Cargo.toml`. Change it there and in the places that copy it, in one commit (`chore: version x.y.z`):

1. `src-tauri/Cargo.toml`, `[package] version`.
2. `package.json`, `version`. `src-tauri/tauri.conf.json` has no version, so Tauri reads Cargo.toml.
3. The lock files: `src-tauri/Cargo.lock` (the `launchdeck` package entry, updated by the next cargo build) and `package-lock.json` (its two root `version` fields, updated by `npm install --package-lock-only`).
4. The contract fixtures that carry the version, regenerated, never edited by hand: `examples/app-info.json` (`dump_app_info`), `examples/action-catalog.json` (`dump_catalog`) and `examples/observables.json` (`dump_observables`). See [testing](testing.md#contract-fixtures). Other version strings under `examples/` belong to the example plugins and stay as they are.
5. The table in [versions](../architecture/versions.md).

`npm run versions:check` fails when `package.json` and `Cargo.toml` differ, when `tauri.conf.json` has a version, or when any license declaration is not MIT. The SDK, ABI, manifest, workspace, bridge and artifact numbers change only with the contract they describe, never because the app's version did.

## 2. The CHANGELOG

[CHANGELOG.md](../../CHANGELOG.md) follows Keep a Changelog 1.1.0, grouped by capability. The section `[Unreleased] — x.y.z` collects the release while it is prepared. At release:

* Give it its date and the release-candidate commit.
* Make every "Run here" and every NOT VERIFIED match the release record (step 5): nothing is called tested on a system, device or service it was not run on, and each NOT VERIFIED keeps its reason.
* Refresh the test counts from the release candidate's own run.

## 3. The gate

On the release candidate commit, on a clean tree:

```bash
npm ci
npm run check:full
```

`check:full` runs typecheck, lint, the architecture rules, versions and license, documentation links, Vitest, cargo fmt and clippy, every Rust test and the production web build. Then, because they guard the release's promises, run these more than once (five times each for 0.3.0): `cargo test --test artifacts_security`, `--test artifacts_roundtrip` and `--test release_acceptance`. Also: `npm run plugins:validate -- <folder>` on each example plugin, `sh scripts/build-example-plugins.sh --check` (needs npm to reach the registry), and actionlint on `.github/workflows/`.

## 4. The bundles

On macOS, with several GB of free disk and no other cargo process running:

```bash
CI=true npx tauri build --bundles app,dmg
plutil -p src-tauri/target/release/bundle/macos/LaunchDeck.app/Contents/Info.plist
codesign -dv --verbose src-tauri/target/release/bundle/macos/LaunchDeck.app
hdiutil verify src-tauri/target/release/bundle/dmg/*.dmg
```

`CI=true` matters on a Mac someone is using: without it Tauri's `bundle_dmg.sh` runs an AppleScript that tells Finder to lay out the disk image's window, and on 0.3.0 that step failed the `.dmg` ([release record](../verification/release-0.3.0.md#the-macos-bundle)). With 8 GB of RAM, `CARGO_BUILD_JOBS=2` keeps the release build (LTO, one codegen unit) out of swap; it took about 4 to 8 minutes here.

Check that the bundle identifier is `dev.launchdeck.app`, the version is the release's, and `Contents/Resources/examples/plugins` holds the three bundled example plugins (Hello, Counter, Home Assistant). The macOS build is ad-hoc signed and not notarized; [install](../getting-started/install.md#opening-a-build-on-macos) says what a person sees.

The Linux `.deb` and `.rpm` and the Windows NSIS installer come from the Bundle workflow (`.github/workflows/bundle.yml`), started by hand or by a `v*` tag; see [CI](ci.md). Until it has run, they are NOT VERIFIED and are not offered.

## 5. The manual checks and the release record

A person installs the bundle built in step 4 on a Mac, with the data folder moved aside (never deleted), and walks the [manual checks](manual-checks-macos.md): the acceptance walk-through, then the window, tray, Auto Context, Keychain and Accessibility checks. What was seen and what was not is written down.

The release record goes in `docs/verification/release-x.y.z.md` and is listed in the [verification index](../verification/README.md): the commit, the machine, every command run with its result and counts, the bundle checks, the manual checks done and not done, and the NOT VERIFIED list with reasons. README's "Verified where" and the CHANGELOG are then updated from that record, and only from it.

## 6. The tag

Only when the owner asks: an annotated tag `vx.y.z` on the release-candidate commit, created locally. The owner pushes it. A pushed `v*` tag starts the Bundle workflow, whose artifacts are kept for 14 days. No workflow publishes a GitHub release: that is a manual step for the owner, marked as a pre-release while the channel is alpha.
