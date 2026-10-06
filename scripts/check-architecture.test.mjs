import { describe, expect, it } from "vitest";
import {
  checkFeatureMap,
  checkFileName,
  checkModuleMap,
  checkRustFile,
  checkWebFile,
  largeFileRows,
  maskRust,
  productionLines,
  run,
  rustReferences,
  stripRustTests,
} from "./check-architecture.mjs";

const rs = (file) => `src-tauri/src/${file}`;
const rules = (violations) => violations.map((v) => v.rule);

// An in-memory repository for run(): files by repo-relative path.
function memoryIo(files, { staged = [], stdin = "" } = {}) {
  const out = [];
  const err = [];
  return {
    root: "/repo",
    listFiles: (dir) => Object.keys(files).filter((f) => f.startsWith(`${dir}/`)),
    readFile: (file) => (file in files ? files[file] : null),
    stagedFiles: () => staged,
    readStaged: (file) => (file in files ? files[file] : null),
    readStdin: () => {
      if (stdin instanceof Error) throw stdin;
      return stdin;
    },
    out: (line) => out.push(line),
    err: (line) => err.push(line),
    printed: () => out.join("\n"),
    warned: () => err.join("\n"),
  };
}

const MAPS = {
  "docs/architecture/CODEBASE_MAP.md": "| `lib.rs` | root |\n| `context` | engine |\n| `core::model` | model |\n",
  "docs/architecture/FRONTEND_MAP.md": "| `src/features/editor` | inspector |\n",
};

describe("Rust import matrix", () => {
  it("fails a forbidden edge and passes an allowed one", () => {
    const bad = checkRustFile(rs("context/probe.rs"), "use crate::devices::DeviceInfo;\n");
    expect(rules(bad)).toEqual(["import-matrix"]);
    expect(bad[0]).toMatchObject({ file: rs("context/probe.rs"), line: 1 });
    expect(bad[0].message).toContain("crate::devices::DeviceInfo");

    expect(checkRustFile(rs("context/probe.rs"), "use crate::core::model::Workspace;\n")).toEqual([]);
    expect(checkRustFile(rs("core/runtime.rs"), "use crate::devices::DeviceInfo;\n")).toEqual([]);
    expect(checkRustFile(rs("lib.rs"), "use crate::devices::SurfaceFrame;\n")).toEqual([]);
  });

  it("expands grouped imports and finds inline paths", () => {
    const grouped = "use crate::{\n    core::runtime::Runtime,\n    state::value::Scalar,\n};\n";
    const found = checkRustFile(rs("state/probe.rs"), grouped);
    expect(rules(found)).toEqual(["import-matrix"]);
    expect(found[0].line).toBe(2);

    const inline = "fn f() {\n    let _ = crate::plugins::PluginRegistry::new();\n}\n";
    expect(checkRustFile(rs("state/probe.rs"), inline).map((v) => v.line)).toEqual([2]);
  });

  it("resolves super paths against the file's module", () => {
    expect(rules(checkRustFile(rs("agents/bridge.rs"), "use super::visual::Look;\n"))).toEqual(["import-matrix"]);
    expect(checkRustFile(rs("agents/bridge.rs"), "use super::model::*;\nuse super::adapter::*;\n")).toEqual([]);
    expect(checkRustFile(rs("surfaces/agent_control.rs"), "use super::*;\n")).toEqual([]);
  });

  it("lets every module import platform", () => {
    expect(checkRustFile(rs("context/identity.rs"), "use crate::platform::Platform;\n")).toEqual([]);
    expect(checkRustFile(rs("devices/manager.rs"), "use crate::platform::Platform;\n")).toEqual([]);
  });

  it("lets plugins use the audit trail only through crate::audit", () => {
    expect(checkRustFile(rs("plugins/external/host.rs"), "use crate::audit::{Actor, AuditEntry};\n")).toEqual([]);
    expect(rules(checkRustFile(rs("plugins/external/host.rs"), "use crate::agents::audit::AuditEntry;\n"))).toEqual([
      "import-matrix",
    ]);
  });

  it("lets only lib.rs and commands import artifacts", () => {
    const use = "use crate::artifacts::inspect;\n";
    expect(checkRustFile(rs("lib.rs"), use)).toEqual([]);
    expect(checkRustFile(rs("commands/artifacts.rs"), use)).toEqual([]);
    expect(rules(checkRustFile(rs("core/runtime.rs"), use))).toEqual(["import-matrix"]);
    expect(rules(checkRustFile(rs("artifacts/export.rs"), "use crate::core::runtime::Runtime;\n"))).toEqual([
      "import-matrix",
    ]);
    expect(checkRustFile(rs("artifacts/service.rs"), "use crate::core::runtime::Runtime;\n")).toEqual([]);
  });

  it("skips test-only files", () => {
    expect(checkRustFile(rs("agents/tests.rs"), "use crate::devices::DeviceInfo;\n")).toEqual([]);
    expect(checkRustFile(rs("plugins/external/state_tests.rs"), "use crate::devices::DeviceInfo;\n")).toEqual([]);
    expect(checkRustFile(rs("plugins/external/testkit.rs"), "use crate::devices::DeviceInfo;\n")).toEqual([]);
    expect(checkRustFile("src-tauri/tests/context_engine.rs", "use launchdeck_lib::devices::DeviceInfo;\n")).toEqual([]);
  });
});

