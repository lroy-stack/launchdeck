//! Secrets: credentials a plugin needs and never sees.
//!
//! A plugin's manifest names a secret (`token`). The person types its value in
//! the Plugins screen; it goes straight into a [`SecretStore`] and is read
//! again only by the network broker, at the moment it adds it to a request
//! for the one origin the secret is bound to. It is never handed to the
//! module, never written to the workspace, a manifest, `plugins.json`, an
//! event, a snapshot or a log.
//!
//! Three stores sit behind one small interface:
//!
//! * [`KeychainStore`]: the operating system's credential store (Keychain on
//!   macOS, Credential Manager on Windows, the Secret Service on Linux);
//! * [`FileStore`]: `secrets.json`, readable by the owner only, for a system
//!   without a credential store. The interface says which one is in use;
//! * [`MemoryStore`]: for tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::persistence::write_private;

/// Longest secret value kept.
pub const MAX_SECRET_BYTES: usize = 4096;
/// Forces a store: `file` or `keychain`. For tests and headless machines.
pub const STORE_ENV: &str = "LAUNCHDECK_SECRET_STORE";
const FILE_SCHEMA: u32 = 1;

/// A secret by name: whose it is and what the manifest calls it. This is all
/// a plugin, the interface or a log ever holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef {
    pub plugin: String,
    pub key: String,
}

impl SecretRef {
    pub fn new(plugin: &str, key: &str) -> SecretRef {
        SecretRef {
            plugin: plugin.to_string(),
            key: key.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("the value is empty")]
    Empty,
    #[error("the value is longer than {MAX_SECRET_BYTES} bytes")]
    TooLong,
    #[error("the credential store refused: {0}")]
    Store(String),
}

/// Where secrets are kept, as the interface says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreKind {
    /// The operating system's credential store.
    Keychain,
    /// A private file, because the system has no credential store.
    File,
    /// Nowhere durable (tests).
    Memory,
}

pub trait SecretStore: Send + Sync {
    /// The value, or `None` when nothing is stored.
    fn get(&self, secret: &SecretRef) -> Result<Option<String>, SecretError>;
    fn set(&self, secret: &SecretRef, value: &str) -> Result<(), SecretError>;
    /// Removing what is not there is not an error.
    fn delete(&self, secret: &SecretRef) -> Result<(), SecretError>;
    fn kind(&self) -> StoreKind;
}

fn check_value(value: &str) -> Result<(), SecretError> {
    if value.is_empty() {
        Err(SecretError::Empty)
    } else if value.len() > MAX_SECRET_BYTES {
        Err(SecretError::TooLong)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<BTreeMap<(String, String), String>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, String), String>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, s: &SecretRef) -> Result<Option<String>, SecretError> {
        Ok(self.lock().get(&(s.plugin.clone(), s.key.clone())).cloned())
    }

    fn set(&self, s: &SecretRef, value: &str) -> Result<(), SecretError> {
        check_value(value)?;
        self.lock()
            .insert((s.plugin.clone(), s.key.clone()), value.to_string());
        Ok(())
    }

    fn delete(&self, s: &SecretRef) -> Result<(), SecretError> {
        self.lock().remove(&(s.plugin.clone(), s.key.clone()));
        Ok(())
    }

    fn kind(&self) -> StoreKind {
        StoreKind::Memory
    }
}

// ---------------------------------------------------------------------------
// A private file
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretFile {
    #[serde(default)]
    schema_version: u32,
    /// Plugin id, then secret key, to the value.
    #[serde(default)]
    secrets: BTreeMap<String, BTreeMap<String, String>>,
}

/// `secrets.json` in the app data folder: owner-only, written atomically. The
/// values are not encrypted; this is the fallback for a system that offers
/// nothing better, and the interface says so.
pub struct FileStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileStore {
    pub fn new(path: PathBuf) -> FileStore {
        FileStore {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<SecretFile, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str::<SecretFile>(&text)
                .ok()
                .filter(|f| f.schema_version <= FILE_SCHEMA)
                // A file this version cannot read is left alone and nothing is found in it.
                .ok_or_else(|| SecretError::Store("secrets.json could not be read".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SecretFile::default()),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }

    fn write(&self, mut file: SecretFile) -> Result<(), SecretError> {
        file.schema_version = FILE_SCHEMA;
        file.secrets.retain(|_, keys| !keys.is_empty());
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|e| SecretError::Store(e.to_string()))?;
        write_private(&self.path, &bytes).map_err(|e| SecretError::Store(e.to_string()))
    }
}

impl SecretStore for FileStore {
    fn get(&self, s: &SecretRef) -> Result<Option<String>, SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self
            .read()?
            .secrets
            .get(&s.plugin)
            .and_then(|k| k.get(&s.key))
            .cloned())
    }

    fn set(&self, s: &SecretRef, value: &str) -> Result<(), SecretError> {
        check_value(value)?;
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = self.read()?;
        file.secrets
            .entry(s.plugin.clone())
            .or_default()
            .insert(s.key.clone(), value.to_string());
        self.write(file)
    }

    fn delete(&self, s: &SecretRef) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = self.read()?;
        let removed = file
            .secrets
            .get_mut(&s.plugin)
            .and_then(|k| k.remove(&s.key));
        if removed.is_none() {
            return Ok(());
        }
        self.write(file)
    }

    fn kind(&self) -> StoreKind {
        StoreKind::File
    }
}

// ---------------------------------------------------------------------------
// The operating system's credential store
// ---------------------------------------------------------------------------

/// The name every LaunchDeck plugin secret is filed under, followed by the plugin id.
const SERVICE_PREFIX: &str = "LaunchDeck plugin ";

