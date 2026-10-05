//! The LaunchDeck runtime.
//!
//! Owns device lifecycle, the active surface/page, control state, the event
//! bus, action execution and rendering. React only observes and configures it
//! through `UiEmitter` events and Tauri commands.

mod actions;
mod context;
mod devices;
mod editing;
mod feedback;
mod input;
mod navigation;
mod observables;
mod startup;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::explain::OverlaySource;
use super::model::Workspace;
use crate::actions::definition::Catalog;
use crate::actions::ActionEngine;
use crate::agents::adapters::HookManager;
use crate::agents::demo::DemoAgentAdapter;
use crate::agents::service::AgentControl;
use crate::agents::settings::SettingsPatch;
use crate::agents::view::AgentsSnapshot;
use crate::context::observer::ForegroundObserver;
use crate::context::status::status as context_status;
use crate::context::{ContextStatus, Engine};
use crate::devices::manager::{DeviceInfo, DeviceManager};
use crate::devices::virtual_device::VirtualHandle;
use crate::events::{Event, EventBus};
use crate::persistence::{StartupNotice, Store};
use crate::plugins::external::PluginHost;
use crate::plugins::keyboard::Keyboard;
use crate::plugins::PluginManifest;
use crate::state::catalog::ObservableCatalog;
use crate::state::index::FeedbackIndex;
use crate::state::{Changes, StateHub};
use feedback::looks_of;

/// UI event names (Tauri events).
pub mod ui {
    pub const STATE: &str = "runtime-state";
    /// Auto Context: its settings, the application in front, what was decided.
    pub const CONTEXT: &str = "context-changed";
    pub const WORKSPACE: &str = "workspace-changed";
    pub const EVENT: &str = "runtime-event";
    pub const FRAME: &str = "virtual-frame";
    /// Full Agent Control snapshot (agents, sessions, slots, adapters).
    pub const AGENTS: &str = "agents-changed";
    /// A notification the app should surface (approval, failure, ...).
    pub const NOTIFICATION: &str = "agent-notification";
    /// What changed in the Observable Catalog: values, a provider's status, or
    /// "read the catalog again". Never the whole catalog.
    pub const OBSERVABLES: &str = "observables-changed";
}

pub trait UiEmitter: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
}

pub struct NullEmitter;
impl UiEmitter for NullEmitter {
    fn emit(&self, _event: &str, _payload: Value) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Pads behave exactly like the physical device.
    Live,
    /// Pads select controls for editing; actions do not run.
    Edit,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStatus {
    pub enabled: bool,
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub active_surface_id: String,
    pub active_page_id: String,
    pub mode: Mode,
    /// `surface/page/control` to the name of the look it shows now: a
    /// temporary one (an action running, an event's look) over the one its
    /// state rules give. Absent means the resting look.
    pub control_states: BTreeMap<String, String>,
    /// Actions that were invoked and have not finished. The runtime owns this:
    /// an entry appears when the action starts and goes when it completes or fails.
    pub pending: Vec<PendingAction>,
    pub devices: Vec<DeviceInfo>,
    pub external: ExternalStatus,
}

/// One action in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAction {
    /// `surface/page/control` of the pad that started it; absent for a test run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<String>,
    /// `provider.action`.
    pub action: String,
    /// When it started, milliseconds since the epoch.
    pub since: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub workspace: Workspace,
    pub state: RuntimeState,
    pub plugins: Vec<PluginManifest>,
    /// Everything the Action Library offers, evaluated when the snapshot was taken.
    pub catalog: Catalog,
    /// Everything a control can observe, with the values the hub holds now.
    pub observables: ObservableCatalog,
    pub recent_events: Vec<Event>,
    pub agents: AgentsSnapshot,
    /// Auto Context, as it is now.
    pub context: ContextStatus,
    /// What the person must be told about this start: the saved workspace
    /// could not be used, and where it was kept.
    pub notices: Vec<StartupNotice>,
}