describe("cfg(test) stripping and masking", () => {
  const source = [
    "use crate::core::model::Workspace; // crate::devices in a comment",
    'const S: &str = "crate::devices::DeviceInfo }";',
    "const C: char = '}';",
    "fn f<'a>(x: &'a str) -> &'a str { x }",
    "#[cfg(test)]",
    "mod tests {",
    "    use crate::devices::DeviceInfo;",
    '    fn g() { let _ = "}"; }',
    "}",
    "#[cfg(test)]",
    "use crate::renderer::render;",
    "pub fn after() -> u8 { 1 }",
    "",
  ].join("\n");

  it("removes test modules, test items, comments and literals", () => {
    expect(checkRustFile(rs("context/probe.rs"), source)).toEqual([]);
    const { code } = stripRustTests(maskRust(source));
    expect(code).not.toContain("devices");
    expect(code).not.toContain("renderer");
    expect(code).toContain("pub fn after()");
    expect(code.split("\n")).toHaveLength(source.split("\n").length);
  });

  it("counts production lines without the tests", () => {
    expect(productionLines(rs("context/probe.rs"), source)).toBe(5);
    expect(productionLines(rs("agents/tests.rs"), source)).toBe(0);
    expect(productionLines("src/lib/x.ts", "a\nb\nc\n")).toBe(3);
    expect(productionLines("src/lib/x.test.ts", "a\nb\nc\n")).toBe(0);
  });

  it("handles raw strings, nested comments and lifetimes", () => {
    const tricky = 'let a = r#"crate::devices " fine"#;\n/* outer /* crate::devices */ still */\nuse crate::core::model::X;\n';
    const refs = rustReferences(stripRustTests(maskRust(tricky)).code, rs("context/probe.rs"));
    expect(refs.modules.map((m) => m.path.join("::"))).toEqual(["core::model::X"]);
    expect(refs.modules[0].line).toBe(3);
  });
});

describe("crate confinement", () => {
  it("allows a crate only in its adapter file", () => {
    expect(rules(checkRustFile(rs("context/rules.rs"), "use tauri::Manager;\n"))).toEqual(["crate-confinement"]);
    expect(checkRustFile(rs("lib.rs"), "use tauri::Manager;\n")).toEqual([]);
    expect(checkRustFile(rs("devices/midi/system.rs"), "fn f() { let _ = midir::MidiOutput::new(\"x\"); }\n")).toEqual([]);
    expect(rules(checkRustFile(rs("devices/manager.rs"), "fn f() { let _ = midir::MidiOutput::new(\"x\"); }\n"))).toEqual([
      "crate-confinement",
    ]);
    expect(rules(checkRustFile(rs("devices/launchpad_x.rs"), "use midir::MidiInput;\n"))).toEqual(["crate-confinement"]);
    expect(checkRustFile(rs("context/observer/macos.rs"), "use objc2_app_kit::NSWorkspace;\n")).toEqual([]);
    expect(rules(checkRustFile(rs("context/identity.rs"), "use objc2_app_kit::NSWorkspace;\n"))).toEqual([
      "crate-confinement",
    ]);
  });

  it("ignores crates used in tests and look-alike names", () => {
    const test = "#[cfg(test)]\nmod tests {\n    use tokio_tungstenite::connect_async;\n}\n";
    expect(checkRustFile(rs("core/runtime.rs"), test)).toEqual([]);
    expect(checkRustFile(rs("core/runtime.rs"), "use crate::tauri_like::x;\nfn tauri() {}\n")).toEqual([]);
  });
});

