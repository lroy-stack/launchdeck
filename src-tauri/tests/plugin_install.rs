//! Installing a bundled example plugin, through the host's install API, with
//! the real example plugins (`examples/plugins`).
//!
//! Each test copies the examples it needs into a temporary examples root, so a
//! test can damage one without touching the repository, and installs into a
//! temporary app data folder. Nothing here opens a window or a device: the
//! action that proves an installed plugin works goes through the same engine
//! and registry a pad press does.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use launchdeck_lib::actions::ActionEngine;
use launchdeck_lib::core::model::Assignment;
use launchdeck_lib::events::EventBus;
use launchdeck_lib::plugins::external::capabilities::GrantStatus;
use launchdeck_lib::plugins::external::{
    plugin_roots, ExampleInfo, InstallError, Limits, PluginHost, PluginStatus,
};
use launchdeck_lib::plugins::PluginRegistry;
use serde_json::{json, Value};

const HELLO: &str = "example.hello";
const COUNTER: &str = "example.counter";
const HA: &str = "example.home-assistant";
const SHIPPED: [&str; 3] = ["hello", "counter", "home-assistant"];

fn repo_examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

/// The regular files at the top of an example folder: its manifest and module,
/// and the build files that are not part of an installed plugin.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// Every file under `dir` with its bytes, and every folder, by relative path.
fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(dir).unwrap().display().to_string();
            if path.is_dir() {
                out.insert(name, None);
                stack.push(path);
            } else {
                out.insert(name, Some(fs::read(&path).unwrap()));
            }
        }
    }
    out
}

struct World {
    host: PluginHost,
    registry: PluginRegistry,
    examples: PathBuf,
    /// The app data folder: `plugins/`, `plugins.json`, `permissions.json`.
    data: PathBuf,
    _tmp: tempfile::TempDir,
}

impl World {
    fn plugins(&self) -> PathBuf {
        self.data.join("plugins")
    }

    fn example(&self, name: &str) -> PathBuf {
        self.examples.join(name)
    }

    fn install(&self, id: &str) -> Result<(), InstallError> {
        self.host.install_example(&self.examples, id)
    }

    fn listed(&self, id: &str) -> ExampleInfo {
        self.host
            .examples(&self.examples)
            .into_iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{id} is not offered"))
    }

    fn offered(&self) -> Vec<String> {
        self.host
            .examples(&self.examples)
            .into_iter()
            .map(|e| e.id)
            .collect()
    }

    /// No staging folder of an install is left in the plugins folder.
    fn assert_no_staging_left(&self) {
        for entry in fs::read_dir(self.plugins()).into_iter().flatten() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.contains(".tmp-"), "staging folder left: {name}");
        }
    }
}

/// The shipped examples in a temporary examples root, and a host over the app
/// data folder (and `extra`, a folder like those `LAUNCHDECK_PLUGINS` names).
fn world_with(names: &[&str], extra: Option<&Path>) -> World {
    world_saved(names, extra, None)
}

/// As [`world_with`], with `plugins.json` holding `saved` before the host starts.
fn world_saved(names: &[&str], extra: Option<&Path>, saved: Option<Value>) -> World {
    let tmp = tempfile::tempdir().unwrap();
    let examples = tmp.path().join("examples");
    for name in names {
        copy_dir(&repo_examples().join(name), &examples.join(name));
    }
    let data = tmp.path().join("data");
    if let Some(saved) = saved {
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("plugins.json"), saved.to_string()).unwrap();
    }
    let registry = PluginRegistry::default();
    let host = PluginHost::new(
        plugin_roots(&data, extra.map(|p| p.as_os_str().to_owned())),
        Some(data.join("plugins.json")),
        registry.clone(),
        Limits::default(),
    );
    host.scan();
    World {
        host,
        registry,
        examples,
        data,
        _tmp: tmp,
    }
}

fn world() -> World {
    world_with(&SHIPPED, None)
}

