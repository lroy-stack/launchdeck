# Build from source

LaunchDeck is a Tauri 2 application: a Rust runtime (`src-tauri/`) and a React interface (`src/`). For 0.3.0 this is the way to run it.

## What you need

| | Version | Notes |
| --- | --- | --- |
| Rust | **1.97.0** | Pinned by `rust-toolchain.toml` with `rustfmt` and `clippy`; rustup installs it on first use. `Cargo.toml` says `rust-version = "1.95"`, but only 1.97.0 is built, formatted and linted here and in CI. |
| Node.js | `^20.19.0`, `^22.13.0` or `>=24.0.0` | From `engines` in `package.json`. Node 22 is what was used here and what CI uses. |
| System libraries | per system | The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/). |

Per system:

* **macOS**: the Xcode Command Line Tools (`xcode-select --install`). Built and tested here on macOS 15.4.1, Apple M2.
* **Linux** (Debian and Ubuntu package names, as CI installs them): `pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev libasound2-dev libxkbcommon-dev libxdo-dev libssl-dev`. Built in an earlier milestone; not built for 0.3.0 (NOT VERIFIED: no Linux desktop here, and CI does not run).
* **Windows**: the WebView2 runtime and the MSVC build tools. Never built (NOT VERIFIED: no Windows machine).

## Build and run

```bash
npm ci                 # the exact dependencies of package-lock.json
npm run tauri dev      # the app, with the simulator, rebuilt when a file changes
```

The first `tauri dev` compiles every Rust dependency and takes several minutes; later runs only rebuild what changed. The window opens with the [first-run screen](first-run.md).

To have the example plugins without installing them, name their folder:

```bash
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev
```

`LAUNCHDECK_PLUGINS` is a list of extra plugin folders, separated like `PATH` (`:` on macOS and Linux, `;` on Windows). LaunchDeck reads plugins from the `plugins` folder in its [data folder](install.md#where-launchdeck-keeps-its-data) first, then from these; it never writes into them. Without it, the examples Hello, Counter and Home Assistant can be installed from the **Plugins** screen.

The interface alone, in a browser, with a stand-in for the runtime (no devices, plugins or files):

```bash
npm run dev            # http://127.0.0.1:1420
```

## Check a change

```bash
npm run check          # typecheck, lint, architecture rules, versions and license,
                       # documentation links, Vitest, cargo fmt --check, cargo clippy -D warnings
npm run check:web      # the same without cargo
npm run check:full     # check, then every Rust test and the production web build
```

The run stops at the first step that fails and names it. `npm run hooks:install` turns on a pre-commit hook with the fast checks on what is staged. More on the test suites: [testing](../contributing/testing.md).

Every cargo command goes through `scripts/cargo.mjs`, which points cargo at `src-tauri/Cargo.toml` from anywhere in the repository. Set `LAUNCHDECK_CARGO` to a cargo binary or wrapper script to use another one.

## Build a bundle

```bash
npx tauri build --bundles app,dmg     # macOS: LaunchDeck.app and a .dmg
npx tauri build --bundles deb,rpm     # Linux
npx tauri build --bundles nsis        # Windows
```

The results are under `src-tauri/target/release/bundle/`. A release build needs several GB of free disk. None of these has been built here for 0.3.0 (NOT VERIFIED); the Bundle workflow that builds all three is written and has not run. How a release is made: [releasing](../contributing/releasing.md).

## Where things are

[AGENTS.md](../../AGENTS.md) and the [codebase map](../architecture/CODEBASE_MAP.md) say where code goes; the [architecture overview](../architecture/overview.md) says how the pieces fit.