describe("platform cfg placement", () => {
  it("allows target_os and windows cfg only in the listed files", () => {
    const os = 'fn f() -> bool { cfg!(target_os = "macos") }\n';
    expect(rules(checkRustFile(rs("state/probe.rs"), os))).toEqual(["platform-cfg"]);
    expect(rules(checkRustFile(rs("state/probe.rs"), "#[cfg(windows)]\nfn f() {}\n"))).toEqual(["platform-cfg"]);
    expect(checkRustFile(rs("context/observer/mod.rs"), os)).toEqual([]);
    expect(checkRustFile(rs("platform.rs"), os)).toEqual([]);
    expect(checkRustFile(rs("plugins/keyboard.rs"), os)).toEqual([]);
  });

  it("allows cfg(unix) anywhere and target_os nowhere else, not even in the former exceptions", () => {
    expect(checkRustFile(rs("state/probe.rs"), "#[cfg(unix)]\nfn perms() {}\n")).toEqual([]);
    const os = 'fn f() -> bool { cfg!(target_os = "macos") }\n';
    for (const file of ["surfaces/mod.rs", "context/identity.rs", "plugins/system.rs"]) {
      expect(rules(checkRustFile(rs(file), os))).toEqual(["platform-cfg"]);
    }
    expect(checkRustFile(rs("main.rs"), '#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]\n')).toEqual(
      [],
    );
  });
});

describe("file names", () => {
  it("fails dumping-ground names", () => {
    expect(rules(checkFileName("src/lib/utils.ts"))).toEqual(["dumping-ground"]);
    expect(rules(checkFileName(rs("helpers/mod.rs")))).toEqual(["dumping-ground"]);
    expect(rules(checkFileName("src/components/Common/Button.tsx"))).toEqual(["dumping-ground"]);
    expect(rules(checkFileName(rs("state/misc.rs")))).toEqual(["dumping-ground"]);
    expect(checkFileName("src/lib/format.ts")).toEqual([]);
    expect(checkFileName("scripts/utils.mjs")).toEqual([]);
  });
});

