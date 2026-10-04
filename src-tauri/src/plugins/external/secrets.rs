//! Secrets: credentials a plugin needs and never sees.
//!
//! A plugin's manifest names a secret (`token`). The person types its value in
//! the Plugins screen; it goes straight into a [`SecretStore`] and is read
//! again only by the network broker, at the moment it adds it to a request
//! for the one origin the secret is bound to. It is never handed to the
//! module, never written to the workspace, a manifest, `plugins.json`, an
//! event, a snapshot or a log.
//!
//! There is one place a secret may be kept: the operating system's credential
//! store ([`KeychainStore`]: Keychain on macOS, Credential Manager on Windows,
//! the Secret Service on Linux). When the system has none that answers, the
//! store is [`UnavailableStore`]: nothing can be saved, nothing is read, and
//! the interface says why. **There is no plain-file fallback.**
//!
//! An earlier version did fall back to `secrets.json`. That file is never
//! read to authenticate anything now. [`LegacyFile`] only lists what is in it
//! so the person can move it into the credential store or delete it.
//!
//! [`MemoryStore`] is for tests and for a runtime without a data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Longest secret value kept.
pub const MAX_SECRET_BYTES: usize = 4096;
/// The plain file an earlier version wrote when the system had no credential store.
pub const LEGACY_FILE: &str = "secrets.json";
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
    /// This system has no secure credential store LaunchDeck can use.
    #[error("no secure credential store is available")]
    Unavailable,
}

/// Where secrets are kept, as the interface says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreKind {
    /// The operating system's credential store.
    Keychain,
    /// Nowhere durable (tests, a runtime without a data folder).
    Memory,
    /// Nowhere: the system has no secure store, so secrets cannot be kept.
    Unavailable,
}

/// What the interface says about the store: which one, whether it can be
/// used, and when it cannot, why and what would change that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub kind: StoreKind,
    pub available: bool,
    /// What this system calls its credential store.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub trait SecretStore: Send + Sync {
    /// The value, or `None` when nothing is stored.
    fn get(&self, secret: &SecretRef) -> Result<Option<String>, SecretError>;
    fn set(&self, secret: &SecretRef, value: &str) -> Result<(), SecretError>;
    /// Removing what is not there is not an error.
    fn delete(&self, secret: &SecretRef) -> Result<(), SecretError>;
    fn kind(&self) -> StoreKind;
    fn status(&self) -> StoreStatus {
        StoreStatus {
            kind: self.kind(),
            available: self.kind() != StoreKind::Unavailable,
            name: match self.kind() {
                StoreKind::Memory => "memory".to_string(),
                _ => system_store_name().to_string(),
            },
            reason: None,
        }
    }
}

/// What this operating system calls its credential store.
pub fn system_store_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Keychain"
    } else if cfg!(target_os = "windows") {
        "Credential Manager"
    } else {
        "Secret Service keyring"
    }
}

/// Why the system's store cannot be used, and what would change that.
fn unavailable_reason() -> &'static str {
    if cfg!(target_os = "macos") {
        "The login keychain could not be reached. Unlock it in Keychain Access, then restart LaunchDeck."
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager could not be reached. Check that the Credential Manager service is running, then restart LaunchDeck."
    } else {
        "No Secret Service answered in this session. LaunchDeck keeps secrets in the system keyring (GNOME Keyring, KWallet or another Secret Service provider). Install and unlock one, then restart LaunchDeck."
    }
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
// No store
// ---------------------------------------------------------------------------

/// The store of a system that has no secure credential store. It holds
/// nothing and says so: a secret cannot be saved, and a request that needs one
/// fails with a reason instead of going out without it.
pub struct UnavailableStore {
    reason: String,
}

impl UnavailableStore {
    pub fn new(reason: &str) -> UnavailableStore {
        UnavailableStore {
            reason: reason.to_string(),
        }
    }
}

