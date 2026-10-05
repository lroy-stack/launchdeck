//! A runtime on a real store in a temporary folder, with its artifact
//! service: what the tests that import need. The configuration (the three
//! files next to the workspace, and the workspace in memory) can be read at
//! any time to prove an import changed nothing; the keyboard, the `shell`
//! provider and the `acme.recorder` plugin record every call instead of
//! acting, so an import that ran anything would be seen.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use launchdeck_lib::actions::builtin::ShellProvider;
use launchdeck_lib::actions::definition::{Issue, ProviderKind};
use launchdeck_lib::actions::params::ParamDef;
use launchdeck_lib::artifacts::ArtifactService;
use launchdeck_lib::context::observer::scripted::ScriptedObserver;
use launchdeck_lib::context::ObserverStatus;
use launchdeck_lib::core::model::Workspace;
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::{ActionDef, ActionError, Plugin, PluginContext, PluginManifest};
use serde_json::Value;

/// The files next to the workspace that an import must never touch, and
/// the workspace itself.
pub const CONFIGURATION_FILES: [&str; 3] = ["workspace.json", "plugins.json", "permissions.json"];

/// What a plugin left in `plugins.json` and a grant left in
/// `permissions.json`: real content, so that a rewrite would show.
const PLUGINS_JSON: &str =
    r#"{"schemaVersion":1,"plugins":{"example.counter":{"enabled":true,"config":{"step":2}}}}"#;
const PERMISSIONS_JSON: &str = r#"{"schemaVersion":1,"plugins":{"example.home-assistant":{"network":{"http://192.168.1.20:8123":{"decision":"granted","at":1}}}}}"#;

#[derive(Default)]
pub struct RecordingKeyboard(AtomicUsize);

impl Keyboard for RecordingKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl RecordingKeyboard {
    pub fn presses(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// The `shell` provider as the runtime has it, except that a command is
/// counted, never run.
#[derive(Default)]
pub struct RecordingShell(AtomicUsize);

impl Plugin for RecordingShell {
    fn manifest(&self) -> PluginManifest {
        ShellProvider.manifest()
    }

    fn invoke(&self, _: &PluginContext, _: &str, _: &Value) -> Result<Value, ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    }

    fn check(&self, action: &str, args: &Value) -> Result<(), Issue> {
        ShellProvider.check(action, args)
    }
}

impl RecordingShell {
    pub fn runs(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// An installed plugin, `acme.recorder` 1.0.0, whose one action `record`
/// takes an address and counts its calls.
#[derive(Default)]
pub struct Recorder(AtomicUsize);

pub const RECORDER: &str = "acme.recorder";

impl Plugin for Recorder {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: RECORDER.into(),
            name: "Recorder".into(),
            version: "1.0.0".into(),
            description: "Counts its calls.".into(),
            kind: ProviderKind::Plugin,
            actions: vec![ActionDef::new("record", "Record", "Counts a call.")
                .param(ParamDef::string("url", "Address"))],
            states: Vec::new(),
            permissions: Vec::new(),
            external: None,
        }
    }

    fn invoke(&self, _: &PluginContext, _: &str, _: &Value) -> Result<Value, ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    }
}

impl Recorder {
    pub fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// The configuration as it is: each file's bytes (absent when there is no
/// file) and the workspace the runtime holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub files: Vec<(&'static str, Option<Vec<u8>>)>,
    pub workspace: Workspace,
}

pub struct World {
    pub rt: Arc<Runtime>,
    pub service: ArtifactService,
    pub keyboard: Arc<RecordingKeyboard>,
    pub shell: Arc<RecordingShell>,
    pub recorder: Arc<Recorder>,
    /// Where the workspace and the other configuration files are.
    pub data: PathBuf,
    /// Where exports are saved.
    pub exports: PathBuf,
    /// The folder plugins are found in (empty until a test copies one in).
    pub plugins: PathBuf,
    _root: tempfile::TempDir,
}

impl World {
    /// `workspace`, saved first, so that every configuration file exists.
    /// X11 identities (a scripted observer), whatever system runs the test.
    pub async fn start(workspace: Workspace) -> World {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let plugins = root.path().join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        let store = Store::new(&data);
        store.save(&workspace).unwrap();
        fs::write(data.join("plugins.json"), PLUGINS_JSON).unwrap();
        fs::write(data.join("permissions.json"), PERMISSIONS_JSON).unwrap();
        let world = Self::open(root, data, plugins, workspace).await;
        assert!(
            world.configuration().files.iter().all(|(_, f)| f.is_some()),
            "every configuration file exists"
        );
        world
    }

    /// A new runtime on the same folders, as after a restart; this one is
    /// left as it is.
    pub async fn restart(self) -> World {
        let workspace = Store::new(&self.data).load_or_default(|| panic!("the workspace is saved"));
        Self::open(self._root, self.data, self.plugins, workspace).await
    }

    async fn open(
        root: tempfile::TempDir,
        data: PathBuf,
        plugins: PathBuf,
        workspace: Workspace,
    ) -> World {
        let keyboard = Arc::new(RecordingKeyboard::default());
        let (observer, _script) =
            ScriptedObserver::new(ObserverStatus::available("the X11 window manager"));
        let rt = Runtime::start(RuntimeConfig {
            workspace,
            store: Some(Store::new(&data)),
            emitter: Arc::new(NullEmitter),
            keyboard: keyboard.clone(),
            scan_hardware: false,
            serve_external: false,
            demo_pace: 0.05,
            plugin_dirs: vec![plugins.clone()],
            secrets: None,
            observer: Some(Box::new(observer)),
        })
        .await;
        let shell = Arc::new(RecordingShell::default());
        let recorder = Arc::new(Recorder::default());
        let registry = rt.plugin_registry();
        registry.register(shell.clone());
        registry.register(recorder.clone());
        let exports = root.path().join("exports");
        let service = ArtifactService::new(rt.clone(), exports.clone());
        World {
            rt,
            service,
            keyboard,
            shell,
            recorder,
            data,
            exports,
            plugins,
            _root: root,
        }
    }

    pub fn configuration(&self) -> Configuration {
        Configuration {
            files: CONFIGURATION_FILES
                .iter()
                .map(|name| (*name, fs::read(self.data.join(name)).ok()))
                .collect(),
            workspace: self.rt.snapshot().workspace,
        }
    }

    /// Nothing was pressed, run or recorded.
    pub fn assert_nothing_ran(&self) {
        assert_eq!(self.keyboard.presses(), 0, "a key was pressed");
        assert_eq!(self.shell.runs(), 0, "a command ran");
        assert_eq!(self.recorder.calls(), 0, "a plugin action ran");
        assert!(self.rt.state().pending.is_empty(), "an action is running");
    }
}

/// Copy the files of `from` (not its folders) into `to`.
pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}