pub struct RuntimeConfig {
    pub workspace: Workspace,
    pub store: Option<Store>,
    pub emitter: Arc<dyn UiEmitter>,
    pub keyboard: Arc<dyn Keyboard>,
    /// Poll for Launchpad X hardware (disabled in tests).
    pub scan_hardware: bool,
    /// Start the localhost WebSocket (disabled in most tests).
    pub serve_external: bool,
    /// Speed of the simulated demo agents (1.0 = documented timings; tests go faster).
    pub demo_pace: f32,
    /// Folders scanned for external plugins (one folder per plugin inside each).
    pub plugin_dirs: Vec<std::path::PathBuf>,
    /// Where plugin secrets are kept. `None` keeps them in memory, which is
    /// what tests want: the app passes the system's credential store.
    pub secrets: Option<Arc<dyn crate::plugins::external::secrets::SecretStore>>,
    /// What tells which application is in front. `None` observes nothing,
    /// which is what tests want unless they bring a scripted one; the app
    /// passes the one for the system it runs on.
    pub observer: Option<Box<dyn ForegroundObserver>>,
}

/// Who asked for a surface or a page to be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The sidebar, the tray, a navigation pad, a top-row button.
    Person,
    /// The context engine, for the application in front.
    Context,
    /// The runtime itself: the surface or page on screen was removed.
    Runtime,
}

/// A temporary look: an unbound pad running an action, or an event rule's look
/// while its hold lasts. The token lets a later change cancel an older timer.
struct StateEntry {
    name: String,
    token: u64,
    origin: Origin,
    /// When it ends by itself, if it does.
    until: Option<Instant>,
}

/// What put a temporary look on a control.
#[derive(Debug, Clone, Copy)]
struct Origin {
    source: OverlaySource,
    /// The event rule, by position.
    rule: Option<usize>,
    /// The rule holds `latch`.
    latched: bool,
}

impl Origin {
    const ACTION: Origin = Origin {
        source: OverlaySource::Action,
        rule: None,
        latched: false,
    };
}

struct Held {
    token: u64,
    long_fired: bool,
}

struct Inner {
    workspace: Workspace,
    active_surface: String,
    active_page: String,
    mode: Mode,
    /// Layer 1, temporary looks. Wins over `derived` while it lasts.
    overlay: HashMap<String, StateEntry>,
    /// Layer 2, the look each control's state rules give from the values the
    /// hub holds now. A function of the hub and the rules: recomputed when
    /// either changes, never an event's last word, never saved.
    derived: HashMap<String, String>,
    index: FeedbackIndex,
    devices: DeviceManager,
    held: HashMap<String, Held>,
    recent: VecDeque<Event>,
    external: ExternalStatus,
    /// Actions in flight, by the token each run was given.
    pending: BTreeMap<u64, PendingAction>,
    /// When a surface may change by itself. It decides; `activate` (navigation.rs) does.
    context: Engine,
}

pub fn control_key(surface: &str, page: &str, control: &str) -> String {
    format!("{surface}/{page}/{control}")
}

/// What the feedback machinery has done, for tests and diagnostics. Counters
/// only; nothing here affects behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackStats {
    /// Controls whose state rules were evaluated.
    pub evaluations: u64,
    /// Events that reached an event rule.
    pub events_routed: u64,
    /// Events the bus dropped because this runtime read it too slowly. State
    /// is not affected (it never travels on the bus); event rules can miss one.
    pub bus_lagged: u64,
}

#[derive(Default)]
struct Counters {
    evaluations: AtomicU64,
    events_routed: AtomicU64,
    bus_lagged: AtomicU64,
}

pub struct Runtime {
    inner: Mutex<Inner>,
    bus: EventBus,
    engine: ActionEngine,
    emitter: Arc<dyn UiEmitter>,
    store: Option<Store>,
    virtual_handle: VirtualHandle,
    agents: Arc<AgentControl>,
    demo: Arc<DemoAgentAdapter>,
    hooks: Arc<HookManager>,
    plugin_host: PluginHost,
    hub: StateHub,
    /// Hub changes the UI has not been told about yet, and whether a message
    /// for them is already scheduled.
    unannounced: Mutex<Changes>,
    announce_scheduled: AtomicBool,
    counters: Counters,
    epoch: Instant,
    counter: AtomicU64,
    handle: tokio::runtime::Handle,
    /// The context status the UI was last sent, so it is only told of changes.
    context_sent: Mutex<Option<ContextStatus>>,
    /// Said at start and never changed (`Snapshot::notices`).
    notices: Vec<StartupNotice>,
}

