//! LaunchDeck: a programmable physical interface runtime for MIDI grid controllers.
//!
//! Module map (see docs/architecture.md):
//! * `core`      model, runtime (surface runtime, state engine, event routing), context
//! * `devices`   `SurfaceDevice` trait, virtual Launchpad X, Launchpad X (the only MIDI code)
//! * `renderer`  semantic visual state to capability-aware frames
//! * `actions`   semantic action execution
//! * `plugins`   plugin runtime plus the System and Demo plugins
//! * `events`    event bus and the localhost-only WebSocket interface
//! * `surfaces`  starter surfaces
//! * `persistence` versioned JSON store

pub mod actions;
pub mod agents;
pub mod core;
pub mod devices;
pub mod events;
pub mod persistence;
pub mod plugins;
pub mod renderer;
pub mod state;
pub mod surfaces;

use std::sync::Arc;

use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::actions::definition::{Assignment, Catalog, Issue};
use crate::agents::adapters::HookSetup;
use crate::agents::model::{AccessLevel, Command};
use crate::agents::service::Target;
use crate::agents::settings::SettingsPatch;
use crate::core::model::{Control, ControlPos, Preferences};
use crate::core::runtime::{Mode, Runtime, RuntimeConfig, Snapshot, UiEmitter};
use crate::devices::SurfaceFrame;
use crate::events::external::{parse_inbound, Parsed};
use crate::persistence::Store;
use crate::plugins::keyboard::EnigoKeyboard;
use crate::surfaces::default_workspace;

struct AppState {
    runtime: Arc<Runtime>,
}

struct TauriEmitter(AppHandle);

impl UiEmitter for TauriEmitter {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }
}

type CmdResult<T> = Result<T, String>;

// ---------------------------------------------------------------------------
// Commands. Thin wrappers: the runtime owns all behaviour.
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.runtime.snapshot()
}

#[tauri::command]
fn virtual_frame(state: State<'_, AppState>) -> SurfaceFrame {
    state.runtime.virtual_handle().frame()
}

#[tauri::command]
fn set_mode(state: State<'_, AppState>, mode: Mode) {
    state.runtime.set_mode(mode);
}

#[tauri::command]
fn activate_surface(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: Option<String>,
) -> CmdResult<()> {
    state
        .runtime
        .activate_surface(&surface_id, page_id.as_deref())
}

#[tauri::command]
fn activate_page(state: State<'_, AppState>, page_id: String) -> CmdResult<()> {
    state.runtime.activate_page(&page_id)
}

/// Press or release a pad on the virtual Launchpad X. This goes through the
/// same input path as hardware.
#[tauri::command]
fn virtual_input(state: State<'_, AppState>, pos: ControlPos, pressed: bool) -> CmdResult<()> {
    let handle = state.runtime.virtual_handle();
    let result = if pressed {
        handle.press(pos, 100)
    } else {
        handle.release(pos)
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn update_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control: Control,
) -> CmdResult<()> {
    state.runtime.update_control(&surface_id, &page_id, control)
}

#[tauri::command]
fn remove_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control_id: String,
) -> CmdResult<()> {
    state
        .runtime
        .remove_control(&surface_id, &page_id, &control_id)
}

#[tauri::command]
fn create_surface(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    state.runtime.create_surface(&name)
}

#[tauri::command]
fn add_page(state: State<'_, AppState>, surface_id: String, name: String) -> CmdResult<String> {
    state.runtime.add_page(&surface_id, &name)
}

#[tauri::command]
fn reset_workspace(state: State<'_, AppState>) {
    state.runtime.reset_workspace();
}

#[tauri::command]
fn update_preferences(state: State<'_, AppState>, preferences: Preferences) {
    state.runtime.update_preferences(preferences);
}

/// Run one assignment immediately (the inspector's "Test" button) and report
/// how it went. Shell commands only ever come from controls the user
/// configured in this UI.
#[tauri::command]
async fn run_action(state: State<'_, AppState>, action: Assignment) -> CmdResult<Value> {
    let runtime = state.runtime.clone();
    runtime.run_action(action).await
}

/// The Action Library's catalog: every provider and every action, with
/// availability evaluated now. React searches it locally.
#[tauri::command]
fn actions_catalog(state: State<'_, AppState>) -> Catalog {
    state.runtime.catalog()
}

/// Everything a control can observe, states and events, with what each state
/// reads now. Updates arrive as `observables-changed`; this is the whole list.
#[tauri::command]
fn observables_catalog(state: State<'_, AppState>) -> crate::state::catalog::ObservableCatalog {
    state.runtime.observables()
}

