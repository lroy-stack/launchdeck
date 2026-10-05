// Zod schemas for everything that crosses the Tauri IPC boundary.
// They mirror the serde types in src-tauri (camelCase). The contract test
// parses examples/default-workspace.json, which Rust also guards against drift.
// One file per domain, without import cycles; everything is imported through
// this index, as "./schema" or "../lib/schema".

export * from "./actions";
export * from "./artifacts";
export * from "./context";
export * from "./feedback";
export * from "./observables";
export * from "./plugins";
export * from "./runtime";
export * from "./workspace";