impl Runtime {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn next_token(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    // -----------------------------------------------------------------------
    // Observation
    // -----------------------------------------------------------------------

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn virtual_handle(&self) -> VirtualHandle {
        self.virtual_handle.clone()
    }

    /// Agent Control: the single source of truth for agents and sessions.
    pub fn agents(&self) -> &Arc<AgentControl> {
        &self.agents
    }

    pub fn demo_adapter(&self) -> &Arc<DemoAgentAdapter> {
        &self.demo
    }

    /// The local hook endpoint used by real harnesses (opt-in).
    pub fn hooks(&self) -> &Arc<HookManager> {
        &self.hooks
    }

    /// Apply a settings change from the desktop UI. Changing the hook endpoint
    /// restarts it; the error is the reason it could not listen.
    pub async fn update_agent_settings(&self, patch: SettingsPatch) -> Result<(), String> {
        patch.validate()?;
        let touches_hooks = patch.touches_hooks();
        self.agents.update_settings(|s| patch.apply(s));
        if touches_hooks {
            self.hooks.apply().await.map(|_| ())
        } else {
            Ok(())
        }
    }

    fn state_of(inner: &Inner) -> RuntimeState {
        RuntimeState {
            active_surface_id: inner.active_surface.clone(),
            active_page_id: inner.active_page.clone(),
            mode: inner.mode,
            control_states: looks_of(inner),
            pending: inner.pending.values().cloned().collect(),
            devices: inner.devices.list(),
            external: inner.external.clone(),
        }
    }

    pub fn state(&self) -> RuntimeState {
        Self::state_of(&self.lock())
    }

    /// Lifecycle of external plugins: list, enable, disable, configure, rescan.
    pub fn plugin_host(&self) -> &PluginHost {
        &self.plugin_host
    }

    /// The plugins the action engine can call (a handle on the shared table).
    pub fn plugin_registry(&self) -> crate::plugins::PluginRegistry {
        self.engine.plugins().clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.lock();
        Snapshot {
            workspace: inner.workspace.clone(),
            state: Self::state_of(&inner),
            plugins: self.engine.plugins().manifests(),
            catalog: self.engine.registry().catalog(),
            observables: self.observables(),
            recent_events: inner.recent.iter().rev().take(50).rev().cloned().collect(),
            agents: self.agents.snapshot(),
            context: context_status(&inner.context, &inner.workspace.context, &inner.workspace),
            notices: self.notices.clone(),
        }
    }

    fn emit_state(&self) {
        let state = self.state();
        // Every change of surface or page ends here, so the hub never lags it.
        self.publish_navigation(&state.active_surface_id, &state.active_page_id);
        self.emitter.emit(
            ui::STATE,
            serde_json::to_value(state).unwrap_or(Value::Null),
        );
    }

    fn emit_workspace(&self) {
        let ws = self.lock().workspace.clone();
        self.emitter.emit(
            ui::WORKSPACE,
            serde_json::to_value(ws).unwrap_or(Value::Null),
        );
        self.emit_state();
    }

    fn persist(&self, inner: &Inner) {
        if let Some(store) = &self.store {
            if let Err(e) = store.save(&inner.workspace) {
                log::error!("could not save workspace: {e}");
            }
        }
    }

    fn publish(&self, source: &str, kind: &str, payload: Value) {
        self.bus.publish(Event::new(source, kind, payload));
    }

    /// Publish an event as if it had arrived over the external interface.
    pub fn publish_external(&self, event: Event) {
        self.bus.publish(event);
    }
}

#[cfg(test)]
mod tests;