describe("frontend rules", () => {
  it("fails a feature edge outside the list", () => {
    const bad = checkWebFile("src/features/plugins/X.tsx", 'import { AgentsView } from "../agents/AgentsView";\n');
    expect(rules(bad)).toEqual(["feature-edge"]);
    expect(bad[0].message).toContain("plugins");
    expect(checkWebFile("src/features/surfaces/X.tsx", 'import { Inspector } from "../editor/Inspector";\n')).toEqual([]);
    expect(checkWebFile("src/app/App.tsx", 'import { PluginsView } from "../features/plugins/PluginsView";\n')).toEqual([]);
  });

  it("allows a tests-only edge from test files only", () => {
    const use = 'import { SurfaceView } from "../surfaces/SurfaceView";\n';
    expect(checkWebFile("src/features/feedback/testkit.tsx", use)).toEqual([]);
    expect(checkWebFile("src/features/library/library.test.tsx", use)).toEqual([]);
    expect(rules(checkWebFile("src/features/feedback/FeedbackEditor.tsx", use))).toEqual(["feature-edge"]);
  });

  it("keeps the seeded exceptions to their files and targets", () => {
    const padGrid = 'import { PadGrid } from "../surfaces/PadGrid";\n';
    expect(checkWebFile("src/features/agents/SurfaceTab.tsx", padGrid)).toEqual([]);
    expect(rules(checkWebFile("src/features/agents/Overview.tsx", padGrid))).toEqual(["feature-edge"]);
    expect(checkWebFile("src/components/Sidebar.tsx", 'import { ContextControl } from "../features/context/ContextControl";\n')).toEqual([]);
    expect(rules(checkWebFile("src/components/Other.tsx", 'import { ContextControl } from "../features/context/ContextControl";\n'))).toEqual([
      "layer",
    ]);
  });

  it("confines @tauri-apps and preview.ts to ipc.ts", () => {
    const dynamic = 'const m = await import("@tauri-apps/api/core");\n';
    expect(checkWebFile("src/lib/ipc.ts", `${dynamic}const p = await import("./preview");\n`)).toEqual([]);
    expect(rules(checkWebFile("src/lib/other.ts", dynamic))).toEqual(["tauri-import"]);
    expect(rules(checkWebFile("src/stores/runtime.ts", 'import { x } from "../lib/preview";\n'))).toEqual(["preview-import"]);
    expect(checkWebFile("src/lib/preview.test.ts", 'import { createPreviewTransport } from "./preview";\n')).toEqual([]);
  });

  it("keeps the shared layers below the features", () => {
    expect(rules(checkWebFile("src/lib/x.ts", 'import { useState } from "react";\n'))).toEqual(["layer"]);
    expect(rules(checkWebFile("src/lib/x.ts", 'import { useStore } from "../stores/runtime";\n'))).toEqual(["layer"]);
    expect(rules(checkWebFile("src/stores/x.ts", 'import { A } from "../features/agents/AgentsView";\n'))).toEqual([
      "layer",
    ]);
    expect(checkWebFile("src/stores/runtime.ts", 'import { api } from "../lib/ipc";\nimport { create } from "zustand";\n')).toEqual([]);
    expect(checkWebFile("src/lib/x.ts", '// import { useState } from "react";\nconst s = "from \\"react\\"";\n')).toEqual([]);
  });
});

describe("map rows", () => {
  it("fails a lib.rs module missing from CODEBASE_MAP", () => {
    const lib = "pub mod context;\npub mod core;\nmod gizmo;\n#[cfg(test)]\nmod probe;\n";
    const found = checkModuleMap(lib, MAPS["docs/architecture/CODEBASE_MAP.md"]);
    expect(rules(found)).toEqual(["map-row"]);
    expect(found[0]).toMatchObject({ line: 3 });
    expect(found[0].message).toContain("gizmo");
  });

  it("fails a feature directory missing from FRONTEND_MAP", () => {
    const found = checkFeatureMap(["editor", "gizmo"], MAPS["docs/architecture/FRONTEND_MAP.md"]);
    expect(rules(found)).toEqual(["map-row"]);
    expect(found[0].file).toBe("src/features/gizmo");
  });
});

describe("large files", () => {
  const allowlist = [
    { file: rs("big.rs"), baseline: 800, reason: "one concept" },
    { file: rs("gone.rs"), baseline: 900, reason: "split since" },
  ];

  it("classifies watch, allowlisted, growth and missing entries", () => {
    const rows = largeFileRows(
      [
        { file: rs("small.rs"), lines: 120 },
        { file: rs("watch.rs"), lines: 450 },
        { file: rs("big.rs"), lines: 900 },
        { file: rs("huge.rs"), lines: 1000 },
      ],
      allowlist,
    );
    const byFile = Object.fromEntries(rows.map((r) => [r.file, r]));
    expect(byFile[rs("small.rs")]).toBeUndefined();
    expect(byFile[rs("watch.rs")].status).toBe("watch");
    expect(byFile[rs("big.rs")]).toMatchObject({ status: "allowlisted", growth: 12.5 });
    expect(byFile[rs("big.rs")].notes.join(" ")).toContain("grew");
    expect(byFile[rs("huge.rs")].status).toBe("needs allowlist entry");
    expect(byFile[rs("gone.rs")].status).toBe("stale allowlist entry");
  });

  it("never fails the report", () => {
    const big = Array.from({ length: 1500 }, (_, i) => `fn f${i}() {}`).join("\n");
    const io = memoryIo({ ...MAPS, [rs("lib.rs")]: "pub mod context;\n", [rs("context/huge.rs")]: big });
    expect(run(["--report"], io)).toBe(0);
    expect(io.printed()).toContain("context/huge.rs");
    expect(io.printed()).toContain("needs allowlist entry");
  });
});