#[tokio::test(flavor = "multi_thread")]
async fn installing_hello_lists_it_and_its_action_runs() {
    let w = world();
    assert!(w.host.list().is_empty());
    assert!(!w.listed(HELLO).installed);

    w.install(HELLO).unwrap();

    // The scan that followed found it, and it is on: nothing else to do.
    let info = w.host.get(HELLO).unwrap();
    assert!(info.enabled);
    assert_eq!(
        info.plugin.external.as_ref().unwrap().status,
        PluginStatus::Enabled
    );
    assert!(w.host.issues().is_empty(), "{:?}", w.host.issues());
    // Into the app data plugins folder, a folder named after the id, holding
    // the manifest and the module it names and nothing else.
    let dir = w.plugins().join(HELLO);
    assert_eq!(PathBuf::from(&info.dir), dir);
    let files: Vec<String> = tree(&dir).into_keys().collect();
    assert_eq!(files, ["hello.wasm", "plugin.json"]);
    for file in ["hello.wasm", "plugin.json"] {
        assert_eq!(
            fs::read(dir.join(file)).unwrap(),
            fs::read(w.example("hello").join(file)).unwrap()
        );
    }
    w.assert_no_staging_left();
    assert!(w.listed(HELLO).installed);

    // Its action runs through the engine a pad press goes through.
    let engine = ActionEngine::new(w.registry.clone(), EventBus::new());
    let value = engine
        .execute(&Assignment::new(HELLO, "hello.say", json!({"name": "Ada"})))
        .await
        .unwrap();
    assert_eq!(value, json!({"message": "Hello, Ada!"}));
}

#[test]
fn a_second_install_is_refused_and_changes_nothing() {
    let w = world();
    w.install(HELLO).unwrap();
    let before = tree(&w.data);

    let err = w.install(HELLO).unwrap_err();
    assert!(matches!(err, InstallError::AlreadyInstalled(_)), "{err:?}");
    assert!(err.to_string().contains("already installed"), "{err}");
    assert_eq!(tree(&w.data), before);
    assert_eq!(w.host.list().len(), 1);
}