impl SecretStore for UnavailableStore {
    fn get(&self, _: &SecretRef) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable)
    }

    fn set(&self, _: &SecretRef, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }

    fn delete(&self, _: &SecretRef) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }

    fn kind(&self) -> StoreKind {
        StoreKind::Unavailable
    }

    fn status(&self) -> StoreStatus {
        StoreStatus {
            kind: StoreKind::Unavailable,
            available: false,
            name: system_store_name().to_string(),
            reason: Some(self.reason.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// The plain file of an earlier version
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

/// What `secrets.json` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyContents {
    /// There is no such file.
    Absent,
    /// There is one and this version cannot read it. It is left alone.
    Unreadable,
    /// Its secrets, in a fixed order (plugin, then key).
    Secrets(Vec<(SecretRef, String)>),
}

/// `secrets.json`, written by an earlier version when the system had no
/// credential store. It is **not a store**: nothing here reads a value from it
/// to use it. It can be listed, moved into a real store, and removed.
pub struct LegacyFile {
    path: PathBuf,
}

impl LegacyFile {
    pub fn in_dir(dir: &Path) -> LegacyFile {
        LegacyFile {
            path: dir.join(LEGACY_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> LegacyContents {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LegacyContents::Absent,
            Err(_) => return LegacyContents::Unreadable,
        };
        match serde_json::from_str::<SecretFile>(&text) {
            Ok(file) if file.schema_version <= FILE_SCHEMA => LegacyContents::Secrets(
                file.secrets
                    .into_iter()
                    .flat_map(|(plugin, keys)| {
                        keys.into_iter()
                            .filter(|(_, value)| !value.is_empty())
                            .map(move |(key, value)| (SecretRef::new(&plugin, &key), value))
                    })
                    .collect(),
            ),
            _ => LegacyContents::Unreadable,
        }
    }

    /// Remove the file. Its bytes are overwritten first, which is the most a
    /// program can do and **not a guarantee**: a journaling or copy-on-write
    /// filesystem, an SSD and any backup may still hold an older copy.
    pub fn remove(&self) -> std::io::Result<()> {
        use std::io::Write as _;
        let len = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len() as usize,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&self.path) {
            let _ = file.write_all(&vec![0u8; len]);
            let _ = file.sync_all();
        }
        std::fs::remove_file(&self.path)
    }
}

/// What moving plain values into a store did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Moved {
    /// Written to the store and read back equal.
    pub moved: Vec<SecretRef>,
    /// The store already held a value for it. That value was kept: it is the
    /// one entered after the plain file stopped being used.
    pub kept: Vec<SecretRef>,
    /// Not moved, and why. Nothing was lost: the plain value is where it was.
    pub failed: Vec<(SecretRef, String)>,
}

/// Move plain values into `store`, one at a time, each read back before it
/// counts as moved. Nothing is removed here; the caller removes a plain value
/// only when it is not in `failed`.
pub fn move_into(store: &dyn SecretStore, entries: &[(SecretRef, String)]) -> Moved {
    let mut out = Moved::default();
    for (secret, value) in entries {
        match store.get(secret) {
            Err(e) => out.failed.push((secret.clone(), e.to_string())),
            Ok(Some(_)) => out.kept.push(secret.clone()),
            Ok(None) => match store.set(secret, value) {
                Err(e) => out.failed.push((secret.clone(), e.to_string())),
                Ok(()) => match store.get(secret) {
                    Ok(Some(back)) if &back == value => out.moved.push(secret.clone()),
                    Ok(_) => out.failed.push((
                        secret.clone(),
                        "the store did not return what was written".into(),
                    )),
                    Err(e) => out.failed.push((secret.clone(), e.to_string())),
                },
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The operating system's credential store
// ---------------------------------------------------------------------------

/// The name every LaunchDeck plugin secret is filed under, followed by the plugin id.
const SERVICE_PREFIX: &str = "LaunchDeck plugin ";

pub struct KeychainStore;

impl KeychainStore {
    /// The store, if this system has one that answers. A system without one
    /// (a Linux session with no Secret Service, for example) gives `None`.
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
/// answers, and otherwise none at all (never a file). Without a data folder
/// there is nothing durable to protect and values live in memory.
pub fn open_store(dir: Option<&Path>) -> Arc<dyn SecretStore> {
    if dir.is_none() {
        return Arc::new(MemoryStore::new());
    }
    match KeychainStore::open() {
        Some(keychain) => Arc::new(keychain),
        None => {
            log::warn!(
                "no secure credential store answered; plugin secrets cannot be saved or used"
            );
            Arc::new(UnavailableStore::new(unavailable_reason()))
        }
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

    fn write_legacy(dir: &Path, json: &str) -> LegacyFile {
        std::fs::write(dir.join(LEGACY_FILE), json).unwrap();
        LegacyFile::in_dir(dir)
    }

    const TWO: &str = r#"{"schemaVersion":1,"secrets":{"acme.lights":{"token":"plain-one"},"acme.music":{"key":"plain-two","empty":""}}}"#;

    #[test]
    fn a_system_without_a_store_keeps_nothing_and_says_why() {
        let store = UnavailableStore::new("No Secret Service answered.");
        assert_eq!(store.get(&token()), Err(SecretError::Unavailable));
        assert_eq!(store.set(&token(), "value"), Err(SecretError::Unavailable));
        assert_eq!(store.delete(&token()), Err(SecretError::Unavailable));
        let status = store.status();
        assert_eq!(
            (status.kind, status.available),
            (StoreKind::Unavailable, false)
        );
        assert_eq!(
            status.reason.as_deref(),
            Some("No Secret Service answered.")
        );
        // The other stores say they can be used and give no reason.
        let memory = MemoryStore::new().status();
        assert!(memory.available && memory.reason.is_none());
    }

    #[test]
    fn the_old_plain_file_is_listed_in_a_fixed_order_and_never_written() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            LegacyFile::in_dir(dir.path()).read(),
            LegacyContents::Absent
        );
        let legacy = write_legacy(dir.path(), TWO);
        let LegacyContents::Secrets(found) = legacy.read() else {
            panic!("the file is readable");
        };
        assert_eq!(
            found,
            vec![
                (SecretRef::new("acme.lights", "token"), "plain-one".into()),
                (SecretRef::new("acme.music", "key"), "plain-two".into()),
            ],
            "an empty value is not a secret"
        );
        assert_eq!(
            std::fs::read_to_string(legacy.path()).unwrap(),
            TWO,
            "reading changes nothing"
        );
    }

    #[test]
    fn a_plain_file_this_version_cannot_read_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        for text in [r#"{"schemaVersion": 99, "secrets": {}}"#, "not json"] {
            let legacy = write_legacy(dir.path(), text);
            assert_eq!(legacy.read(), LegacyContents::Unreadable);
            assert_eq!(std::fs::read_to_string(legacy.path()).unwrap(), text);
        }
    }

    #[test]
    fn moving_plain_values_verifies_each_one_and_keeps_what_the_store_already_has() {
        let dir = tempfile::tempdir().unwrap();
        let LegacyContents::Secrets(entries) = write_legacy(dir.path(), TWO).read() else {
            panic!()
        };
        let store = MemoryStore::new();
        // A value entered after the file stopped being used wins over the file's.
        store
            .set(&SecretRef::new("acme.music", "key"), "newer")
            .unwrap();

        let moved = move_into(&store, &entries);
        assert_eq!(moved.moved, vec![SecretRef::new("acme.lights", "token")]);
        assert_eq!(moved.kept, vec![SecretRef::new("acme.music", "key")]);
        assert!(moved.failed.is_empty());
        assert_eq!(store.get(&token()).unwrap().as_deref(), Some("plain-one"));
        assert_eq!(
            store
                .get(&SecretRef::new("acme.music", "key"))
                .unwrap()
                .as_deref(),
            Some("newer")
        );
        // Doing it again changes nothing.
        let again = move_into(&store, &entries);
        assert!(again.moved.is_empty() && again.failed.is_empty());
        assert_eq!(again.kept.len(), 2);
    }

    #[test]
    fn nothing_moves_into_a_store_that_is_not_there_or_refuses() {
        let entries = vec![
            (token(), "plain-one".to_string()),
            (
                SecretRef::new("acme.music", "key"),
                "x".repeat(MAX_SECRET_BYTES + 1),
            ),
        ];
        let none = move_into(&UnavailableStore::new("none"), &entries);
        assert!(none.moved.is_empty() && none.kept.is_empty());
        assert_eq!(none.failed.len(), 2);
        assert!(
            none.failed
                .iter()
                .all(|(_, why)| !why.contains("plain-one")),
            "a reason never repeats a value"
        );

        // One value the store refuses does not stop the others.
        let store = MemoryStore::new();
        let some = move_into(&store, &entries);
        assert_eq!(some.moved, vec![token()]);
        assert_eq!(some.failed.len(), 1);
        assert_eq!(some.failed[0].0, SecretRef::new("acme.music", "key"));
    }

    #[test]
    fn removing_the_plain_file_overwrites_it_first_and_is_fine_when_it_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = write_legacy(dir.path(), TWO);
        // A second name for the same file sees what is done to its bytes.
        let link = dir.path().join("same-file");
        let linked = std::fs::hard_link(legacy.path(), &link).is_ok();
        legacy.remove().unwrap();
        assert!(!legacy.path().exists());
        if linked {
            let left = std::fs::read(&link).unwrap();
            assert_eq!(left.len(), TWO.len());
            assert!(
                left.iter().all(|b| *b == 0),
                "the bytes were overwritten before the name was removed"
            );
        }
        legacy.remove().unwrap();
        assert_eq!(legacy.read(), LegacyContents::Absent);
    }

    #[test]
    fn errors_say_nothing_about_the_value() {
        for e in [
            SecretError::Empty,
            SecretError::TooLong,
            SecretError::Store("it is locked".into()),
            SecretError::Unavailable,
        ] {
            assert!(!e.to_string().contains("s3cret"));
        }
    }

    #[test]
    fn without_a_folder_secrets_live_in_memory() {
        assert_eq!(open_store(None).kind(), StoreKind::Memory);
    }
}