describe("modes", () => {
  const tree = {
    ...MAPS,
    [rs("lib.rs")]: "pub mod context;\npub mod core;\n",
    [rs("context/rules.rs")]: "use crate::core::model::Workspace;\n",
    "src/features/editor/Inspector.tsx": 'import { api } from "../../lib/ipc";\n',
  };

  it("passes a clean tree and fails a planted violation", () => {
    expect(run([], memoryIo(tree))).toBe(0);
    const io = memoryIo({ ...tree, [rs("context/zz_probe.rs")]: "use crate::devices::DeviceInfo;\n" });
    expect(run([], io)).toBe(1);
    expect(io.printed()).toContain("src-tauri/src/context/zz_probe.rs:1");
  });

  it("limits the check with --only", () => {
    const io = memoryIo({ ...tree, [rs("context/zz_probe.rs")]: "use crate::devices::DeviceInfo;\n" });
    expect(run(["--only=web"], io)).toBe(0);
    expect(run(["--only=rust"], io)).toBe(1);
  });

  it("checks only staged files with --staged", () => {
    const files = { ...tree, [rs("context/zz_probe.rs")]: "use crate::devices::DeviceInfo;\n" };
    expect(run(["--staged"], memoryIo(files, { staged: [rs("context/rules.rs"), "README.md"] }))).toBe(0);
    expect(run(["--staged"], memoryIo(files, { staged: [rs("context/zz_probe.rs")] }))).toBe(1);
  });

  it("exits 2 with one paragraph in --hook mode on a violation, else 0", () => {
    const files = { ...tree, [rs("context/zz_probe.rs")]: "use crate::devices::DeviceInfo;\n" };
    const hook = (path, extra = {}) => memoryIo(files, { stdin: JSON.stringify({ tool_input: { file_path: path } }), ...extra });

    const bad = hook("/repo/src-tauri/src/context/zz_probe.rs");
    expect(run(["--hook"], bad)).toBe(2);
    expect(bad.warned()).toContain("crate::devices::DeviceInfo");
    expect(bad.warned().trim().split("\n")).toHaveLength(1);

    expect(run(["--hook"], hook("/repo/src-tauri/src/context/rules.rs"))).toBe(0);
    expect(run(["--hook"], hook("/repo/README.md"))).toBe(0);
    expect(run(["--hook"], hook("/elsewhere/src/context/zz_probe.rs"))).toBe(0);
    expect(run(["--hook"], hook("/repo/src-tauri/src/context/deleted.rs"))).toBe(0);
    expect(run(["--hook"], memoryIo(files, { stdin: "not json" }))).toBe(0);
    expect(run(["--hook"], memoryIo(files, { stdin: new Error("closed") }))).toBe(0);
    expect(run(["--hook"], memoryIo(files, { stdin: "{}" }))).toBe(0);
  });

  it("checks the map row when the hook sees lib.rs or a new feature", () => {
    const files = { ...tree, [rs("lib.rs")]: "pub mod context;\npub mod gizmo;\n", "src/features/gizmo/View.tsx": "export {};\n" };
    const hook = (path) => memoryIo(files, { stdin: JSON.stringify({ tool_input: { file_path: path } }) });
    expect(run(["--hook"], hook("/repo/src-tauri/src/lib.rs"))).toBe(2);
    expect(run(["--hook"], hook("/repo/src/features/gizmo/View.tsx"))).toBe(2);
  });

  it("rejects an unknown argument", () => {
    const io = memoryIo(tree);
    expect(run(["--nope"], io)).toBe(2);
    expect(io.warned()).toContain("--nope");
  });
});
