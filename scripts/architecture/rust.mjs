// The Rust rules of the architecture check (scripts/check-architecture.mjs), as data.
//
// This file and docs/architecture/CODEBASE_MAP.md section 5 hold the same rules:
// change both in one commit. The web rules live in web.mjs, so that a Rust package
// and a TypeScript package running at the same time edit different files.
//
// Paths are relative to src-tauri/src. A module path is written `a::b` and covers
// everything below it (`core::model` covers `core::model::Workspace`). Globs: `**`
// is any number of directories, `*` part of one name.

export const root = "src-tauri/src";

// Files that hold only tests; nothing in them is checked or counted.
export const testFiles = ["**/tests.rs", "**/*_tests.rs", "**/testkit.rs"];

// Modules every module may import.
export const importableEverywhere = ["platform"];

// The forbidden-import matrix of PRODUCTIZATION_PLAN.md section 5.1, for non-test
// code. `only`: the files may import nothing else from the crate. `forbid`: the
// files may not import these. `core::runtime`, `lib.rs` and `commands/**` may
// import anything, so no rule names them.
export const importRules = [
  { files: ["context/**"], only: ["context", "core::model"] },
  { files: ["devices/**"], only: ["devices", "core::model"] },
  { files: ["renderer/**"], only: ["renderer", "devices", "core::model"] },
  { files: ["events/**"], only: ["events"] },
  {
    files: ["state/**"],
    forbid: ["devices", "renderer", "core::runtime", "plugins", "agents", "context", "surfaces", "persistence"],
  },
  {
    files: ["core/model.rs", "core/validate.rs"],
    forbid: ["core::runtime", "devices", "renderer", "plugins", "agents", "persistence", "events"],
  },
  {
    files: ["actions/**"],
    forbid: ["devices", "renderer", "core::runtime", "agents", "context", "surfaces", "persistence"],
  },
  { files: ["plugins/**"], forbid: ["devices", "renderer", "core::runtime", "context", "surfaces", "agents"] },
  { files: ["agents/**"], forbid: ["devices", "renderer", "core::runtime", "context", "surfaces"] },
  { files: ["agents/adapters/**", "agents/bridge.rs"], forbid: ["core::model", "agents::visual", "state::feedback"] },
  {
    files: ["surfaces/**"],
    forbid: ["devices", "renderer", "core::runtime", "plugins::external", "persistence", "context::engine"],
  },
  { files: ["persistence/**"], forbid: ["core::runtime", "devices", "renderer", "context", "plugins::external"] },
  {
    files: ["artifacts/**"],
    except: ["artifacts/service.rs"],
    only: [
      "artifacts",
      "core::model",
      "core::validate",
      "state::feedback",
      "state::address",
      "state::value",
      "actions::definition",
      "actions::params",
      "context::identity",
      "context::rules",
      "context::portable",
      "devices",
      "plugins::external::manifest",
      "plugins::system",
    ],
  },
  {
    files: ["artifacts/service.rs"],
    only: [
      "artifacts",
      "core::model",
      "core::validate",
      "core::runtime",
      "persistence",
      "state::feedback",
      "state::address",
      "state::value",
      "actions::definition",
      "actions::params",
      "context::identity",
      "context::rules",
      "context::portable",
      "devices",
      "plugins::external::manifest",
      "plugins::system",
    ],
  },
];

// Modules only some files may import.
export const restrictedModules = [{ module: "artifacts", importers: ["lib.rs", "commands/**", "artifacts/**"] }];

// Known violations, each removed by the package named. Nothing else may be added
// here without a planned fix in PRODUCTIZATION_PLAN.md. Entries have the shape
// { files: [...], imports: "a::b", removedBy: "<package>" }.
export const importExceptions = [];

// Each outside system in exactly one adapter file (R4). Crate names as written in
// Rust paths; `objc2_*` covers objc2_app_kit and objc2_foundation.
export const crateConfinement = [
  { crates: ["midir"], files: ["devices/midi/system.rs"] },
  { crates: ["wasmtime"], files: ["plugins/external/sandbox.rs"] },
  { crates: ["keyring"], files: ["plugins/external/secrets.rs"] },
  { crates: ["enigo"], files: ["plugins/keyboard.rs"] },
  { crates: ["objc2", "block2", "objc2_*", "windows_sys", "x11rb"], files: ["context/observer/**"] },
  { crates: ["tokio_tungstenite"], files: ["events/external.rs"] },
  { crates: ["ureq"], files: ["actions/builtin.rs", "plugins/external/network.rs"] },
  { crates: ["tauri"], files: ["lib.rs", "commands/**", "tray.rs"] },
  { crates: ["semver", "sha2"], files: ["artifacts/**", "plugins/external/manifest.rs"] },
];

// Where `cfg!(target_os…)`, `#[cfg(target_os…)]` and `cfg(windows)` may appear (R3).
// `cfg(unix)` (file permissions) is allowed anywhere.
export const platformCfg = {
  files: [
    "platform.rs",
    "context/observer/**",
    "plugins/external/secrets.rs",
    "plugins/keyboard.rs",
    "actions/builtin.rs",
  ],
  // Known violations, shaped { files: [...], removedBy: "<package>" }; none today.
  exceptions: [],
};

// Every `mod` in lib.rs needs a row in this map (R1).
export const moduleMap = { crateRoot: "lib.rs", map: "docs/architecture/CODEBASE_MAP.md" };

// Files of 700 production lines or more need an entry here (R14, advisory only).
// `baseline` is the production line count (tests excluded) when the entry was made;
// growth above 10 % is reported. Remove an entry when its file is split.
export const largeFiles = [
  { file: "plugins/external/manifest.rs", baseline: 934, reason: "one concept, strict manifest parsing for schemas 1-4; kept whole (plan section 2)" },
  { file: "plugins/external/network.rs", baseline: 827, reason: "the security-critical network broker, one flow; kept whole (plan section 2)" },
  { file: "plugins/external/sandbox.rs", baseline: 771, reason: "the only Wasmtime file, on purpose; kept whole (plan section 2)" },
  { file: "state/feedback.rs", baseline: 621, reason: "one concept, FeedbackRule and its gate; kept whole (plan section 2)" },
  { file: "state/hub.rs", baseline: 502, reason: "one concept, the StateHub; kept whole (plan section 2)" },
  { file: "context/engine.rs", baseline: 353, reason: "one state machine, Auto and Manual Override; kept whole (plan section 2)" },
  { file: "agents/bridge.rs", baseline: 628, reason: "one adapter, the Agent Bridge protocol; kept whole (plan section 2)" },
  { file: "agents/model.rs", baseline: 521, reason: "one concept, the Agent Control domain types; kept whole (plan section 2)" },
  { file: "agents/demo.rs", baseline: 747, reason: "scripted demo data; kept whole (plan section 2)" },
  { file: "agents/service.rs", baseline: 1970, reason: "one state machine with clear sections; split deferred until next edited (plan section 4)" },
  { file: "plugins/external/capabilities.rs", baseline: 726, reason: "grants and permissions.json, security-critical and cohesive; split deferred (plan section 4)" },
  { file: "plugins/external/host.rs", baseline: 1182, reason: "PluginHost and plugins.json; D1 moves the legacy secrets code to legacy_secrets.rs" },
];
