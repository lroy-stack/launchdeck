//! The LaunchDeck runtime.
//!
//! Owns device lifecycle, the active surface/page, control state, the event
//! bus, action execution and rendering. React only observes and configures it
//! through `UiEmitter` events and Tauri commands.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::unbounded_channel;

use super::explain::{self, ControlExplanation, OverlayExplanation, OverlaySource};
use super::model::{Assignment, Control, Page, Preferences, Surface, Workspace};
use super::observables as native;
use super::review::{self, FeedbackReview};
use super::validate::validate_control;
use crate::actions::builtin::{
    HttpProvider, NavigationProvider, ShellProvider, NAVIGATION, OPEN_PAGE, OPEN_SURFACE,
};
use crate::actions::definition::{Catalog, Issue};
use crate::actions::ActionEngine;
use crate::agents::adapter::AdapterState;
use crate::agents::adapters::HookManager;
use crate::agents::bridge::BridgeAdapter;
use crate::agents::demo::DemoAgentAdapter;
use crate::agents::notify::Notification;
use crate::agents::plugin::AgentControlPlugin;
use crate::agents::service::{is_projection, AgentControl, AgentsSink, Timing};
use crate::agents::settings::{AgentsStorage, SettingsPatch};
use crate::agents::view::AgentsSnapshot;
use crate::devices::manager::{DeviceChange, DeviceInfo, DeviceManager};
use crate::devices::virtual_device::{VirtualHandle, VirtualLaunchpadX, VIRTUAL_ID};
use crate::devices::{DeviceInput, InputKind};
use crate::events::{types, Event, EventBus};
use crate::persistence::Store;
use crate::plugins::demo::DemoPlugin;
use crate::plugins::external::{Limits as PluginLimits, PluginHost};
use crate::plugins::keyboard::Keyboard;
use crate::plugins::system::SystemPlugin;
use crate::plugins::{PluginManifest, PluginRegistry};
use crate::renderer;
use crate::state::catalog::{self, ObservableCatalog, ObservablesDelta, Source};
use crate::state::eval;
use crate::state::feedback::{prepare_rules, Hold, When, IDLE};
use crate::state::index::{ControlRef, FeedbackIndex};
use crate::state::{Changes, Scalar, StateHub};
use crate::surfaces::{blank_surface, default_workspace};

const LONG_PRESS: Duration = Duration::from_millis(500);
const RENDER_INTERVAL: Duration = Duration::from_millis(40);
const RESCAN_INTERVAL: Duration = Duration::from_secs(3);
const SUCCESS_RESET: Duration = Duration::from_millis(2500);
const ERROR_RESET: Duration = Duration::from_millis(4000);
/// How long the `failed` look stays on a control whose action failed.
const FAILED_RESET: Duration = Duration::from_millis(1500);
/// How often plugins that asked to be refreshed are checked for being due.
const REFRESH_TICK: Duration = Duration::from_millis(250);
/// Looks a control may define for its own action: shown while it is in flight
/// and briefly after it failed. They sit over the state rules and end by themselves.
const PENDING_LOOK: &str = "pending";
const FAILED_LOOK: &str = "failed";
const RECENT_EVENTS: usize = 100;
/// Changes to the Observable Catalog that arrive within this window reach the
/// UI as one message. Feedback is not delayed by it: pads are re-evaluated as
/// soon as the hub reports.
const OBSERVABLES_COALESCE: Duration = Duration::from_millis(50);

/// UI event names (Tauri events).
pub mod ui {
    pub const STATE: &str = "runtime-state";
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

/// Forwards Agent Control state to the UI. It holds no state of its own.
struct UiAgentsSink(Arc<dyn UiEmitter>);

impl AgentsSink for UiAgentsSink {
    fn changed(&self, snapshot: &AgentsSnapshot) {
        self.0.emit(
            ui::AGENTS,
            serde_json::to_value(snapshot).unwrap_or(Value::Null),
        );
    }
    fn notify(&self, notification: &Notification) {
        self.0.emit(
            ui::NOTIFICATION,
            serde_json::to_value(notification).unwrap_or(Value::Null),
        );
    }
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
}

pub fn control_key(surface: &str, page: &str, control: &str) -> String {
    format!("{surface}/{page}/{control}")
}

fn find_control<'a>(ws: &'a Workspace, r: &ControlRef) -> Option<&'a Control> {
    ws.surface(&r.surface)?
        .page(&r.page)?
        .controls
        .iter()
        .find(|c| c.id == r.control)
}

/// The look every control shows: a temporary one over the one its state rules
/// give. A control at rest is absent.
fn looks_of(inner: &Inner) -> BTreeMap<String, String> {
    let mut looks: BTreeMap<String, String> = inner
        .derived
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, e) in &inner.overlay {
        looks.insert(k.clone(), e.name.clone());
    }
    looks
}

/// An event rule's payload test: equal as values, so `2` equals `2.0`.
fn payload_matches(payload: &Value, test: &crate::state::feedback::FieldTest) -> bool {
    payload
        .get(&test.field)
        .and_then(|v| Scalar::from_json(v).ok())
        .is_some_and(|v| v.same_as(&test.equals))
}

/// The look and hold of the first event rule of `control` that `event`
/// matches, and the position of that rule.
fn event_look(control: &Control, event: &Event) -> Option<(usize, String, Hold)> {
    for (index, rule) in control.feedback.iter().enumerate() {
        let When::Event(c) = &rule.when else {
            continue;
        };
        if c.event.provider != event.source
            || c.event.id != event.kind
            || !c.where_.iter().all(|t| payload_matches(&event.payload, t))
        {
            continue;
        }
        let look = match (&rule.show, &rule.show_from_field) {
            (Some(name), _) => name.clone(),
            (None, Some(field)) => match event
                .payload
                .get(field)
                .and_then(|v| Scalar::from_json(v).ok())
            {
                Some(v) => v.look_name(),
                // The event lacks the field this rule names its look from.
                None => continue,
            },
            (None, None) => continue,
        };
        return Some((index, look, rule.hold_or_default()));
    }
    None
}

fn describe_issues(issues: &[crate::state::feedback::RuleIssue]) -> String {
    issues
        .iter()
        .map(|i| format!("Feedback rule {}: {}", i.rule + 1, i.message))
        .collect::<Vec<_>>()
        .join("; ")
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
}

impl Runtime {
    // -----------------------------------------------------------------------
    // Startup
    // -----------------------------------------------------------------------

