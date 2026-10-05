//! LaunchDeck: a programmable physical interface runtime for MIDI grid controllers.
//!
//! Modules (who owns what and who may import whom: docs/architecture/CODEBASE_MAP.md):
//! * `core`        the workspace model and its validation, the runtime (composition root),
//!   feedback review and explanation, the runtime's own states and events
//! * `state`       states and feedback: values, definitions, rules, evaluation, the state hub
//! * `actions`     semantic actions: assignments, parameters, the registry, built-in providers
//! * `context`     the Context Engine: the surface follows the application in front
//! * `devices`     `SurfaceDevice` trait, virtual Launchpad X, Launchpad X (the only MIDI code)
//! * `renderer`    semantic visual state to capability-aware frames
//! * `plugins`     plugin runtime, the System and Demo plugins, installed WebAssembly plugins
//! * `agents`      Agent Control: autonomous agents on a physical control surface
//! * `events`      event bus and the localhost-only WebSocket interface
//! * `surfaces`    starter surfaces
//! * `persistence` versioned JSON store and its migrations
//! * `audit`       the append-only audit trail
//! * `artifacts`   portable Surface and Pack files: their format, limits and checks,
//!   export and import
//! * `platform`    the operating system, as a value logic takes as a parameter
//! * `commands`    the Tauri commands, one file per feature
//! * `tray`        the system tray menu

pub mod actions;
pub mod agents;
pub mod artifacts;
pub mod audit;
pub mod context;
pub mod core;
pub mod devices;
pub mod events;
pub mod persistence;
pub mod platform;
pub mod plugins;
pub mod renderer;
pub mod state;
pub mod surfaces;

mod commands;
mod tray;

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::artifacts::ArtifactService;
use crate::core::runtime::{Runtime, RuntimeConfig, UiEmitter};
use crate::persistence::Store;
use crate::plugins::keyboard::EnigoKeyboard;
use crate::surfaces::default_workspace;

pub(crate) struct AppState {
    runtime: Arc<Runtime>,
    /// Export and import of Surface files and Packs.
    artifacts: ArtifactService,
}

struct TauriEmitter(AppHandle);

impl UiEmitter for TauriEmitter {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Store::new(&dir);
            let workspace = store.load_or_default(default_workspace);
            let config = RuntimeConfig {
                workspace,
                store: Some(store),
                emitter: Arc::new(TauriEmitter(app.handle().clone())),
                keyboard: Arc::new(EnigoKeyboard),
                scan_hardware: true,
                serve_external: true,
                demo_pace: 1.0,
                plugin_dirs: plugins::external::plugin_roots(
                    &dir,
                    std::env::var_os(plugins::external::PLUGINS_ENV),
                ),
                // The system's credential store. Without one, secrets are
                // unavailable: there is no file to fall back to.
                secrets: Some(plugins::external::secrets::open_store(Some(&dir))),
                // Which application is in front, by this system's own means.
                observer: Some(context::observer::native()),
            };
            let runtime = tauri::async_runtime::block_on(Runtime::start(config));
            if let Err(e) = tray::build_tray(app, &runtime) {
                // The tray is a convenience; never block startup on it.
                log::warn!("system tray unavailable: {e}");
            }
            // Exports are saved in Downloads; where a system has none, next
            // to the workspace.
            let exports = app
                .path()
                .download_dir()
                .unwrap_or_else(|_| dir.join("exports"));
            let artifacts = ArtifactService::new(runtime.clone(), exports);
            app.manage(AppState { runtime, artifacts });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::get_snapshot,
            commands::workspace::virtual_frame,
            commands::workspace::set_mode,
            commands::workspace::activate_surface,
            commands::workspace::activate_page,
            commands::workspace::virtual_input,
            commands::workspace::update_control,
            commands::workspace::remove_control,
            commands::workspace::create_surface,
            commands::workspace::add_page,
            commands::workspace::reset_workspace,
            commands::workspace::update_preferences,
            commands::context::context_status,
            commands::context::context_update,
            commands::context::context_resume,
            commands::actions::run_action,
            commands::actions::actions_catalog,
            commands::actions::actions_validate,
            commands::actions::observables_catalog,
            commands::actions::observable_state,
            commands::actions::send_event,
            commands::feedback::explain_control,
            commands::feedback::review_feedback,
            commands::devices::connect_device,
            commands::devices::disconnect_device,
            commands::devices::rescan_devices,
            commands::agents::agents_command,
            commands::agents::agents_focus,
            commands::agents::agents_dismiss,
            commands::agents::agents_set_access,
            commands::agents::agents_demo,
            commands::agents::agents_update_settings,
            commands::agents::agents_hook_setup,
            commands::agents::agents_rotate_hook_token,
            commands::plugins::plugins_list,
            commands::plugins::plugins_set_enabled,
            commands::plugins::plugins_set_config,
            commands::plugins::plugins_reload,
            commands::plugins::plugins_rescan,
            commands::plugins::plugins_logs,
            commands::plugins::plugins_set_permission,
            commands::plugins::plugins_set_secret,
            commands::plugins::plugins_clear_secret,
            commands::plugins::plugins_migrate_legacy_secrets,
            commands::plugins::plugins_discard_legacy_secrets,
            commands::plugins::plugins_migrate_plugin_legacy_secrets,
            commands::plugins::plugins_remove_plugin_legacy_secrets,
            commands::plugins::plugins_examples,
            commands::plugins::plugins_install_example,
            commands::plugins::plugins_reveal_folder,
            commands::artifacts::artifacts_limits,
            commands::artifacts::artifacts_export_preview,
            commands::artifacts::artifacts_export_save,
            commands::artifacts::artifacts_show_export,
            commands::artifacts::artifacts_inspect,
            commands::artifacts::artifacts_preview,
            commands::artifacts::artifacts_apply,
            commands::artifacts::artifacts_discard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LaunchDeck");
}