#[test]
fn a_damaged_example_is_refused_and_leaves_no_folder_behind() {
    type Spoil = fn(&Path);
    let damage: [(&str, Spoil); 3] = [
        ("a manifest cut short", |dir| {
            let text = fs::read_to_string(dir.join("plugin.json")).unwrap();
            fs::write(dir.join("plugin.json"), &text[..text.len() / 2]).unwrap();
        }),
        ("a manifest for another LaunchDeck", |dir| {
            let path = dir.join("plugin.json");
            let mut m: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            m["abi"] = json!(99);
            fs::write(&path, m.to_string()).unwrap();
        }),
        ("a module that is not WebAssembly", |dir| {
            fs::write(dir.join("hello.wasm"), b"MZ not a module").unwrap();
        }),
    ];
    for (what, spoil) in damage {
        let w = world();
        spoil(&w.example("hello"));

        assert!(!w.offered().contains(&HELLO.to_string()), "{what}");
        let err = w.install(HELLO).unwrap_err();
        assert!(matches!(err, InstallError::Unknown(_)), "{what}: {err:?}");
        // No plugin folder, no staging folder, no plugins folder even.
        assert_eq!(tree(&w.data), BTreeMap::new(), "{what}");
        assert!(w.host.list().is_empty(), "{what}");
        // The intact examples are still offered and still install.
        w.install(COUNTER).unwrap();
        w.assert_no_staging_left();
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_an_example_is_refused() {
    use std::os::unix::fs::symlink;

    // The module is a link to a real module elsewhere.
    let w = world();
    let outside = w.data.with_file_name("outside");
    fs::create_dir_all(&outside).unwrap();
    let module = w.example("hello").join("hello.wasm");
    fs::rename(&module, outside.join("hello.wasm")).unwrap();
    symlink(outside.join("hello.wasm"), &module).unwrap();
    assert!(!w.offered().contains(&HELLO.to_string()));
    let err = w.install(HELLO).unwrap_err();
    assert!(matches!(err, InstallError::Unknown(_)), "{err:?}");
    assert_eq!(tree(&w.data), BTreeMap::new());

    // The whole example folder is a link to a real example elsewhere.
    let w = world_with(&["counter"], None);
    let real = w.data.with_file_name("real-hello");
    copy_dir(&repo_examples().join("hello"), &real);
    symlink(&real, w.example("hello")).unwrap();
    assert!(!w.offered().contains(&HELLO.to_string()));
    let err = w.install(HELLO).unwrap_err();
    assert!(matches!(err, InstallError::Unknown(_)), "{err:?}");
    assert_eq!(tree(&w.data), BTreeMap::new());
}

#[test]
fn installing_grants_nothing() {
    let w = world();
    w.install(HA).unwrap();
    w.install(HELLO).unwrap();

    // Home Assistant asks for the address in its settings; nothing is set,
    // nothing is allowed, and its token is not set.
    let ha = w.host.get(HA).unwrap();
    let network = ha.network.expect("Home Assistant asks for the network");
    assert_eq!(network.status, GrantStatus::NeedsApproval);
    assert!(network
        .hosts
        .iter()
        .all(|h| h.status != GrantStatus::Granted));
    assert!(ha.secrets.iter().all(|s| !s.is_set));
    assert!(w.host.get(HELLO).unwrap().network.is_none());
    // Nothing was decided on the person's behalf: permissions.json, if it was
    // written at all, holds no decision.
    if let Ok(text) = fs::read_to_string(w.data.join("permissions.json")) {
        let saved: Value = serde_json::from_str(&text).unwrap();
        for (id, grants) in saved["plugins"].as_object().into_iter().flatten() {
            let decided = grants["network"].as_object().map_or(0, |n| n.len());
            assert_eq!(decided, 0, "{id}: {grants}");
        }
    }
}

#[test]
fn examples_say_which_are_installed_from_any_folder() {
    // A development folder (as LAUNCHDECK_PLUGINS names) already holds Counter.
    let dev = tempfile::tempdir().unwrap();
    copy_dir(
        &repo_examples().join("counter"),
        &dev.path().join("counter"),
    );
    let dev_before = tree(dev.path());
    let w = world_with(&SHIPPED, Some(dev.path()));

    let offered = w.host.examples(&w.examples);
    let by_id = |id: &str| offered.iter().find(|e| e.id == id).unwrap().clone();
    assert_eq!(offered.len(), 3, "{offered:?}");
    assert!(by_id(COUNTER).installed);
    assert!(!by_id(HELLO).installed);
    assert!(!by_id(HA).installed);
    assert!(by_id(HA).network);
    assert!(!by_id(HELLO).network && !by_id(COUNTER).network);

    // Counter is known already: refused, and neither folder is written.
    let err = w.install(COUNTER).unwrap_err();
    assert!(matches!(err, InstallError::AlreadyInstalled(_)), "{err:?}");
    assert!(!w.plugins().join(COUNTER).exists());

    // Hello goes into the app data folder, never into the development one.
    w.install(HELLO).unwrap();
    assert!(w.plugins().join(HELLO).join("plugin.json").is_file());
    assert_eq!(tree(dev.path()), dev_before);
    assert!(w.listed(HELLO).installed);
    assert!(!w.listed(HA).installed);
}

#[test]
fn an_unknown_id_or_an_occupied_folder_is_refused() {
    let w = world();
    let err = w.install("example.nope").unwrap_err();
    assert!(matches!(err, InstallError::Unknown(_)), "{err:?}");

    // A folder by that name that is not a plugin is the person's: left alone.
    let stray = w.plugins().join(HELLO);
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("notes.txt"), "mine").unwrap();
    let before = tree(&w.data);
    let err = w.install(HELLO).unwrap_err();
    assert!(matches!(err, InstallError::Exists(_)), "{err:?}");
    assert_eq!(tree(&w.data), before);
    w.assert_no_staging_left();
}

#[test]
fn installing_turns_on_a_plugin_that_was_turned_off_before() {
    // The person had turned Hello off, then removed its folder.
    let saved = json!({"schemaVersion": 1, "plugins": {HELLO: {"enabled": false}}});
    let w = world_saved(&SHIPPED, None, Some(saved));

    w.install(HELLO).unwrap();
    let info = w.host.get(HELLO).unwrap();
    assert!(info.enabled);
    assert_eq!(
        info.plugin.external.as_ref().unwrap().status,
        PluginStatus::Enabled
    );
}

#[test]
fn an_example_is_described_by_its_manifest_on_the_wire() {
    let w = world();
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(w.example("home-assistant").join("plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(w.listed(HA)).unwrap(),
        json!({
            "id": HA,
            "name": manifest["name"],
            "version": manifest["version"],
            "description": manifest["description"],
            "installed": false,
            "network": true,
        })
    );
}
