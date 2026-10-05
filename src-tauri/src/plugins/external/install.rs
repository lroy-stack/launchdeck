//! Installing an example plugin the app ships.
//!
//! The Hello, Counter and Home Assistant examples come with the app, and the
//! Plugins screen copies one into the plugins folder when the person asks.
//! Only a folder of the examples root is ever installed, never a file or a
//! folder someone names. The example is validated as a scan validates a
//! plugin, and only its manifest and the module the manifest names are copied,
//! into a hidden staging folder next to the destination, where the copy is
//! validated again before it is renamed into place: a scan never sees half a
//! plugin, and a failed install leaves nothing behind.
//!
//! Installing turns the plugin on, as dropping its folder in does, and grants
//! nothing: a plugin that asks for the network reaches nothing until the
//! person allows an address.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use super::discovery::{self, Candidate};
use super::host::PluginHost;
use super::manifest::{MANIFEST_FILE, MAX_MANIFEST_BYTES};
use crate::persistence::write_atomic;

/// One install at a time, so two clicks cannot race for one destination.
static INSTALLING: Mutex<()> = Mutex::new(());

/// An example plugin the app ships, as the Plugins screen offers it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExampleInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// A plugin with this id is known to the host already, from any plugin folder.
    pub installed: bool,
    /// The manifest asks for network access, which stays off until the person
    /// allows an address.
    pub network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    /// No usable example has this id (a damaged one is not offered).
    #[error("there is no example plugin `{0}`")]
    Unknown(String),
    /// A plugin with this id is known already. Holds the plugin's name.
    #[error("{0} is already installed")]
    AlreadyInstalled(String),
    /// Something is in the way at the destination. Holds the path.
    #[error("{0} already exists")]
    Exists(String),
    #[error("there is no plugins folder to install into")]
    NoFolder,
    #[error("could not install the plugin: {0}")]
    Copy(String),
    /// The plugin is installed, and turning it on failed.
    #[error("the plugin was installed but could not be turned on: {0}")]
    Enable(String),
}

impl PluginHost {
    /// The folder plugins are installed into: the first plugin root, which is
    /// the `plugins` folder in the app data directory. Folders named in
    /// `LAUNCHDECK_PLUGINS` come after it and are never written.
    pub fn install_folder(&self) -> Option<&Path> {
        self.roots().first().map(PathBuf::as_path)
    }

    /// The usable examples under `examples_root`, one per id, by folder name.
    pub fn examples(&self, examples_root: &Path) -> Vec<ExampleInfo> {
        let known: HashSet<String> = self.data().plugins.keys().cloned().collect();
        let mut seen = HashSet::new();
        usable_examples(examples_root)
            .filter(|c| seen.insert(c.manifest.id.clone()))
            .map(|c| ExampleInfo {
                installed: known.contains(&c.manifest.id),
                network: c.manifest.capabilities.network.is_some(),
                id: c.manifest.id,
                name: c.manifest.name,
                version: c.manifest.version,
                description: c.manifest.description,
            })
            .collect()
    }

    /// Install the example `id` from `examples_root` into the
    /// [install folder](PluginHost::install_folder), then scan, so it runs. It
    /// is refused when a plugin with that id is known already, when the
    /// destination exists, or when the example is not usable.
    pub fn install_example(&self, examples_root: &Path, id: &str) -> Result<(), InstallError> {
        let _one_at_a_time = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
        let example = usable_examples(examples_root)
            .find(|c| c.manifest.id == id)
            .ok_or_else(|| InstallError::Unknown(id.to_string()))?;
        if self.data().plugins.contains_key(id) {
            return Err(InstallError::AlreadyInstalled(example.manifest.name));
        }
        let root = self.install_folder().ok_or(InstallError::NoFolder)?;
        let dest = root.join(id);
        if fs::symlink_metadata(&dest).is_ok() {
            return Err(InstallError::Exists(dest.display().to_string()));
        }
        fs::create_dir_all(root)
            .map_err(|e| InstallError::Copy(format!("cannot create {}: {e}", root.display())))?;
        stage(&example, root)?.place(&dest)?;
        self.scan();
        // The click means "on", also for a plugin the person had turned off
        // before its folder was removed.
        if self.get(id).is_ok_and(|info| !info.enabled) {
            self.set_enabled(id, true)
                .map_err(|e| InstallError::Enable(e.to_string()))?;
        }
        Ok(())
    }
}

/// The example folders under `root` that load as a plugin, by folder name.
/// A symbolic link is not followed, whether it is the folder or a file in it.
fn usable_examples(root: &Path) -> impl Iterator<Item = Candidate> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| discovery::load_dir(&dir).ok())
}

/// A folder next to the destination holding the copy until it is complete.
/// Removed when dropped, unless it was renamed into place.
struct Staging(Option<PathBuf>);

impl Staging {
    fn create(root: &Path, id: &str) -> Result<Staging, InstallError> {
        let mut random = [0u8; 6];
        getrandom::fill(&mut random)
            .map_err(|e| InstallError::Copy(format!("no random name for the copy: {e}")))?;
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let path = root.join(format!(".{id}.tmp-{suffix}"));
        // `create_dir`, not `create_dir_all`: a folder already there is not ours.
        fs::create_dir(&path)
            .map_err(|e| InstallError::Copy(format!("cannot create {}: {e}", path.display())))?;
        Ok(Staging(Some(path)))
    }