/// What one state reads now: `known`, `unknown` or `unavailable`. The address
/// is checked against the state's definition, so a wrong argument is an error.
#[tauri::command]
fn observable_state(
    state: State<'_, AppState>,
    provider: String,
    id: String,
    args: Option<Value>,
) -> CmdResult<crate::state::Observed> {
    state
        .runtime
        .read_state(&provider, &id, &args.unwrap_or(Value::Null))
}

/// Why a control looks the way it does right now, with the rules, what each
/// reads, which one decided and why a state may not answer.
#[tauri::command]
fn explain_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control_id: String,
) -> CmdResult<crate::core::explain::ControlExplanation> {
    state
        .runtime
        .explain_control(&surface_id, &page_id, &control_id)
}

/// What a draft of a control's feedback comes to, without saving it: the
/// issues saving would refuse (with the field each is about), what Rust can
/// prove about the rules, the limits, and the draft's explanation. Read-only.
#[tauri::command]
fn review_feedback(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control: Control,
) -> CmdResult<crate::core::review::FeedbackReview> {
    state
        .runtime
        .review_feedback(&surface_id, &page_id, &control)
}

#[derive(serde::Serialize)]
struct ValidationOutcome {
    ok: bool,
    /// The assignment as it would be saved: defaults applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    assignment: Option<Assignment>,
    issues: Vec<Issue>,
}

/// Check an assignment the way saving does, without saving.
#[tauri::command]
fn actions_validate(state: State<'_, AppState>, assignment: Assignment) -> ValidationOutcome {
    match state.runtime.validate_assignment(&assignment) {
        Ok(a) => ValidationOutcome {
            ok: true,
            assignment: Some(a),
            issues: vec![],
        },
        Err(issues) => ValidationOutcome {
            ok: false,
            assignment: None,
            issues,
        },
    }
}

#[tauri::command]
fn connect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.runtime.connect_device(&id)
}

#[tauri::command]
fn disconnect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.runtime.disconnect_device(&id)
}

#[tauri::command]
async fn rescan_devices(state: State<'_, AppState>) -> CmdResult<()> {
    let rt = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || rt.rescan())
        .await
        .map_err(|e| e.to_string())
}

