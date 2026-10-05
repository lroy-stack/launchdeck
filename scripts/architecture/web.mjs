// The frontend rules of the architecture check (scripts/check-architecture.mjs), as data.
//
// The feature edges below and docs/architecture/FRONTEND_MAP.md section 3 list the
// same edges: change both in one commit. The Rust rules live in rust.mjs, so that a
// Rust package and a TypeScript package running at the same time edit different files.
//
// Paths are relative to the repository root and written without a file extension
// when they name an import target. Globs: `**` is any number of directories, `*`
// part of one name.

export const root = "src";

// Test files: checked like any other file, never counted as production lines, and
// the only files a tests-only feature edge allows.
export const testFiles = ["**/*.test.ts", "**/*.test.tsx", "**/testkit.tsx"];

// R10: only the IPC bridge talks to Tauri, and only it uses the browser stand-in
// (its own tests aside, as FRONTEND_MAP section 1 says).
export const tauriImporters = ["src/lib/ipc.ts"];
export const preview = {
  module: "src/lib/preview",
  importers: ["src/lib/ipc.ts", "src/lib/preview.test.ts", "src/lib/plugins.test.ts"],
};

// R20: what the shared layers must not import. Bare names are packages, paths are
// directories of this repository.
export const layerRules = [
  { files: ["src/lib/**"], forbid: ["react", "react-dom", "src/features", "src/components", "src/stores"] },
  { files: ["src/stores/**"], forbid: ["src/features", "src/components"] },
  { files: ["src/components/**"], forbid: ["src/features"] },
];

// R20: the composition points, FRONTEND_MAP section 3. `src/app` may import every
// feature and is not listed. `testsOnly`: allowed from test files only.
export const featureEdges = [
  { from: "surfaces", to: "editor", where: "SurfaceView.tsx -> Inspector" },
  { from: "surfaces", to: "library", where: "SurfaceView.tsx -> ActionLibrary" },
  { from: "editor", to: "feedback", where: "Inspector.tsx -> FeedbackEditor, useReview" },
  { from: "editor", to: "library", where: "AssignmentEditor.tsx -> library/parts" },
  { from: "settings", to: "context", where: "SettingsView.tsx -> ContextPanel" },
  { from: "feedback", to: "surfaces", where: "testkit.tsx -> SurfaceView", testsOnly: true },
  { from: "library", to: "surfaces", where: "library.test.tsx -> SurfaceView", testsOnly: true },
];

// Known violations, each removed by the package named (FRONTEND_MAP section 3).
// One entry per importing file and imported module, so a new import still fails.
export const importExceptions = [
  // No import matches these two entries since A4 moved PadGrid to src/components and
  // Sidebar to src/app. They stay only because the exception test in
  // scripts/check-architecture.test.mjs uses them as its examples; remove them when
  // that test brings its own seeded rules.
  { file: "src/features/agents/SurfaceTab.tsx", imports: "src/features/surfaces/PadGrid", removedBy: "A4" },
  { file: "src/components/Sidebar.tsx", imports: "src/features/context/ContextControl", removedBy: "A4" },
];

// Every src/features/* directory needs a row in this map (R1).
export const featureMap = { features: "src/features", map: "docs/architecture/FRONTEND_MAP.md" };

// Files of 700 production lines or more need an entry here (R14, advisory only).
// `baseline` is the line count when the entry was made; growth above 10 % is
// reported. Remove an entry when its file is split.
export const largeFiles = [
  { file: "src/lib/schema.ts", baseline: 814, reason: "every IPC schema in one file; split into src/lib/schema/ planned (A4)" },
  { file: "src/lib/feedbackEdit.ts", baseline: 804, reason: "a pure, cohesive editing model; kept whole (plan section 2)" },
];