    /// Start order (STATE_FEEDBACK_PLAN.md, section 8.3):
    ///
    /// 1. providers declare their states and report what they know (Agent
    ///    Control writes its whole projection, the plugin host registers every
    ///    plugin: running ones `unknown`, the rest `unavailable`);
    /// 2. every control is evaluated once;
    /// 3. only then do the loops start.
    ///
    /// Step 2 before step 3 is why the very first frame is already right.
    pub async fn start(config: RuntimeConfig) -> Arc<Runtime> {
        let bus = EventBus::new();
        let hub = StateHub::new();
        let storage = config
            .store
            .as_ref()
            .and_then(|s| s.path().parent())
            .map(AgentsStorage::new);
        let agents = AgentControl::new(
            bus.clone(),
            Arc::new(UiAgentsSink(config.emitter.clone())),
            storage,
            Timing::default(),
            hub.clone(),
        );
        let demo = DemoAgentAdapter::new(config.demo_pace);
        agents.register_adapter(demo.clone());
        let bridge = BridgeAdapter::new();
        agents.register_adapter(bridge.clone());
        let hooks = HookManager::new(agents.clone());
        let registry = PluginRegistry::default();
        registry.register(Arc::new(NavigationProvider));
        registry.register(Arc::new(ShellProvider));
        registry.register(Arc::new(HttpProvider));
        registry.register(Arc::new(SystemPlugin::new(config.keyboard.clone())));
        registry.register(Arc::new(DemoPlugin::new()));
        registry.register(Arc::new(AgentControlPlugin::new(
            agents.clone(),
            demo.clone(),
        )));
        // The states of what ships with the app. These providers are always
        // running; anything they report later replaces the `unknown` they start with.
        for plugin in registry.all() {
            let defs = plugin.states();
            if defs.is_empty() {
                continue;
            }
            let id = plugin.manifest().id;
            if let Err(e) = hub.ensure_provider(&id, defs) {
                log::error!("could not declare the states of `{id}`: {e}");
            }
        }
        // What the app itself reports: which surface and page are on screen.
        if let Err(e) = hub.ensure_provider(native::RUNTIME, native::states()) {
            log::error!("could not declare the states of the runtime: {e}");
        }
        // External plugins: found on disk, sandboxed, registered next to the built-ins.
        let data_dir = config.store.as_ref().and_then(|s| s.path().parent());
        let plugin_host = PluginHost::with_parts(
            config.plugin_dirs.clone(),
            data_dir.map(|dir| dir.join("plugins.json")),
            registry.clone(),
            PluginLimits::default(),
            hub.clone(),
            // What each plugin was granted, and the trail of who decided what:
            // next to the workspace, never inside it.
            Arc::new(crate::plugins::external::capabilities::Grants::new(
                data_dir.map(|dir| dir.join("permissions.json")),
                data_dir.map(|dir| dir.join("audit.jsonl")),
            )),
            config
                .secrets
                .clone()
                .unwrap_or_else(|| Arc::new(crate::plugins::external::secrets::MemoryStore::new())),
        );
        {
            // Compiling modules is blocking work.
            let host = plugin_host.clone();
            if let Ok(report) = tokio::task::spawn_blocking(move || host.scan()).await {
                log::info!(
                    "plugins: {} loaded, {} problems",
                    report.loaded,
                    report.issues
                );
            }
        }
        let engine = ActionEngine::with_hub(registry, bus.clone(), hub.clone());

        let (input_tx, mut input_rx) = unbounded_channel::<DeviceInput>();
        let mut devices = DeviceManager::new(input_tx);
        let (vdev, vhandle) = VirtualLaunchpadX::new();
        {
            let emitter = config.emitter.clone();
            vhandle.set_on_change(move |frame| {
                emitter.emit(
                    ui::FRAME,
                    serde_json::to_value(frame).unwrap_or(Value::Null),
                );
            });
        }
        devices.register(Box::new(vdev));
        if let Err(e) = devices.connect(VIRTUAL_ID) {
            log::error!("virtual device failed to connect: {e}");
        }

        let workspace = config.workspace;
        let active_surface = workspace
            .last_active_surface
            .as_deref()
            .and_then(|id| workspace.surface(id))
            .or_else(|| workspace.surfaces.first())
            .map(|s| s.id.clone())
            .unwrap_or_default();
        let active_page = workspace
            .surface(&active_surface)
            .and_then(|s| s.home_page())
            .map(|p| p.id.clone())
            .unwrap_or_default();

        let external = ExternalStatus {
            enabled: workspace.preferences.external_events_enabled,
            port: workspace.preferences.external_events_port,
            listening: false,
            error: None,
        };

        let index = FeedbackIndex::build(&workspace);
        let rt = Arc::new(Runtime {
            inner: Mutex::new(Inner {
                workspace,
                active_surface,
                active_page,
                mode: Mode::Live,
                overlay: HashMap::new(),
                derived: HashMap::new(),
                index,
                devices,
                held: HashMap::new(),
                recent: VecDeque::new(),
                external,
                pending: BTreeMap::new(),
            }),
            bus: bus.clone(),
            engine,
            emitter: config.emitter,
            store: config.store,
            virtual_handle: vhandle,
            agents,
            demo,
            hooks: hooks.clone(),
            plugin_host,
            hub,
            unannounced: Mutex::new(Changes::default()),
            announce_scheduled: AtomicBool::new(false),
            counters: Counters::default(),
            epoch: Instant::now(),
            counter: AtomicU64::new(1),
            handle: tokio::runtime::Handle::current(),
        });

        // Every control is evaluated once, from the states the providers
        // already hold, before anything can draw or react.
        {
            let mut inner = rt.lock();
            rt.publish_navigation(&inner.active_surface, &inner.active_page);
            // What the providers declared so far is already in the evaluation
            // below; anything they write from now on is drained later.
            let _ = rt.hub.drain();
            rt.evaluate_all(&mut inner);
        }
        // Input loop: devices (any thread) -> runtime.
        {
            let rt = rt.clone();
            tokio::spawn(async move {
                while let Some(input) = input_rx.recv().await {
                    rt.handle_input(input);
                }
            });
        }
        // State loop: the hub says what changed, the runtime re-evaluates only
        // the controls that read it. Writes are coalesced, so a burst on one
        // state is one evaluation per control.
        {
            let rt = rt.clone();
            tokio::spawn(async move {
                loop {
                    rt.hub.changed().await;
                    let changes = rt.hub.drain();
                    if !changes.is_empty() {
                        rt.apply_hub_changes(&changes);
                        rt.announce_observables(changes);
                    }
                }
            });
        }
        // Event loop: bus -> event rules (temporary looks) + UI log.
        {
            let rt = rt.clone();
            let mut rx = bus.subscribe();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => rt.on_event(event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            rt.bus_lagged(missed);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        // Render loop.
        {
            let rt = rt.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(RENDER_INTERVAL);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    rt.render_tick();
                }
            });
        }
        // Plugins that asked to be refreshed: checked often, called when due.
        {
            let rt = rt.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(REFRESH_TICK);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    rt.refresh_due();
                }
            });
        }
        // Real-harness hooks: off unless the user turned the endpoint on.
        if let Err(e) = hooks.apply().await {
            log::warn!("{e}");
        }
        // Hardware discovery loop.
        if config.scan_hardware {
            let rt = rt.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(RESCAN_INTERVAL);
                loop {
                    tick.tick().await;
                    let r = rt.clone();
                    let _ = tokio::task::spawn_blocking(move || r.rescan()).await;
                }
            });
        }
        // External events.
        // The Agent Bridge rides on the same localhost WebSocket.
        if config.serve_external {
            let (enabled, port) = {
                let i = rt.lock();
                (i.external.enabled, i.external.port)
            };
            if enabled {
                let extension: Arc<dyn crate::events::external::Extension> = bridge.clone();
                // Installed plugin ids are sources of their own: a client may not impersonate one.
                let registry = rt.engine.plugins().clone();
                let guard: crate::events::external::SourceGuard =
                    Arc::new(move |source: &str| registry.contains(source));
                let result =
                    crate::events::external::serve_with(port, bus, Some(extension), Some(guard))
                        .await;
                let mut i = rt.lock();
                match result {
                    Ok(addr) => {
                        i.external.listening = true;
                        i.external.port = addr.port();
                        bridge.mark_listening(addr);
                    }
                    Err(e) => {
                        let msg = format!("could not listen on 127.0.0.1:{port}: {e}");
                        bridge.mark_unavailable(AdapterState::Error, &msg);
                        i.external.error = Some(msg);
                    }
                }
            } else {
                bridge.mark_unavailable(
                    AdapterState::Disabled,
                    "External events are turned off in the preferences.",
                );
            }
        } else {
            bridge.mark_unavailable(
                AdapterState::Disabled,
                "The local WebSocket is not served in this session.",
            );
        }
        rt
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn next_token(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    /// The index follows the workspace: rebuilt after any edit of the rules.
    fn rebuild_index(&self, inner: &mut Inner) {
        inner.index = FeedbackIndex::build(&inner.workspace);
        // What controls read may have changed, so what plugins report should too.
        self.plugin_host.refresh_all_soon();
    }

    /// The instances of a provider's states that some control's rules read,
    /// anywhere in the workspace.
    fn watched_states(&self, provider: &str) -> Vec<crate::state::StateAddress> {
        let inner = self.lock();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for surface in &inner.workspace.surfaces {
            for page in &surface.pages {
                for control in &page.controls {
                    for rule in &control.feedback {
                        if let When::State(c) = &rule.when {
                            if c.state.provider == provider && seen.insert(c.state.key()) {
                                out.push(c.state.clone());
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Start the refresh of every plugin that is due. Each runs on a blocking
    /// thread, like an action; none is started on top of one still running.
    fn refresh_due(self: &Arc<Self>) {
        for id in self.plugin_host.due_refreshes() {
            let watching = self.watched_states(&id);
            let rt = self.clone();
            self.handle.spawn(async move {
                let host = rt.plugin_host.clone();
                let ctx = crate::plugins::PluginContext::new(
                    &id,
                    rt.bus.clone(),
                    tokio::runtime::Handle::current(),
                )
                .with_hub(rt.hub.clone());
                let _ =
                    tokio::task::spawn_blocking(move || host.refresh(&id, &ctx, &watching)).await;
            });
        }
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

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    pub fn activate_surface(&self, surface_id: &str, page_id: Option<&str>) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = match page_id {
                Some(p) => surface
                    .page(p)
                    .ok_or_else(|| format!("unknown page `{p}`"))?,
                None => surface.home_page().ok_or("surface has no pages")?,
            }
            .id
            .clone();
            let surface_changed = inner.active_surface != surface_id;
            inner.active_surface = surface_id.to_string();
            inner.active_page = page.clone();
            inner.held.clear();
            inner.workspace.last_active_surface = Some(surface_id.to_string());
            self.persist(&inner);
            drop(inner);
            if surface_changed {
                self.publish(
                    "runtime",
                    types::SURFACE_CHANGED,
                    json!({ "surfaceId": surface_id, "pageId": page }),
                );
            }
            self.publish(
                "runtime",
                types::PAGE_CHANGED,
                json!({ "surfaceId": surface_id, "pageId": page }),
            );
        }
        // Nothing is replayed to repaint the surface: every control's look is
        // already derived from the states the hub holds, shown or not.
        self.emit_state();
        Ok(())
    }

    pub fn activate_page(&self, page_id: &str) -> Result<(), String> {
        let surface_id = {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface(&inner.active_surface)
                .ok_or("no active surface")?;
            if surface.page(page_id).is_none() {
                return Err(format!("unknown page `{page_id}`"));
            }
            inner.active_page = page_id.to_string();
            inner.held.clear();
            inner.active_surface.clone()
        };
        self.publish(
            "runtime",
            types::PAGE_CHANGED,
            json!({ "surfaceId": surface_id, "pageId": page_id }),
        );
        self.emit_state();
        Ok(())
    }

    /// Perform a navigation assignment (already prepared by the registry).
    fn navigate(&self, assignment: &Assignment) -> Result<(), String> {
        let text = |key: &str| {
            assignment
                .args
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        match assignment.action.as_str() {
            OPEN_SURFACE => {
                let surface = text("surfaceId").ok_or("a surface is required")?;
                self.activate_surface(&surface, text("pageId").as_deref())
            }
            OPEN_PAGE => {
                let page = text("pageId").ok_or("a page is required")?;
                self.activate_page(&page)
            }
            other => Err(format!("unknown navigation action `{other}`")),
        }
    }

    pub fn set_mode(&self, mode: Mode) {
        {
            let mut inner = self.lock();
            inner.mode = mode;
            inner.held.clear();
        }
        self.emit_state();
    }

    // -----------------------------------------------------------------------
    // Input and actions
    // -----------------------------------------------------------------------

    pub fn handle_input(self: &Arc<Self>, input: DeviceInput) {
        let mut inner = self.lock();
        let (surface_id, page_id) = (inner.active_surface.clone(), inner.active_page.clone());
        let Some(control) = inner
            .workspace
            .surface(&surface_id)
            .and_then(|s| s.page(&page_id))
            .and_then(|p| p.control_at(input.pos))
            .cloned()
        else {
            return;
        };
        let key = control_key(&surface_id, &page_id, &control.id);

        match input.kind {
            InputKind::Pressed { velocity } => {
                self.publish(
                    "device",
                    types::CONTROL_PRESSED,
                    json!({
                        "deviceId": input.device_id,
                        "surfaceId": surface_id,
                        "pageId": page_id,
                        "controlId": control.id,
                        "label": control.label,
                        "velocity": velocity,
                    }),
                );
                if inner.mode == Mode::Edit {
                    return;
                }
                if control.long_press.is_some() {
                    // The tap action is deferred to release so a long press can win.
                    let token = self.next_token();
                    inner.held.insert(
                        key.clone(),
                        Held {
                            token,
                            long_fired: false,
                        },
                    );
                    drop(inner);
                    let rt = self.clone();
                    self.handle.spawn(async move {
                        tokio::time::sleep(LONG_PRESS).await;
                        rt.fire_long_press(key, token, surface_id, page_id, control);
                    });
                } else if let Some(action) = control.press.clone() {
                    drop(inner);
                    self.trigger(surface_id, page_id, Some(control), action);
                }
            }
            InputKind::Released => {
                let Some(held) = inner.held.remove(&key) else {
                    return;
                };
                if held.long_fired || inner.mode == Mode::Edit {
                    return;
                }
                if let Some(action) = control.press.clone() {
                    drop(inner);
                    self.trigger(surface_id, page_id, Some(control), action);
                }
            }
        }
    }

    fn fire_long_press(
        self: &Arc<Self>,
        key: String,
        token: u64,
        surface_id: String,
        page_id: String,
        control: Control,
    ) {
        {
            let mut inner = self.lock();
            match inner.held.get_mut(&key) {
                Some(h) if h.token == token && !h.long_fired => h.long_fired = true,
                _ => return,
            }
        }
        if let Some(action) = control.long_press.clone() {
            self.trigger(surface_id, page_id, Some(control), action);
        }
    }

    /// Run an assignment as if a pad had triggered it (the inspector's Test
    /// button) and report how it went. The same events, the same checks.
    pub async fn run_action(self: &Arc<Self>, assignment: Assignment) -> Result<Value, String> {
        let (s, p) = {
            let i = self.lock();
            (i.active_surface.clone(), i.active_page.clone())
        };
        self.clone().run_action_async(s, p, None, assignment).await
    }

    fn trigger(
        self: &Arc<Self>,
        surface_id: String,
        page_id: String,
        control: Option<Control>,
        assignment: Assignment,
    ) {
        let rt = self.clone();
        self.handle.spawn(async move {
            let _ = rt
                .run_action_async(surface_id, page_id, control, assignment)
                .await;
        });
    }

    /// Check an assignment the way saving and running do, without doing either.
    pub fn validate_assignment(&self, assignment: &Assignment) -> Result<Assignment, Vec<Issue>> {
        self.engine
            .registry()
            .prepare(assignment)
            .map_err(|e| e.issues())
    }

    /// The Action Library's catalog, evaluated now.
    pub fn catalog(&self) -> Catalog {
        self.engine.registry().catalog()
    }

    /// Who offers what to observe: the app itself first, then every registered
    /// provider (built-in, Agent Control, installed plugins).
    fn observable_sources(&self) -> Vec<Source> {
        let mut sources = native::sources();
        sources.extend(self.engine.registry().observable_sources());
        sources
    }

    /// Everything a control can observe, states and events, with the values the
    /// hub holds now. The one list the editor picks from.
    pub fn observables(&self) -> ObservableCatalog {
        catalog::build(self.observable_sources(), &self.hub)
    }

    /// What one state reads now, for an address the editor has built. The
    /// arguments are checked against the definition, so `2` and `2.0` read the
    /// same instance and a wrong argument is an error, not an `unknown`.
    pub fn read_state(
        &self,
        provider: &str,
        id: &str,
        args: &Value,
    ) -> Result<crate::state::Observed, String> {
        let def = self
            .hub
            .definition(provider, id)
            .ok_or_else(|| format!("`{provider}.{id}` is not a state anything offers right now"))?;
        let address = def.address(args)?;
        Ok(self.hub.read_address(&address))
    }

    /// The surface and page on screen are states like any other.
    fn publish_navigation(&self, surface: &str, page: &str) {
        for (address, value) in [
            (native::surface_active(), surface),
            (native::page_active(), page),
        ] {
            if let Err(e) = self.hub.set(&address, Scalar::text(value)) {
                log::warn!("could not publish {address}: {e}");
            }
        }
    }

    /// Tell the UI what the hub reported, as one message for a burst. The
    /// window starts at the first change; whatever arrives within it joins.
    fn announce_observables(self: &Arc<Self>, changes: Changes) {
        self.unannounced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .merge(changes);
        if self.announce_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        let rt = self.clone();
        self.handle.spawn(async move {
            tokio::time::sleep(OBSERVABLES_COALESCE).await;
            // Cleared before the take: a change that lands after it schedules
            // the next message and is never lost.
            rt.announce_scheduled.store(false, Ordering::SeqCst);
            let changes =
                std::mem::take(&mut *rt.unannounced.lock().unwrap_or_else(|p| p.into_inner()));
            let delta: ObservablesDelta =
                catalog::delta(&rt.observable_sources(), &rt.hub, &changes);
            if !delta.is_empty() {
                rt.emitter.emit(
                    ui::OBSERVABLES,
                    serde_json::to_value(delta).unwrap_or(Value::Null),
                );
            }
        });
    }

    async fn run_action_async(
        self: Arc<Self>,
        surface_id: String,
        page_id: String,
        control: Option<Control>,
        action: Assignment,
    ) -> Result<Value, String> {
        let semantic = action.definition_id();
        let control_id = control.as_ref().map(|c| c.id.clone());
        let ctx = json!({ "action": semantic, "controlId": control_id, "surfaceId": surface_id, "pageId": page_id });
        let with = |extra: Value| {
            let mut v = ctx.clone();
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };

        if action.provider == NAVIGATION {
            let outcome = match self.engine.registry().prepare(&action) {
                Ok(prepared) => self.navigate(&prepared),
                Err(e) => Err(e.to_string()),
            };
            return match outcome {
                Ok(()) => {
                    self.publish("runtime", types::ACTION_COMPLETED, ctx);
                    Ok(Value::Null)
                }
                Err(e) => {
                    self.publish(
                        "runtime",
                        types::ACTION_FAILED,
                        with(json!({ "error": e.clone() })),
                    );
                    Err(e)
                }
            };
        }

        // A control with feedback rules gets its stable look from them. Its own
        // action only ever adds a temporary one: `pending` while it is in flight
        // and `failed` briefly after it failed, when the control defines them.
        // A control without rules keeps the automatic running/success/error looks.
        let auto = control.as_ref().filter(|c| c.feedback.is_empty());
        let key = control
            .as_ref()
            .map(|c| control_key(&surface_id, &page_id, &c.id));
        let has = |state: &str| auto.is_some_and(|c| c.states.contains_key(state));
        let defines = |state: &str| {
            control
                .as_ref()
                .is_some_and(|c| c.states.contains_key(state))
        };
        let pending_look = if has("running") {
            Some("running")
        } else if defines(PENDING_LOOK) {
            Some(PENDING_LOOK)
        } else {
            None
        };

        self.publish("runtime", types::ACTION_STARTED, ctx.clone());
        // From here until the provider answers, the action is pending, and the
        // runtime says so: in its state, and on the pad if it has a look for it.
        let run = self.begin_action(key.clone(), &semantic);
        let shown = match (&key, pending_look) {
            (Some(key), Some(look)) => {
                Some(self.set_overlay(key, Some(look), None, Origin::ACTION))
            }
            _ => None,
        };
        let outcome = self.engine.execute(&action).await;
        self.end_action(run);
        match outcome {
            Ok(result) => {
                self.publish(
                    "runtime",
                    types::ACTION_COMPLETED,
                    with(json!({ "result": result })),
                );
                if let Some(key) = &key {
                    if has("success") {
                        self.set_overlay(key, Some("success"), Some(SUCCESS_RESET), Origin::ACTION);
                    } else if auto.is_some() {
                        self.set_overlay(key, None, None, Origin::ACTION);
                    } else if let Some(token) = shown {
                        // Success leaves nothing behind: the state rules decide what the pad shows.
                        self.clear_overlay(key, token);
                    }
                }
                Ok(result)
            }
            Err(e) => {
                self.publish(
                    "runtime",
                    types::ACTION_FAILED,
                    with(json!({ "error": e.to_string() })),
                );
                if let Some(key) = &key {
                    if has("error") {
                        self.set_overlay(key, Some("error"), Some(ERROR_RESET), Origin::ACTION);
                    } else if defines(FAILED_LOOK) {
                        self.set_overlay(
                            key,
                            Some(FAILED_LOOK),
                            Some(FAILED_RESET),
                            Origin::ACTION,
                        );
                    } else if auto.is_some() {
                        self.set_overlay(key, None, None, Origin::ACTION);
                    } else if let Some(token) = shown {
                        self.clear_overlay(key, token);
                    }
                }
                Err(e.to_string())
            }
        }
    }

    /// Record that an action is in flight. The token ends it.
    fn begin_action(&self, control: Option<String>, action: &str) -> u64 {
        let token = self.next_token();
        self.lock().pending.insert(
            token,
            PendingAction {
                control,
                action: action.to_string(),
                since: crate::events::now_ms(),
            },
        );
        self.emit_state();
        token
    }

    fn end_action(&self, token: u64) {
        if self.lock().pending.remove(&token).is_some() {
            self.emit_state();
        }
    }

    // -----------------------------------------------------------------------
    // State and events
    // -----------------------------------------------------------------------

    /// Put a temporary look on a control, or take it off with `None`.
    /// Optionally it ends by itself after `reset_after`, unless something else
    /// replaced it in the meantime (tracked with a token).
    fn set_overlay(
        self: &Arc<Self>,
        key: &str,
        look: Option<&str>,
        reset_after: Option<Duration>,
        origin: Origin,
    ) -> u64 {
        let token = self.next_token();
        {
            let mut inner = self.lock();
            match look {
                Some(name) => {
                    inner.overlay.insert(
                        key.to_string(),
                        StateEntry {
                            name: name.to_string(),
                            token,
                            origin,
                            until: reset_after.map(|after| Instant::now() + after),
                        },
                    );
                }
                None => {
                    inner.overlay.remove(key);
                }
            }
        }
        self.emit_state();
        if let (Some(after), Some(_)) = (reset_after, look) {
            let rt = self.clone();
            let key = key.to_string();
            self.handle.spawn(async move {
                tokio::time::sleep(after).await;
                let removed = {
                    let mut i = rt.lock();
                    if i.overlay.get(&key).is_some_and(|e| e.token == token) {
                        i.overlay.remove(&key);
                        true
                    } else {
                        false
                    }
                };
                if removed {
                    rt.emit_state();
                }
            });
        }
        token
    }

    /// Take a temporary look off, if it is still the one `token` put there.
    /// Something that replaced it in the meantime (an event's look) stays.
    fn clear_overlay(&self, key: &str, token: u64) {
        let removed = {
            let mut i = self.lock();
            if i.overlay.get(key).is_some_and(|e| e.token == token) {
                i.overlay.remove(key);
                true
            } else {
                false
            }
        };
        if removed {
            self.emit_state();
        }
    }

    /// Evaluate the state rules of every control that has any, from the values
    /// the hub holds now. Used at start and whenever the rules change as a
    /// whole.
    fn evaluate_all(&self, inner: &mut Inner) {
        inner.derived.clear();
        let refs: Vec<ControlRef> = inner.index.stateful().cloned().collect();
        for r in &refs {
            self.derive_into(inner, r);
        }
    }

    /// Re-evaluate one control and store what its state rules give. Returns
    /// whether the derived look changed.
    fn derive_into(&self, inner: &mut Inner, r: &ControlRef) -> bool {
        let key = r.key();
        // A control with only event rules has nothing to derive.
        if !find_control(&inner.workspace, r)
            .is_some_and(|c| c.feedback.iter().any(|r| r.is_state()))
        {
            return inner.derived.remove(&key).is_some();
        }
        let evaluation = find_control(&inner.workspace, r).map(|c| {
            eval::evaluate(&c.feedback, &|k| self.hub.read(k), &|name| {
                c.states.contains_key(name)
            })
        });
        self.counters.evaluations.fetch_add(1, Ordering::Relaxed);
        let Some(evaluation) = evaluation else {
            return inner.derived.remove(&key).is_some();
        };
        let before = inner.derived.get(&key).cloned();
        match &evaluation.look {
            Some(look) => {
                inner.derived.insert(key.clone(), look.clone());
            }
            None => {
                inner.derived.remove(&key);
            }
        }
        let changed = before != evaluation.look;
        if changed {
            for d in &evaluation.diagnostics {
                log::warn!("feedback of `{key}`: {d:?}");
            }
        }
        changed
    }

    /// The hub says these states changed: re-evaluate the controls that read
    /// them, and only those.
    fn apply_hub_changes(&self, changes: &Changes) {
        let changed = {
            let mut inner = self.lock();
            let refs: Vec<ControlRef> = inner
                .index
                .dependents(changes)
                .into_iter()
                .cloned()
                .collect();
            let mut any = false;
            for r in &refs {
                any |= self.derive_into(&mut inner, r);
            }
            any
        };
        if changed {
            self.emit_state();
        }
    }

    /// The event loop fell behind and the bus dropped events. States are not
    /// affected (they never travel on the bus); an event rule can have missed
    /// its event.
    fn bus_lagged(&self, missed: u64) {
        self.counters
            .bus_lagged
            .fetch_add(missed, Ordering::Relaxed);
        log::warn!("the event loop missed {missed} events; states are unaffected");
    }

    /// What the feedback machinery has done so far.
    pub fn feedback_stats(&self) -> FeedbackStats {
        FeedbackStats {
            evaluations: self.counters.evaluations.load(Ordering::Relaxed),
            events_routed: self.counters.events_routed.load(Ordering::Relaxed),
            bus_lagged: self.counters.bus_lagged.load(Ordering::Relaxed),
        }
    }

    /// The current values every provider holds.
    pub fn state_hub(&self) -> &StateHub {
        &self.hub
    }

    /// Why a control looks the way it does: the temporary look if one is on,
    /// then the state rules rule by rule against the values now, with what
    /// each state is and why it may not answer.
    pub fn explain_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control_id: &str,
    ) -> Result<ControlExplanation, String> {
        let sources = self.observable_sources();
        let inner = self.lock();
        let control = inner
            .workspace
            .surface(surface_id)
            .and_then(|s| s.page(page_id))
            .and_then(|p| p.controls.iter().find(|c| c.id == control_id))
            .ok_or_else(|| format!("unknown control `{surface_id}/{page_id}/{control_id}`"))?;
        let key = control_key(surface_id, page_id, control_id);
        let now = Instant::now();
        let overlay = inner.overlay.get(&key).map(|e| OverlayExplanation {
            look: e.name.clone(),
            source: e.origin.source,
            rule: e.origin.rule,
            remaining_ms: e
                .until
                .map(|t| t.saturating_duration_since(now).as_millis() as u64),
            latched: e.origin.latched,
        });
        Ok(explain::explain(
            (surface_id, page_id, control_id),
            control,
            overlay,
            inner.derived.get(&key).cloned(),
            &self.hub,
            &sources,
        ))
    }

    /// What a draft of a control's feedback comes to, without saving it: the
    /// issues `update_control` would refuse, what Rust can prove about the
    /// rules, the limits, and what the draft would explain right now. Reads the
    /// workspace and the hub and changes neither.
    pub fn review_feedback(
        &self,
        surface_id: &str,
        page_id: &str,
        control: &Control,
    ) -> Result<FeedbackReview, String> {
        let stored = {
            let inner = self.lock();
            let surface = inner
                .workspace
                .surface(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = surface
                .page(page_id)
                .ok_or_else(|| format!("unknown page `{page_id}`"))?;
            page.controls.iter().find(|c| c.id == control.id).cloned()
        };
        Ok(review::review(
            (surface_id, page_id),
            control,
            stored.as_ref(),
            &self.hub,
            &self.observable_sources(),
        ))
    }

    fn on_event(self: &Arc<Self>, event: Event) {
        // Projection events are for event rules and external listeners; they
        // would drown the event log.
        if !is_projection(&event) {
            {
                let mut inner = self.lock();
                inner.recent.push_back(event.clone());
                while inner.recent.len() > RECENT_EVENTS {
                    inner.recent.pop_front();
                }
            }
            self.emitter.emit(
                ui::EVENT,
                serde_json::to_value(&event).unwrap_or(Value::Null),
            );
        }
        self.route_events(&event);
    }

    /// Event rules: the first matching rule of each control shows its look as
    /// a temporary overlay. The event is not stored anywhere else.
    fn route_events(self: &Arc<Self>, event: &Event) {
        // (key, rule, look, hold, the control has a look called `idle` of its own)
        let mut matched: Vec<(String, usize, String, Hold, bool)> = Vec::new();
        {
            let inner = self.lock();
            for r in inner.index.for_event(&event.source, &event.kind) {
                let Some(control) = find_control(&inner.workspace, r) else {
                    continue;
                };
                if let Some((rule, look, hold)) = event_look(control, event) {
                    matched.push((r.key(), rule, look, hold, control.states.contains_key(IDLE)));
                }
            }
        }
        if matched.is_empty() {
            return;
        }
        self.counters
            .events_routed
            .fetch_add(matched.len() as u64, Ordering::Relaxed);
        for (key, rule, look, hold, own_idle) in matched {
            let origin = Origin {
                source: OverlaySource::Event,
                rule: Some(rule),
                latched: matches!(hold, Hold::Latch),
            };
            // `idle` means "no temporary look" unless the control draws one.
            if look == IDLE && !own_idle {
                self.set_overlay(&key, None, None, origin);
                continue;
            }
            let after = match hold {
                Hold::For(ms) => Some(Duration::from_millis(u64::from(ms))),
                Hold::Latch => None,
            };
            self.set_overlay(&key, Some(&look), after, origin);
        }
    }

    // -----------------------------------------------------------------------
    // Rendering and devices
    // -----------------------------------------------------------------------

    pub fn render_tick(&self) {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let Inner {
                workspace,
                active_surface,
                active_page,
                overlay,
                derived,
                devices,
                ..
            } = &mut *inner;
            let page = workspace
                .surface(active_surface)
                .and_then(|s| s.page(active_page));
            for (id, caps) in devices.connected_capabilities() {
                // A temporary look over the one the state rules give.
                let state_of = |cid: &str| {
                    let key = control_key(active_surface, active_page, cid);
                    overlay
                        .get(&key)
                        .map(|e| e.name.clone())
                        .or_else(|| derived.get(&key).cloned())
                };
                let frame = renderer::compose(page, &state_of, &caps, t_ms);
                if let Some(change) = devices.render(&id, &frame) {
                    changes.push(change);
                }
            }
        }
        self.announce(changes);
    }

    fn announce(&self, changes: Vec<DeviceChange>) {
        if changes.is_empty() {
            return;
        }
        for change in changes {
            match change {
                DeviceChange::Connected(id) => self.publish(
                    "runtime",
                    types::DEVICE_CONNECTED,
                    json!({ "deviceId": id }),
                ),
                DeviceChange::Disconnected(id) => self.publish(
                    "runtime",
                    types::DEVICE_DISCONNECTED,
                    json!({ "deviceId": id }),
                ),
            }
        }
        self.emit_state();
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.lock().devices.list()
    }

    pub fn connect_device(&self, id: &str) -> Result<(), String> {
        let change = self.lock().devices.connect(id).map_err(|e| e.to_string())?;
        self.announce(vec![change]);
        Ok(())
    }

    pub fn disconnect_device(&self, id: &str) -> Result<(), String> {
        let change = self
            .lock()
            .devices
            .disconnect(id)
            .map_err(|e| e.to_string())?;
        self.announce(vec![change]);
        Ok(())
    }

    /// Look for Launchpad X hardware; connect newly found units.
    pub fn rescan(&self) {
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let (added, removed) = inner.devices.rescan_hardware();
            changes.extend(removed);
            for id in added {
                match inner.devices.connect(&id) {
                    Ok(change) => changes.push(change),
                    Err(e) => log::warn!("found `{id}` but could not connect: {e}"),
                }
            }
        }
        // A newly registered but unconnected device also changes the list.
        self.announce(changes);
        self.emit_state();
    }

    // -----------------------------------------------------------------------
    // Editing
    // -----------------------------------------------------------------------

    pub fn update_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control: Control,
    ) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = surface
                .pages
                .iter_mut()
                .find(|p| p.id == page_id)
                .ok_or_else(|| format!("unknown page `{page_id}`"))?;
            validate_control(page, &control)?;
            let mut control = control;
            let stored = page.controls.iter().find(|c| c.id == control.id);
            // Rules that are new or changed must name a state something
            // offers, with arguments that fit it. Saved ones stay as they are.
            let hub = &self.hub;
            control.feedback = prepare_rules(
                &control.feedback,
                stored.map_or(&[][..], |c| &c.feedback[..]),
                &|provider, id| hub.definition(provider, id),
            )
            .map_err(|issues| describe_issues(&issues))?;
            // Only an assignment that changed is checked. One that is saved
            // already stays editable even when its provider is gone, so a pad
            // can still be relabelled while its plugin is uninstalled.
            let registry = self.engine.registry();
            let prepare = |new: &mut Option<Assignment>,
                           old: Option<&Assignment>,
                           slot: &str|
             -> Result<(), String> {
                if let Some(a) = new.as_ref().filter(|a| Some(*a) != old) {
                    let prepared = registry
                        .prepare(a)
                        .map_err(|e| format!("{slot} action: {e}"))?;
                    *new = Some(prepared);
                }
                Ok(())
            };
            prepare(
                &mut control.press,
                stored.and_then(|c| c.press.as_ref()),
                "Press",
            )?;
            prepare(
                &mut control.long_press,
                stored.and_then(|c| c.long_press.as_ref()),
                "Long press",
            )?;
            let control_id = control.id.clone();
            let key = control_key(surface_id, page_id, &control_id);
            match page.controls.iter_mut().find(|c| c.id == control.id) {
                Some(slot) => *slot = control,
                None => page.controls.push(control),
            }
            // An edit starts the control over: no temporary look survives it,
            // and its look is derived again from the values the hub holds.
            inner.overlay.remove(&key);
            self.rebuild_index(&mut inner);
            let r = ControlRef {
                surface: surface_id.to_string(),
                page: page_id.to_string(),
                control: control_id,
            };
            inner.derived.remove(&key);
            self.derive_into(&mut inner, &r);
            self.persist(&inner);
        }
        self.emit_workspace();
        Ok(())
    }

    pub fn remove_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control_id: &str,
    ) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = surface
                .pages
                .iter_mut()
                .find(|p| p.id == page_id)
                .ok_or_else(|| format!("unknown page `{page_id}`"))?;
            let before = page.controls.len();
            page.controls.retain(|c| c.id != control_id);
            if page.controls.len() == before {
                return Err(format!("unknown control `{control_id}`"));
            }
            let key = control_key(surface_id, page_id, control_id);
            inner.overlay.remove(&key);
            inner.derived.remove(&key);
            self.rebuild_index(&mut inner);
            self.persist(&inner);
        }
        self.emit_workspace();
        Ok(())
    }

    pub fn create_surface(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("name must be 1-40 characters".into());
        }
        let id = {
            let mut inner = self.lock();
            let base: String = name
                .to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            let base = if base.is_empty() {
                "surface".to_string()
            } else {
                base
            };
            let mut id = base.clone();
            let mut n = 2;
            while inner.workspace.surface(&id).is_some() {
                id = format!("{base}-{n}");
                n += 1;
            }
            inner.workspace.surfaces.push(blank_surface(&id, name));
            self.persist(&inner);
            id
        };
        self.emit_workspace();
        Ok(id)
    }

    pub fn add_page(&self, surface_id: &str, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("name must be 1-40 characters".into());
        }
        let id = {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let base: String = name
                .to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            let base = if base.is_empty() {
                "page".to_string()
            } else {
                base
            };
            let mut id = base.clone();
            let mut n = 2;
            while surface.page(&id).is_some() {
                id = format!("{base}-{n}");
                n += 1;
            }
            surface.pages.push(Page {
                id: id.clone(),
                name: name.to_string(),
                controls: Vec::new(),
            });
            self.persist(&inner);
            id
        };
        self.emit_workspace();
        Ok(id)
    }

    pub fn reset_workspace(&self) {
        {
            let mut inner = self.lock();
            let prefs = inner.workspace.preferences.clone();
            let mut ws = default_workspace();
            ws.preferences = prefs;
            let first = ws
                .surfaces
                .first()
                .map(|s: &Surface| s.id.clone())
                .unwrap_or_default();
            inner.active_page = ws
                .surface(&first)
                .and_then(|s| s.home_page())
                .map(|p| p.id.clone())
                .unwrap_or_default();
            inner.active_surface = first;
            ws.last_active_surface = Some(inner.active_surface.clone());
            inner.workspace = ws;
            inner.overlay.clear();
            inner.held.clear();
            self.rebuild_index(&mut inner);
            self.evaluate_all(&mut inner);
            self.persist(&inner);
        }
        self.emit_workspace();
    }

    pub fn update_preferences(&self, preferences: Preferences) {
        {
            let mut inner = self.lock();
            inner.workspace.preferences = preferences;
            self.persist(&inner);
        }
        self.emit_workspace();
    }

    /// Publish an event as if it had arrived over the external interface.
    pub fn publish_external(&self, event: Event) {
        self.bus.publish(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Animation, ControlPos, Rgb, Visual};
    use crate::plugins::keyboard::{Combo, Keyboard};
    use crate::plugins::ActionError;
    use crate::state::feedback::FeedbackRule;

    struct NoKeyboard;
    impl Keyboard for NoKeyboard {
        fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
            Ok(())
        }
    }

    /// A generic surface for the runtime tests: one pad that mirrors a demo
    /// agent, one that also listens to an external source, and a big approval
    /// pad. Nothing here depends on a shipped surface.
    fn bound_workspace() -> Workspace {
        use crate::core::model::Span;
        use crate::state::feedback::Test;
        use crate::state::StateAddress;
        let blue = Rgb::new(40, 110, 255);
        let green = Rgb::new(30, 220, 90);
        let red = Rgb::new(255, 40, 50);
        let white = Rgb::new(255, 255, 255);
        // The look named after the agent's state, resting when it is idle.
        let mirror = |agent: &str| {
            FeedbackRule::state(StateAddress::new("demo", agent), Test::HasValue).show_from_value()
        };
        // An external source's events, which the pad keeps showing until the
        // next one (the behaviour schema 3 bindings had).
        let on = |kind: &str, look: &str| {
            FeedbackRule::event("beta", kind)
                .show(look)
                .hold(Hold::Latch)
        };
        let look = |c: &mut Control| {
            c.states.insert("running".into(), Visual::pulse(green));
            c.states
                .insert("waiting".into(), Visual::pulse(Rgb::new(170, 80, 255)));
            c.states
                .insert("approval-required".into(), Visual::pulse(white));
            c.states.insert("success".into(), Visual::solid(green));
            c.states.insert("error".into(), Visual::flash(red));
        };
        let start = |agent: &str| Assignment::new("demo", "startTask", json!({ "agent": agent }));

        let mut alpha = Control::new(ControlPos::grid(0, 0), "Alpha", blue);
        alpha.press = Some(start("alpha"));
        alpha.long_press = Some(Assignment::new(
            "demo",
            "stopTask",
            json!({ "agent": "alpha" }),
        ));
        look(&mut alpha);
        alpha.feedback = vec![mirror("alpha")];

        let mut beta = Control::new(ControlPos::grid(1, 0), "Beta", blue);
        beta.press = Some(start("beta"));
        look(&mut beta);
        beta.feedback = vec![
            mirror("beta"),
            on("task.started", "running"),
            on("task.waiting", "waiting"),
            // The first matching rule wins: the explicit failure goes first.
            FeedbackRule::event("beta", "task.completed")
                .where_field("success", false)
                .show("error")
                .hold(Hold::Latch),
            on("task.completed", "success"),
            on("task.failed", "error"),
        ];

        let mut approve = Control::new(ControlPos::grid(6, 2), "Approve", white.scaled(0.25));
        approve.span = Span { w: 2, h: 2 };
        approve.press = Some(Assignment::new("demo", "approve", json!({})));
        approve
            .states
            .insert("pending".into(), Visual::flash(white));
        approve.feedback = vec![FeedbackRule::state(
            StateAddress::new("demo", "alpha"),
            Test::Equals {
                value: "approval-required".into(),
            },
        )
        .show("pending")];

        Workspace {
            schema_version: crate::core::model::SCHEMA_VERSION,
            surfaces: vec![Surface {
                id: "bound".into(),
                name: "Bound".into(),
                description: String::new(),
                icon: None,
                category: "custom".into(),
                pages: vec![Page {
                    id: "home".into(),
                    name: "Home".into(),
                    controls: vec![alpha, beta, approve],
                }],
                context: None,
            }],
            last_active_surface: Some("bound".into()),
            preferences: Preferences::default(),
        }
    }

    async fn start(ws: Workspace, store: Option<Store>) -> Arc<Runtime> {
        Runtime::start(RuntimeConfig {
            workspace: ws,
            store,
            emitter: Arc::new(NullEmitter),
            keyboard: Arc::new(NoKeyboard),
            scan_hardware: false,
            serve_external: false,
            demo_pace: 1.0,
            plugin_dirs: vec![],
            secrets: None,
        })
        .await
    }

    async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        for _ in 0..300 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    fn tap(rt: &Arc<Runtime>, col: u8, row: u8) {
        let h = rt.virtual_handle();
        h.press(ControlPos::grid(col, row), 100).unwrap();
        h.release(ControlPos::grid(col, row)).unwrap();
    }

    fn state(rt: &Runtime, surface: &str, page: &str, control: &str) -> Option<String> {
        rt.state()
            .control_states
            .get(&control_key(surface, page, control))
            .cloned()
    }

    fn led(rt: &Runtime, col: u8, row: u8) -> crate::devices::Led {
        rt.virtual_handle()
            .frame()
            .get(ControlPos::grid(col, row))
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn starts_on_the_last_active_surface_with_the_virtual_device_connected() {
        let rt = start(default_workspace(), None).await;
        let s = rt.state();
        assert_eq!(s.active_surface_id, "agent-control");
        assert_eq!(s.active_page_id, "home");
        assert_eq!(s.mode, Mode::Live);
        assert!(s
            .devices
            .iter()
            .any(|d| d.identity.id == VIRTUAL_ID && d.connected));
        wait_until("first frame", || {
            !rt.virtual_handle().frame().leds.is_empty()
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn surface_and_page_switching_including_navigation_pads() {
        let rt = start(default_workspace(), None).await;
        rt.activate_surface("development", None).unwrap();
        assert_eq!(rt.state().active_surface_id, "development");
        assert!(rt.activate_surface("nope", None).is_err());
        assert!(rt.activate_page("nope").is_err());
        // The Git pad is a page navigation action.
        tap(&rt, 0, 2);
        wait_until("git page", || rt.state().active_page_id == "git").await;
        // Back returns home.
        tap(&rt, 0, 7);
        wait_until("home page", || rt.state().active_page_id == "home").await;
        // The grid changes instantly: Terminal (0,0) is lit on dev home.
        assert_ne!(led(&rt, 0, 0).color, Rgb::OFF);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn top_row_buttons_switch_surfaces() {
        let rt = start(default_workspace(), None).await;
        rt.virtual_handle()
            .press(ControlPos::Top { index: 1 }, 100)
            .unwrap();
        rt.virtual_handle()
            .release(ControlPos::Top { index: 1 })
            .unwrap();
        wait_until("development", || {
            rt.state().active_surface_id == "development"
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pad_press_runs_a_plugin_action_and_the_bound_pad_follows_the_state() {
        let rt = start(bound_workspace(), None).await;
        // Alpha pad: tap starts a simulated task.
        tap(&rt, 0, 0);
        wait_until("running", || {
            state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
        })
        .await;
        wait_until("green pulse on the grid", || {
            let l = led(&rt, 0, 0);
            l.animation == Animation::Pulse && l.color.g > l.color.r
        })
        .await;
        // Other agents are untouched.
        assert_eq!(state(&rt, "bound", "home", "g.1.0"), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn external_events_route_to_bound_controls() {
        let rt = start(bound_workspace(), None).await;
        rt.publish_external(Event::new("beta", "task.started", Value::Null));
        wait_until("beta running", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
        })
        .await;
        rt.publish_external(Event::new(
            "beta",
            "task.completed",
            json!({ "success": false }),
        ));
        wait_until("beta error", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("error")
        })
        .await;
        wait_until("red flash", || {
            let l = led(&rt, 1, 0);
            l.animation == Animation::Flash && l.color.r > l.color.g
        })
        .await;
        rt.publish_external(Event::new(
            "beta",
            "task.completed",
            json!({ "success": true }),
        ));
        wait_until("beta success", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("success")
        })
        .await;
        // The documented minimal message has no payload and means success.
        rt.publish_external(Event::new("beta", "task.started", Value::Null));
        wait_until("beta running again", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
        })
        .await;
        rt.publish_external(Event::new("beta", "task.completed", Value::Null));
        wait_until("bare completion is success", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("success")
        })
        .await;
        // Events from other sources do not leak into this pad.
        rt.publish_external(Event::new("someone-else", "task.started", Value::Null));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            state(&rt, "bound", "home", "g.1.0").as_deref(),
            Some("success")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approval_pad_flashes_until_the_user_approves() {
        let rt = start(bound_workspace(), None).await;
        wait_until("first frame", || {
            !rt.virtual_handle().frame().leds.is_empty()
        })
        .await;
        rt.run_action(Assignment::new(
            "demo",
            "requestApproval",
            json!({ "agent": "alpha" }),
        ))
        .await
        .unwrap();
        // Approve is a 2x2 pad at (6,2).
        wait_until("approve pending", || {
            state(&rt, "bound", "home", "g.6.2").as_deref() == Some("pending")
        })
        .await;
        wait_until("white flash on all four cells", || {
            [(6, 2), (7, 2), (6, 3), (7, 3)].iter().all(|&(c, r)| {
                let l = led(&rt, c, r);
                l.animation == Animation::Flash && l.color == Rgb::new(255, 255, 255)
            })
        })
        .await;
        // Pressing any cell of the big pad approves.
        tap(&rt, 7, 3);
        wait_until("alpha running after approval", || {
            state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
        })
        .await;
        wait_until("approve pad idle again", || {
            state(&rt, "bound", "home", "g.6.2").is_none()
        })
        .await;
    }

    fn one_pad_workspace(command: &str) -> Workspace {
        let mut control = Control::new(ControlPos::grid(0, 0), "Task", Rgb::new(0, 0, 255));
        control.press = Some(Assignment::new(
            "shell",
            "execute",
            json!({ "command": command }),
        ));
        control
            .states
            .insert("running".into(), Visual::pulse(Rgb::new(255, 200, 0)));
        control
            .states
            .insert("success".into(), Visual::solid(Rgb::new(0, 255, 0)));
        control
            .states
            .insert("error".into(), Visual::flash(Rgb::new(255, 0, 0)));
        let mut ws = default_workspace();
        ws.surfaces = vec![Surface {
            id: "t".into(),
            name: "T".into(),
            description: String::new(),
            icon: None,
            category: "custom".into(),
            pages: vec![Page {
                id: "home".into(),
                name: "Home".into(),
                controls: vec![control],
            }],
            context: None,
        }];
        ws.last_active_surface = Some("t".into());
        ws
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn unbound_pad_goes_running_then_success_and_emits_action_events() {
        let rt = start(one_pad_workspace("sleep 0.4; echo ok"), None).await;
        let mut rx = rt.bus().subscribe();
        tap(&rt, 0, 0);
        wait_until("running", || {
            state(&rt, "t", "home", "g.0.0").as_deref() == Some("running")
        })
        .await;
        wait_until("yellow pulse", || {
            led(&rt, 0, 0).animation == Animation::Pulse
        })
        .await;
        wait_until("success", || {
            state(&rt, "t", "home", "g.0.0").as_deref() == Some("success")
        })
        .await;
        wait_until("solid green", || {
            led(&rt, 0, 0).color == Rgb::new(0, 255, 0)
        })
        .await;
        let mut kinds = Vec::new();
        while let Ok(e) = rx.try_recv() {
            kinds.push(e.kind);
        }
        let pos = |k: &str| kinds.iter().position(|x| x == k);
        assert!(pos(types::CONTROL_PRESSED).unwrap() < pos(types::ACTION_STARTED).unwrap());
        assert!(pos(types::ACTION_STARTED).unwrap() < pos(types::ACTION_COMPLETED).unwrap());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn failing_command_ends_in_the_error_state() {
        let rt = start(one_pad_workspace("exit 7"), None).await;
        let mut rx = rt.bus().subscribe();
        tap(&rt, 0, 0);
        wait_until("error", || {
            state(&rt, "t", "home", "g.0.0").as_deref() == Some("error")
        })
        .await;
        let mut failed = false;
        while let Ok(e) = rx.try_recv() {
            if e.kind == types::ACTION_FAILED {
                assert!(e.payload["error"].as_str().unwrap().contains("exit 7"));
                failed = true;
            }
        }
        assert!(failed, "action.failed was published");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn edit_mode_selects_instead_of_executing() {
        let rt = start(bound_workspace(), None).await;
        let mut rx = rt.bus().subscribe();
        rt.set_mode(Mode::Edit);
        tap(&rt, 0, 0);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            state(&rt, "bound", "home", "g.0.0"),
            None,
            "no action in edit mode"
        );
        let mut pressed = false;
        while let Ok(e) = rx.try_recv() {
            pressed |= e.kind == types::CONTROL_PRESSED && e.payload["controlId"] == "g.0.0";
        }
        assert!(
            pressed,
            "control.pressed still tells the editor which control was hit"
        );
        rt.set_mode(Mode::Live);
        tap(&rt, 0, 0);
        wait_until("runs in live mode", || {
            state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn long_press_wins_over_tap() {
        let rt = start(bound_workspace(), None).await;
        // Alpha pad: tap = start, long press = stop.
        tap(&rt, 0, 0);
        wait_until("started by tap", || {
            state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
        })
        .await;
        let h = rt.virtual_handle();
        h.press(ControlPos::grid(0, 0), 100).unwrap();
        tokio::time::sleep(LONG_PRESS + Duration::from_millis(250)).await;
        wait_until("stopped by long press", || {
            state(&rt, "bound", "home", "g.0.0").is_none()
        })
        .await;
        h.release(ControlPos::grid(0, 0)).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            state(&rt, "bound", "home", "g.0.0"),
            None,
            "release after a long press must not also tap"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn edits_and_the_active_surface_persist_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
        let mut control = rt
            .snapshot()
            .workspace
            .surface("development")
            .unwrap()
            .page("home")
            .unwrap()
            .control("g.0.0")
            .unwrap()
            .clone();
        control.label = "My Terminal".into();
        control.visual = Visual::solid(Rgb::new(1, 2, 3));
        control.press = Some(Assignment::new(
            "shell",
            "execute",
            json!({ "command": "echo hi" }),
        ));
        rt.update_control("development", "home", control).unwrap();
        rt.activate_surface("development", None).unwrap();

        // "Restart": load from the same directory.
        let loaded = Store::new(dir.path()).load_or_default(|| panic!("workspace file must exist"));
        let c = loaded
            .surface("development")
            .unwrap()
            .page("home")
            .unwrap()
            .control("g.0.0")
            .unwrap();
        assert_eq!(c.label, "My Terminal");
        assert_eq!(c.visual.color, Rgb::new(1, 2, 3));
        assert_eq!(
            c.press,
            Some(Assignment::new(
                "shell",
                "execute",
                json!({ "command": "echo hi" })
            ))
        );
        assert_eq!(loaded.last_active_surface.as_deref(), Some("development"));
        let rt2 = start(loaded, Some(Store::new(dir.path()))).await;
        assert_eq!(rt2.state().active_surface_id, "development");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invalid_edits_are_rejected_and_do_not_change_anything() {
        let rt = start(default_workspace(), None).await;
        let before = rt.snapshot().workspace;
        let mut clash = Control::new(ControlPos::grid(1, 0), "Clash", Rgb::new(1, 1, 1));
        clash.id = "clash".into();
        assert!(rt.update_control("development", "home", clash).is_err());
        let mut bad = Control::new(ControlPos::grid(7, 7), "Bad", Rgb::new(1, 1, 1));
        bad.press = Some(Assignment::new(
            "system",
            "openUrl",
            json!({ "url": "file:///etc/passwd" }),
        ));
        assert!(rt.update_control("development", "home", bad).is_err());
        assert!(rt
            .update_control(
                "nope",
                "home",
                Control::new(ControlPos::grid(7, 7), "X", Rgb::OFF)
            )
            .is_err());
        assert_eq!(rt.snapshot().workspace, before);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn new_controls_surfaces_and_pages_can_be_created_and_removed() {
        let rt = start(default_workspace(), None).await;
        let id = rt.create_surface("My Surface").unwrap();
        assert_eq!(id, "my-surface");
        assert_eq!(rt.create_surface("My Surface").unwrap(), "my-surface-2");
        let page = rt.add_page(&id, "Extra").unwrap();
        let mut c = Control::new(ControlPos::grid(3, 3), "New", Rgb::new(9, 9, 9));
        c.press = Some(Assignment::new(
            "system",
            "openUrl",
            json!({ "url": "https://example.com" }),
        ));
        rt.update_control(&id, &page, c).unwrap();
        assert_eq!(
            rt.snapshot()
                .workspace
                .surface(&id)
                .unwrap()
                .page(&page)
                .unwrap()
                .controls
                .len(),
            1
        );
        rt.remove_control(&id, &page, "g.3.3").unwrap();
        assert!(rt.remove_control(&id, &page, "g.3.3").is_err());
        assert!(rt.create_surface("   ").is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn disconnecting_the_virtual_device_stops_input() {
        let rt = start(default_workspace(), None).await;
        rt.disconnect_device(VIRTUAL_ID).unwrap();
        assert!(rt
            .virtual_handle()
            .press(ControlPos::grid(0, 0), 100)
            .is_err());
        assert!(!rt.state().devices[0].connected);
        rt.connect_device(VIRTUAL_ID).unwrap();
        assert!(rt
            .virtual_handle()
            .press(ControlPos::grid(0, 0), 100)
            .is_ok());
        assert!(rt.connect_device("missing").is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn external_websocket_events_reach_pads_end_to_end() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message};
        let mut ws = bound_workspace();
        ws.preferences.external_events_port = 0; // any free port
        let rt = Runtime::start(RuntimeConfig {
            workspace: ws,
            store: None,
            emitter: Arc::new(NullEmitter),
            keyboard: Arc::new(NoKeyboard),
            scan_hardware: false,
            serve_external: true,
            demo_pace: 1.0,
            plugin_dirs: vec![],
            secrets: None,
        })
        .await;
        let ext = rt.state().external;
        assert!(ext.listening, "{:?}", ext.error);
        let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}", ext.port))
            .await
            .unwrap();
        socket
            .send(Message::text(
                r#"{"type":"event","source":"beta","event":"task.started"}"#,
            ))
            .await
            .unwrap();
        assert!(socket
            .next()
            .await
            .unwrap()
            .unwrap()
            .to_text()
            .unwrap()
            .contains("ack"));
        wait_until("beta pad running via websocket", || {
            state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
        })
        .await;
        // An attempt to run something is refused and changes nothing.
        socket
            .send(Message::text(
                r#"{"type":"exec","command":"touch /tmp/pwned"}"#,
            ))
            .await
            .unwrap();
        assert!(socket
            .next()
            .await
            .unwrap()
            .unwrap()
            .to_text()
            .unwrap()
            .contains("error"));
    }
}