/// Send a test event through the same validation as the WebSocket interface.
#[tauri::command]
fn send_event(
    state: State<'_, AppState>,
    source: String,
    event: String,
    payload: Option<Value>,
) -> CmdResult<()> {
    let text = serde_json::json!({ "type": "event", "source": source, "event": event, "payload": payload }).to_string();
    match parse_inbound(&text)? {
        Parsed::Event(e) => {
            state.runtime.publish_external(e);
            Ok(())
        }
        Parsed::Ping => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Agent Control. Also thin: `AgentControl` owns the state and the rules.
// ---------------------------------------------------------------------------

/// Which track a desktop command is aimed at.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TargetArg {
    Focus,
    Session { id: String },
    Agent { id: String },
}

impl From<TargetArg> for Target {
    fn from(t: TargetArg) -> Self {
        match t {
            TargetArg::Focus => Target::Focus,
            TargetArg::Session { id } => Target::Session(id),
            TargetArg::Agent { id } => Target::Agent(id),
        }
    }
}

/// Returns the request id. The outcome arrives in the next agents snapshot.
#[tauri::command]
fn agents_command(
    state: State<'_, AppState>,
    target: TargetArg,
    command: Command,
) -> CmdResult<String> {
    state
        .runtime
        .agents()
        .command(target.into(), command)
        .map_err(|e| e.to_string())
}

/// Select a slot on the Agent Control surface (`null` clears the selection).
#[tauri::command]
fn agents_focus(state: State<'_, AppState>, slot: Option<usize>) -> CmdResult<()> {
    match slot {
        Some(slot) => state.runtime.agents().focus_slot(slot),
        None => {
            state.runtime.agents().clear_focus();
            Ok(())
        }
    }
}

/// Remove one finished session, or every finished session when `sessionId` is null.
#[tauri::command]
fn agents_dismiss(state: State<'_, AppState>, session_id: Option<String>) -> CmdResult<usize> {
    match session_id {
        Some(id) => state.runtime.agents().dismiss_session(&id).map(|()| 1),
        None => Ok(state.runtime.agents().dismiss_finished()),
    }
}

#[tauri::command]
fn agents_set_access(
    state: State<'_, AppState>,
    agent_id: String,
    access: AccessLevel,
) -> CmdResult<()> {
    state.runtime.agents().set_access(&agent_id, access)
}

#[tauri::command]
fn agents_demo(state: State<'_, AppState>, run: bool) -> CmdResult<()> {
    if run {
        state.runtime.demo_adapter().run_demo()
    } else {
        state.runtime.demo_adapter().reset_demo();
        Ok(())
    }
}

#[tauri::command]
async fn agents_update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> CmdResult<()> {
    state.runtime.update_agent_settings(patch).await
}

/// Contains the hook token, so it is only returned when the user asks for it.
#[tauri::command]
fn agents_hook_setup(state: State<'_, AppState>) -> HookSetup {
    state.runtime.hooks().setup()
}

#[tauri::command]
async fn agents_rotate_hook_token(state: State<'_, AppState>) -> CmdResult<()> {
    state.runtime.hooks().rotate_token().await.map(|_| ())
}

// ---------------------------------------------------------------------------
// External plugins (WebAssembly). Thin: `PluginHost` owns the lifecycle. Loading
// compiles a module, so the mutating commands run off the UI thread.
// ---------------------------------------------------------------------------

#[tauri::command]
fn plugins_list(state: State<'_, AppState>) -> plugins::external::PluginsOverview {
    state.runtime.plugin_host().overview()
}

#[tauri::command]
async fn plugins_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_enabled(&id, enabled))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn plugins_set_config(
    state: State<'_, AppState>,
    id: String,
    key: String,
    value: Value,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_config(&id, &key, value))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn plugins_reload(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.reload(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn plugins_rescan(state: State<'_, AppState>) -> CmdResult<plugins::external::ScanReport> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.scan())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn plugins_logs(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<plugins::external::LogEntry>> {
    state
        .runtime
        .plugin_host()
        .logs(&id)
        .map_err(|e| e.to_string())
}

/// Grant (`"granted"`), deny (`"denied"`) or take back (`null`) one origin a
/// plugin asks for. Only the person does this: no plugin, event or remote
/// interface can reach this command.
#[tauri::command]
async fn plugins_set_permission(
    state: State<'_, AppState>,
    id: String,
    origin: String,
    decision: Option<plugins::external::capabilities::Decision>,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_permission(&id, &origin, decision))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Store a secret. The value goes to the credential store and is never returned.
#[tauri::command]
async fn plugins_set_secret(
    state: State<'_, AppState>,
    id: String,
    key: String,
    value: String,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_secret(&id, &key, &value))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn plugins_clear_secret(
    state: State<'_, AppState>,
    id: String,
    key: String,
) -> CmdResult<plugins::external::PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.clear_secret(&id, &key))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tray
// ---------------------------------------------------------------------------

fn build_tray(app: &tauri::App, runtime: &Arc<Runtime>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open LaunchDeck", true, None::<&str>)?;
    let surfaces = runtime.snapshot().workspace.surfaces;
    let items: Vec<MenuItem<tauri::Wry>> = surfaces
        .iter()
        .map(|s| {
            MenuItem::with_id(
                app,
                format!("surface:{}", s.id),
                &s.name,
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<_>>()?;
    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let submenu = Submenu::with_items(app, "Surface", true, &item_refs)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LaunchDeck", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &submenu, &sep, &quit])?;

    let rt = runtime.clone();
    let mut builder = TrayIconBuilder::new().menu(&menu).tooltip("LaunchDeck");
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            id => {
                if let Some(surface) = id.strip_prefix("surface:") {
                    if let Err(e) = rt.activate_surface(surface, None) {
                        log::warn!("tray: {e}");
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

// ---------------------------------------------------------------------------

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
                // The system's credential store, or a private file next to the
                // workspace when the system has none.
                secrets: Some(plugins::external::secrets::open_store(Some(&dir))),
            };
            let runtime = tauri::async_runtime::block_on(Runtime::start(config));
            if let Err(e) = build_tray(app, &runtime) {
                // The tray is a convenience; never block startup on it.
                log::warn!("system tray unavailable: {e}");
            }
            app.manage(AppState { runtime });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            virtual_frame,
            set_mode,
            activate_surface,
            activate_page,
            virtual_input,
            update_control,
            remove_control,
            create_surface,
            add_page,
            reset_workspace,
            update_preferences,
            run_action,
            actions_catalog,
            actions_validate,
            observables_catalog,
            observable_state,
            explain_control,
            review_feedback,
            connect_device,
            disconnect_device,
            rescan_devices,
            send_event,
            agents_command,
            agents_focus,
            agents_dismiss,
            agents_set_access,
            agents_demo,
            agents_update_settings,
            agents_hook_setup,
            agents_rotate_hook_token,
            plugins_list,
            plugins_set_enabled,
            plugins_set_config,
            plugins_reload,
            plugins_rescan,
            plugins_logs,
            plugins_set_permission,
            plugins_set_secret,
            plugins_clear_secret,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LaunchDeck");
}
