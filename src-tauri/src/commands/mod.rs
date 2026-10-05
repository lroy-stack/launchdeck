//! The Tauri commands, one file per feature. Thin wrappers: the runtime owns all
//! behaviour. Each command's name, argument names and return type are the IPC
//! contract with `src/lib/ipc.ts`; `lib.rs` registers them in `generate_handler!`.

pub(crate) mod actions;
pub(crate) mod agents;
pub(crate) mod app;
pub(crate) mod artifacts;
pub(crate) mod context;
pub(crate) mod devices;
pub(crate) mod feedback;
pub(crate) mod plugins;
pub(crate) mod workspace;

pub(crate) type CmdResult<T> = Result<T, String>;
