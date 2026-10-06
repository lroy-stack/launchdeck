//! One computer for the release acceptance scenario: the app's own start
//! (the workspace loaded from a data folder, or the starters when there is
//! none, plugins found in `<data>/plugins`, exports saved in a folder of
//! their own), headless. Secrets are kept in memory, a scripted observer
//! says which application is in front (X11 identities, whatever system runs
//! the test), the keyboard counts its presses, and nothing looks for
//! hardware or serves the network. Waits are for a condition or an event,
//! with a deadline; nothing sleeps for a fixed time.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use launchdeck_lib::artifacts::ArtifactService;
use launchdeck_lib::context::identity::X11;
use launchdeck_lib::context::observer::scripted::{Script, ScriptedObserver};
use launchdeck_lib::context::{ApplicationIdentity, ObserverStatus, Report, Seen};
use launchdeck_lib::core::model::ControlPos;
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::events::Event;
use launchdeck_lib::persistence::{LoadOutcome, Store};
use launchdeck_lib::plugins::external::{plugin_roots, InstallError};
use launchdeck_lib::surfaces::default_workspace;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast::Receiver;

use crate::artifact_world::RecordingKeyboard;

/// The files next to the workspace that a refused import must leave as they
/// are, when they exist.
pub const SETTINGS_FILES: [&str; 4] = [
    "workspace.json",
    "plugins.json",
    "permissions.json",
    "agents.json",
];

/// Generous: CI is slow, and a wait that ends early costs nothing.
const PATIENCE: Duration = Duration::from_secs(20);

pub struct Machine {
    pub rt: Arc<Runtime>,
    pub service: ArtifactService,
    pub keyboard: Arc<RecordingKeyboard>,
    /// What the test says is in front.
    pub script: Script,
    /// The app data folder: the workspace, the other settings, `plugins/`.
    pub data: PathBuf,
    pub exports: PathBuf,
    root: tempfile::TempDir,
}

impl Machine {
    /// The first start on a computer that never ran LaunchDeck.
    pub async fn first_start() -> Machine {
        let root = tempfile::tempdir().unwrap();
        Self::open(root).await
    }

    /// Quit and start again from the same folders. The old runtime gives
    /// its devices back and its observer stops, as when the app quits.
    pub async fn restart(self) -> Machine {
        let Machine {
            rt, script, root, ..
        } = self;
        rt.release_devices();
        drop((rt, script));
        Self::open(root).await
    }

    async fn open(root: tempfile::TempDir) -> Machine {
        let data = root.path().join("data");
        let exports = root.path().join("exports");
        let store = Store::new(&data);
        let LoadOutcome { workspace, notice } = store.load(default_workspace);
        let keyboard = Arc::new(RecordingKeyboard::default());
        let (observer, script) =
            ScriptedObserver::new(ObserverStatus::available("the X11 window manager"));
        let rt = Runtime::start_with_notices(
            RuntimeConfig {
                workspace,
                store: Some(store),
                emitter: Arc::new(NullEmitter),
                keyboard: keyboard.clone(),
                scan_hardware: false,
                serve_external: false,
                demo_pace: 0.05,
                plugin_dirs: plugin_roots(&data, None),
                secrets: None,
                observer: Some(Box::new(observer)),
            },
            notice.into_iter().collect(),
        )
        .await;
        let service = ArtifactService::new(rt.clone(), exports.clone());
        Machine {
            rt,
            service,
            keyboard,
            script,
            data,
            exports,
            root,
        }
    }

    /// Where the app installs plugins.
    pub fn plugins(&self) -> PathBuf {
        self.data.join("plugins")
    }

    /// The Plugins screen's Install for an example the app ships, through
    /// the same host call, off the async threads as the command runs it.
    pub async fn install_example(&self, id: &str) -> Result<(), InstallError> {
        let host = self.rt.plugin_host().clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || host.install_example(&examples_root(), &id))
            .await
            .unwrap()
    }

    /// The Plugins screen's Rescan.
    pub async fn rescan(&self) {
        let host = self.rt.plugin_host().clone();
        tokio::task::spawn_blocking(move || host.scan())
            .await
            .unwrap();
    }

    /// `id` comes to the front and stays long enough to count.
    pub async fn front(&self, id: &str) {
        let identity = application(id);
        self.script.report(Report::Seen(Seen::Application {
            identity: identity.clone(),
            own: false,
        }));
        wait_until(&format!("{id} to be in front"), || {
            self.rt.context().application.as_ref() == Some(&identity)
        })
        .await;
    }

    pub async fn until_shown(&self, surface: &str) {
        wait_until(&format!("{surface} to be shown"), || {
            self.rt.state().active_surface_id == surface
        })
        .await;
    }

    /// Until the virtual device shows `led` at `pos`.
    pub async fn until_lit(&self, what: &str, pos: ControlPos, led: Led) {
        wait_until(what, || {
            self.rt.virtual_handle().frame().get(pos) == Some(led)
        })
        .await;
    }

    /// A press and release of the virtual device's pad at `pos`.
    pub fn press(&self, pos: ControlPos) {
        let handle = self.rt.virtual_handle();
        handle.press(pos, 100).unwrap();
        handle.release(pos).unwrap();
    }

    /// The SHA-256 of each settings file that exists.
    pub fn settings_digests(&self) -> Vec<(&'static str, String)> {
        SETTINGS_FILES
            .iter()
            .filter_map(|name| {
                let bytes = fs::read(self.data.join(name)).ok()?;
                Some((*name, format!("{:x}", Sha256::digest(bytes))))
            })
            .collect()
    }
}

/// Where the example plugins are, as a debug build of the app finds them.
pub fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

pub fn application(id: &str) -> ApplicationIdentity {
    ApplicationIdentity::new(X11, id, id)
}

pub async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + PATIENCE;
    while Instant::now() < until {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// The next event of `kind` from `source` on the bus.
pub async fn next_event(events: &mut Receiver<Event>, source: &str, kind: &str) -> Event {
    let wanted = async {
        loop {
            match events.recv().await {
                Ok(e) if e.source == source && e.kind == kind => return e,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("the bus closed: {e}"),
            }
        }
    };
    tokio::time::timeout(PATIENCE, wanted)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {source} {kind}"))
}