    fn path(&self) -> &Path {
        self.0
            .as_deref()
            .expect("a staging folder until it is placed")
    }

    /// Rename the copy to `dest`. On failure the copy is removed.
    fn place(mut self, dest: &Path) -> Result<(), InstallError> {
        if fs::symlink_metadata(dest).is_ok() {
            return Err(InstallError::Exists(dest.display().to_string()));
        }
        let from = self.path().to_path_buf();
        fs::rename(&from, dest).map_err(|e| {
            InstallError::Copy(format!("cannot move it to {}: {e}", dest.display()))
        })?;
        self.0 = None;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            if let Err(e) = fs::remove_dir_all(&path) {
                log::warn!("could not remove {}: {e}", path.display());
            }
        }
    }
}

/// Copy the manifest and the module of a validated example into a staging
/// folder in `root`, and validate the copy: it must be the very bytes that
/// were validated, or the example changed while it was being copied. The
/// manifest is read again as a scan reads it (a regular file, not a symbolic
/// link, within the size limit).
fn stage(example: &Candidate, root: &Path) -> Result<Staging, InstallError> {
    let manifest = discovery::read_plain_file(
        &example.dir.join(MANIFEST_FILE),
        MAX_MANIFEST_BYTES,
        MANIFEST_FILE,
    )
    .map_err(InstallError::Copy)?;
    let staging = Staging::create(root, &example.manifest.id)?;
    let write = |name: &str, bytes: &[u8]| {
        write_atomic(&staging.path().join(name), bytes)
            .map_err(|e| InstallError::Copy(format!("cannot write {name}: {e}")))
    };
    write(&example.manifest.main, &example.wasm)?;
    // The manifest last: to a scan, a folder without one is not a plugin.
    write(MANIFEST_FILE, &manifest)?;
    let copy = discovery::load_dir(staging.path())
        .map_err(|e| InstallError::Copy(format!("the copy is not usable: {e}")))?;
    if copy.manifest.id != example.manifest.id || copy.fingerprint != example.fingerprint {
        return Err(InstallError::Copy(
            "the example changed while it was being copied".into(),
        ));
    }
    Ok(staging)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_that_cannot_be_placed_is_removed_and_the_destination_kept() {
        let root = tempfile::tempdir().unwrap();
        let staging = Staging::create(root.path(), "a.b").unwrap();
        let staged = staging.path().to_path_buf();
        fs::write(staged.join(MANIFEST_FILE), "{}").unwrap();
        assert!(staged
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".a.b.tmp-"));
        // Something appeared at the destination meanwhile.
        let dest = root.path().join("a.b");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("keep.txt"), "mine").unwrap();

        let err = staging.place(&dest).unwrap_err();
        assert!(matches!(err, InstallError::Exists(_)), "{err:?}");
        assert!(!staged.exists());
        assert_eq!(fs::read_to_string(dest.join("keep.txt")).unwrap(), "mine");
    }

    #[test]
    fn a_copy_left_unplaced_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let staged = {
            let staging = Staging::create(root.path(), "a.b").unwrap();
            fs::write(staging.path().join("m.wasm"), b"\0asm").unwrap();
            staging.path().to_path_buf()
        };
        assert!(!staged.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /// A valid example folder, as `usable_examples` would hand it over.
    fn example(root: &Path) -> Candidate {
        let dir = root.join("example");
        fs::create_dir_all(&dir).unwrap();
        let manifest = serde_json::json!({"schemaVersion": 1, "id": "a.b", "name": "T",
            "version": "1.0.0", "author": "t", "abi": 1, "main": "m.wasm"});
        fs::write(dir.join(MANIFEST_FILE), manifest.to_string()).unwrap();
        fs::write(dir.join("m.wasm"), b"\0asm\x01\0\0\0").unwrap();
        discovery::load_dir(&dir).unwrap()
    }

    #[test]
    fn an_example_that_changed_after_it_was_validated_is_not_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let example = example(tmp.path());
        let root = tmp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();
        // Still a valid manifest, but not the one that was validated.
        let path = example.dir.join(MANIFEST_FILE);
        let changed = fs::read_to_string(&path).unwrap().replace("\"T\"", "\"U\"");
        fs::write(&path, changed).unwrap();

        let err = stage(&example, &root).err().expect("refused");
        assert!(err.to_string().contains("changed"), "{err}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_manifest_that_became_a_symbolic_link_is_not_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let example = example(tmp.path());
        let root = tmp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();
        // The very same bytes, now behind a link.
        let path = example.dir.join(MANIFEST_FILE);
        let elsewhere = tmp.path().join("elsewhere.json");
        fs::rename(&path, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();

        let err = stage(&example, &root).err().expect("refused");
        assert!(err.to_string().contains("symbolic link"), "{err}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn a_placed_copy_stays() {
        let root = tempfile::tempdir().unwrap();
        let staging = Staging::create(root.path(), "a.b").unwrap();
        fs::write(staging.path().join(MANIFEST_FILE), "{}").unwrap();
        let dest = root.path().join("a.b");
        staging.place(&dest).unwrap();
        assert!(dest.join(MANIFEST_FILE).is_file());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