pub struct KeychainStore;

impl KeychainStore {
    /// The store, if this system has one that answers. A system without one
    /// (a headless Linux session with no Secret Service, for example) gives `None`.
    pub fn open() -> Option<KeychainStore> {
        if keyring::Entry::store_status().is_err() {
            return None;
        }
        // Asking for something that is not there proves the store can be reached.
        let probe = keyring::Entry::new(&format!("{SERVICE_PREFIX}probe"), "probe").ok()?;
        match probe.get_password() {
            Ok(_) | Err(keyring::Error::NoEntry) => Some(KeychainStore),
            Err(_) => None,
        }
    }

    fn entry(s: &SecretRef) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&format!("{SERVICE_PREFIX}{}", s.plugin), &s.key).map_err(store_error)
    }
}

/// What a credential store said, without anything it may have echoed back.
fn store_error(e: keyring::Error) -> SecretError {
    SecretError::Store(match e {
        keyring::Error::NoStorageAccess(_) => "it is locked or cannot be reached".into(),
        keyring::Error::PlatformFailure(_) => "it reported a failure".into(),
        keyring::Error::TooLong(_, _) => "the value is too long for it".into(),
        keyring::Error::NoDefaultStore => "this system has none".into(),
        _ => "it could not complete the request".into(),
    })
}

impl SecretStore for KeychainStore {
    fn get(&self, s: &SecretRef) -> Result<Option<String>, SecretError> {
        match Self::entry(s)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(e)),
        }
    }

    fn set(&self, s: &SecretRef, value: &str) -> Result<(), SecretError> {
        check_value(value)?;
        Self::entry(s)?.set_password(value).map_err(store_error)
    }

    fn delete(&self, s: &SecretRef) -> Result<(), SecretError> {
        match Self::entry(s)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(e)),
        }
    }

    fn kind(&self) -> StoreKind {
        StoreKind::Keychain
    }
}

/// The store this installation uses: the system's credential store when it
/// has one, a private file in `dir` otherwise, memory when there is no folder.
pub fn open_store(dir: Option<&Path>) -> Arc<dyn SecretStore> {
    let forced = std::env::var(STORE_ENV).ok();
    let Some(dir) = dir else {
        return Arc::new(MemoryStore::new());
    };
    let file = || -> Arc<dyn SecretStore> { Arc::new(FileStore::new(dir.join("secrets.json"))) };
    match forced.as_deref() {
        Some("file") => file(),
        _ => match KeychainStore::open() {
            Some(keychain) => Arc::new(keychain),
            None => {
                log::info!("no system credential store answered; plugin secrets are kept in a private file");
                file()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> SecretRef {
        SecretRef::new("acme.lights", "token")
    }

    /// The contract every store keeps.
    fn behaves_like_a_store(store: &dyn SecretStore) {
        let s = token();
        assert_eq!(
            store.get(&s).unwrap(),
            None,
            "nothing until something is set"
        );
        store.delete(&s).unwrap();
        store.set(&s, "first").unwrap();
        assert_eq!(store.get(&s).unwrap().as_deref(), Some("first"));
        store.set(&s, "second").unwrap();
        assert_eq!(
            store.get(&s).unwrap().as_deref(),
            Some("second"),
            "a new value replaces the old"
        );
        // A secret belongs to one plugin and one key.
        assert_eq!(
            store
                .get(&SecretRef::new("another.plugin", "token"))
                .unwrap(),
            None
        );
        assert_eq!(
            store.get(&SecretRef::new("acme.lights", "other")).unwrap(),
            None
        );
        assert_eq!(store.set(&s, ""), Err(SecretError::Empty));
        assert_eq!(
            store.set(&s, &"x".repeat(MAX_SECRET_BYTES + 1)),
            Err(SecretError::TooLong)
        );
        assert_eq!(
            store.get(&s).unwrap().as_deref(),
            Some("second"),
            "a refused value changes nothing"
        );
        store.delete(&s).unwrap();
        assert_eq!(store.get(&s).unwrap(), None);
    }

    #[test]
    fn the_memory_store_keeps_the_contract() {
        behaves_like_a_store(&MemoryStore::new());
    }

    #[test]
    fn the_file_store_keeps_the_contract_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = FileStore::new(path.clone());
        behaves_like_a_store(&store);

        store.set(&token(), "s3cret-value").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o077,
                0,
                "secrets.json is readable by its owner only"
            );
        }
        // A new process finds it.
        assert_eq!(
            FileStore::new(path.clone())
                .get(&token())
                .unwrap()
                .as_deref(),
            Some("s3cret-value")
        );
        // Removing the last secret leaves no trace of the plugin.
        store.delete(&token()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("s3cret-value") && !text.contains("acme.lights"),
            "{text}"
        );
    }

    #[test]
    fn a_file_this_version_cannot_read_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, r#"{"schemaVersion": 99, "secrets": {}}"#).unwrap();
        let store = FileStore::new(path.clone());
        assert!(matches!(store.get(&token()), Err(SecretError::Store(_))));
        assert!(matches!(
            store.set(&token(), "x"),
            Err(SecretError::Store(_))
        ));
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("99"),
            "nothing was overwritten"
        );
    }

    #[test]
    fn errors_say_nothing_about_the_value() {
        for e in [
            SecretError::Empty,
            SecretError::TooLong,
            SecretError::Store("it is locked".into()),
        ] {
            assert!(!e.to_string().contains("s3cret"));
        }
    }

    #[test]
    fn without_a_folder_secrets_live_in_memory() {
        assert_eq!(open_store(None).kind(), StoreKind::Memory);
    }
}
