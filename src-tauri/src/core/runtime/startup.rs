//! Startup: the composition root. Builds every provider, the plugin host,
//! the devices and the runtime, evaluates every control once, then starts
//! the loops (input, state, events, rendering, plugin refresh, hardware
//! scan, the external WebSocket).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use super::{ui, Counters, ExternalStatus, Inner, Mode, Runtime, RuntimeConfig, UiEmitter};
use crate::actions::builtin::{HttpProvider, NavigationProvider, ShellProvider};
use crate::actions::ActionEngine;
use crate::agents::adapter::AdapterState;
use crate::agents::adapters::HookManager;
use crate::agents::bridge::BridgeAdapter;
use crate::agents::demo::DemoAgentAdapter;
use crate::agents::notify::Notification;
use crate::agents::plugin::AgentControlPlugin;
use crate::agents::service::{AgentControl, AgentsSink, Timing};
use crate::agents::settings::AgentsStorage;
use crate::agents::view::AgentsSnapshot;
use crate::context::observer::{ForegroundObserver, NoObserver, Report, Sink};
use crate::context::Engine;
use crate::core::observables as native;
use crate::devices::manager::DeviceManager;
use crate::devices::virtual_device::{VirtualLaunchpadX, VIRTUAL_ID};
use crate::devices::DeviceInput;
use crate::events::EventBus;
use crate::persistence::StartupNotice;
use crate::plugins::demo::DemoPlugin;
use crate::plugins::external::{Limits as PluginLimits, PluginHost};
use crate::plugins::system::SystemPlugin;
use crate::plugins::PluginRegistry;
use crate::state::index::FeedbackIndex;
use crate::state::{Changes, StateHub};

const RENDER_INTERVAL: Duration = Duration::from_millis(40);
const RESCAN_INTERVAL: Duration = Duration::from_secs(3);
/// How often plugins that asked to be refreshed are checked for being due.
const REFRESH_TICK: Duration = Duration::from_millis(250);

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

impl Runtime {
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
        Self::start_with_notices(config, Vec::new()).await
    }

    /// [`Runtime::start`], with what loading the workspace has to tell the
    /// person (`persistence::Store::load`); the snapshot carries it.
    pub async fn start_with_notices(
        config: RuntimeConfig,
        notices: Vec<StartupNotice>,
    ) -> Arc<Runtime> {
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
        devices.set_preferred_brightness(config.workspace.preferences.device_brightness);
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

        let observer: Box<dyn ForegroundObserver> = config.observer.unwrap_or_else(|| {
            Box::new(NoObserver(
                "Nothing observes the application in front in this run.".into(),
            ))
        });
        let observing = observer.status();

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
                context: Engine::new(observing.clone(), observer.namespace(), false),
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
            context_sent: Mutex::new(None),
            emitting: Mutex::new(()),
            notices,
        });

        // Every control is evaluated once, from the states the providers
        // already hold, before anything can draw or react.
        {
            let mut inner = rt.lock();
            rt.publish_navigation(&inner.active_surface, &inner.active_page);
            rt.publish_context_mode(inner.context.mode(&inner.workspace.context));
            // What the providers declared so far is already in the evaluation
            // below; anything they write from now on is drained later.
            let _ = rt.hub.drain();
            rt.evaluate_all(&mut inner);
        }
        // Context: the observer has a thread of its own (it blocks on the
        // system's events); what it sees arrives here. Without one, nothing is
        // started and every surface change is the person's.
        if observing.can_observe() {
            let (tx, mut rx) = unbounded_channel::<Report>();
            let sink = Sink::new(move |report| {
                let _ = tx.send(report);
            });
            let started = std::thread::Builder::new()
                .name("foreground-observer".into())
                .spawn(move || observer.run(sink));
            match started {
                Ok(_) => {
                    let rt = rt.clone();
                    tokio::spawn(async move {
                        while let Some(report) = rx.recv().await {
                            rt.on_foreground(report);
                        }
                    });
                }
                Err(e) => {
                    rt.lock()
                        .context
                        .set_observer(crate::context::ObserverStatus::unavailable(&format!(
                            "The observer could not be started ({e})."
                        )));
                }
            }
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
}
