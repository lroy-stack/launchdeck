//! Secrets an earlier version kept in plain text: the `secrets.json` file a
//! system without a credential store fell back to, and settings of an older
//! version of a plugin that its current manifest declares as secrets.
//!
//! They are listed by name in the host's overview, never used, and moved into
//! the credential store or deleted only when the person asks.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::host::{Data, HostError, PluginHost};
use super::log::Level;
use super::secrets::{move_into, LegacyContents, LegacyFile, SecretRef};
use crate::audit::{Actor, AuditEntry};
use crate::plugins::Plugin;

/// Where a secret that is not in the credential store was found.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LegacySource {
    /// `secrets.json`, the plain file an earlier version fell back to.
    File,
    /// `plugins.json`: a setting of an older version of the plugin that its
    /// current manifest declares as a secret.
    Settings,
}

/// One secret kept in plain text by an earlier version: its name, never its value.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacySecretView {
    pub plugin: String,
    /// The plugin's name, when it is installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    pub key: String,
    pub source: LegacySource,
}

/// Secrets kept in plain text by an earlier version. They are not used.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacySecretsView {
    pub entries: Vec<LegacySecretView>,
    /// The plain file, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The file is there and this version cannot read it.
    pub unreadable: bool,
}

impl LegacySecretsView {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.file.is_none()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFailure {
    pub plugin: String,
    pub key: String,
    pub reason: String,
}

/// What moving the plain secrets into the credential store did.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigration {
    /// Written to the credential store and read back equal.
    pub moved: usize,
    /// The credential store already had a value, which was kept.
    pub kept: usize,
    /// Not moved. Each one is still where it was.
    pub failed: Vec<LegacyFailure>,
    /// The plain file was overwritten and removed.
    pub file_removed: bool,
}

impl LegacyMigration {
    fn absorb(&mut self, done: &super::secrets::Moved, settled: &mut Vec<SecretRef>) {
        self.moved += done.moved.len();
        self.kept += done.kept.len();
        settled.extend(done.moved.iter().chain(&done.kept).cloned());
        self.failed
            .extend(done.failed.iter().map(|(secret, reason)| LegacyFailure {
                plugin: secret.plugin.clone(),
                key: secret.key.clone(),
                reason: reason.clone(),
            }));
    }
}

impl PluginHost {
    fn legacy_file(&self) -> Option<LegacyFile> {
        self.inner
            .state_path
            .as_deref()
            .and_then(Path::parent)
            .map(LegacyFile::in_dir)
    }

    /// Settings left in `plugins.json` under a key that the plugin's current
    /// manifest declares as a secret: an older version of the plugin kept the
    /// value as a setting. In a fixed order (plugin, then key).
    fn leftover_settings(data: &Data) -> Vec<(SecretRef, String)> {
        let mut out = Vec::new();
        for (id, plugin) in &data.plugins {
            let Some(saved) = data.saved.plugins.get(id) else {
                continue;
            };
            let mut keys: Vec<&str> = plugin
                .spec()
                .secrets
                .iter()
                .map(|s| s.key.as_str())
                .collect();
            keys.sort_unstable();
            for key in keys {
                if let Some(Value::String(value)) = saved.config.get(key) {
                    if !value.is_empty() {
                        out.push((SecretRef::new(id, key), value.clone()));
                    }
                }
            }
        }
        out
    }

    fn legacy_view(&self, data: &Data) -> LegacySecretsView {
        let name = |plugin: &str| data.plugins.get(plugin).map(|p| p.manifest().name);
        let view = |s: &SecretRef, source| LegacySecretView {
            plugin: s.plugin.clone(),
            plugin_name: name(&s.plugin),
            key: s.key.clone(),
            source,
        };
        let mut out = LegacySecretsView::default();
        if let Some(file) = self.legacy_file() {
            match file.read() {
                LegacyContents::Absent => {}
                LegacyContents::Unreadable => {
                    out.file = Some(file.path().display().to_string());
                    out.unreadable = true;
                }
                LegacyContents::Secrets(found) => {
                    out.file = Some(file.path().display().to_string());
                    out.entries
                        .extend(found.iter().map(|(s, _)| view(s, LegacySource::File)));
                }
            }
        }
        out.entries.extend(
            Self::leftover_settings(data)
                .iter()
                .map(|(s, _)| view(s, LegacySource::Settings)),
        );
        out
    }

    /// Secrets an earlier version kept in plain text: their names, never their
    /// values. Nothing reads these values to use them.
    pub fn legacy_secrets(&self) -> LegacySecretsView {
        self.legacy_view(&self.data())
    }

    /// Move every plain secret into the credential store. Each value is read
    /// back before it counts. The plain file is overwritten and removed only
    /// when everything in it is safely in the store; a setting is removed from
    /// `plugins.json` when its own value is. A value the store already holds
    /// is kept, and the plain one is dropped.
    pub fn migrate_legacy_secrets(&self) -> Result<LegacyMigration, HostError> {
        let store = &self.inner.secrets;
        if !store.status().available {
            return Err(HostError::Invalid(
                "there is no secure credential store to move them into".into(),
            ));
        }
        let mut data = self.data();
        let mut report = LegacyMigration::default();
        let mut settled: Vec<SecretRef> = Vec::new();

        if let Some(file) = self.legacy_file() {
            match file.read() {
                LegacyContents::Absent => {}
                LegacyContents::Unreadable => report.failed.push(LegacyFailure {
                    plugin: String::new(),
                    key: String::new(),
                    reason: "secrets.json could not be read, so nothing in it was moved".into(),
                }),
                LegacyContents::Secrets(found) => {
                    let done = move_into(store.as_ref(), &found);
                    let complete = done.failed.is_empty();
                    report.absorb(&done, &mut settled);
                    if complete {
                        match file.remove() {
                            Ok(()) => report.file_removed = true,
                            Err(e) => report.failed.push(LegacyFailure {
                                plugin: String::new(),
                                key: String::new(),
                                reason: format!("secrets.json could not be removed: {e}"),
                            }),
                        }
                    }
                }
            }
        }

        let leftovers = Self::leftover_settings(&data);
        if !leftovers.is_empty() {
            let done = move_into(store.as_ref(), &leftovers);
            let mut saved = data.saved.clone();
            for secret in done.moved.iter().chain(&done.kept) {
                if let Some(settings) = saved.plugins.get_mut(&secret.plugin) {
                    settings.config.remove(&secret.key);
                }
            }
            self.persist(&saved)?;
            data.saved = saved;
            report.absorb(&done, &mut settled);
        }

        for secret in &settled {
            let Some(plugin) = data.plugins.get(&secret.plugin).cloned() else {
                continue;
            };
            if plugin.spec().secrets.iter().any(|s| s.key == secret.key) {
                data.secrets_set
                    .insert((secret.plugin.clone(), secret.key.clone()));
                plugin.log().host(
                    Level::Info,
                    &format!(
                        "secret `{}` is now kept in the credential store",
                        secret.key
                    ),
                );
                Self::refresh_now(&mut data, &secret.plugin);
            }
        }
        self.inner
            .grants
            .record(
                AuditEntry::new("secrets.migrated", Actor::User).detail(&format!(
                    "moved {}, already stored {}, failed {}, file removed {}",
                    report.moved,
                    report.kept,
                    report.failed.len(),
                    report.file_removed
                )),
            );
        Ok(report)
    }

    /// Delete the plain secrets without moving them: the file is overwritten
    /// and removed, and leftover settings are removed from `plugins.json`.
    /// Returns how many were deleted.
    pub fn discard_legacy_secrets(&self) -> Result<usize, HostError> {
        let mut data = self.data();
        let before = self.legacy_view(&data);
        if let Some(file) = self.legacy_file() {
            file.remove().map_err(|e| {
                HostError::Storage(format!("secrets.json could not be removed: {e}"))
            })?;
        }
        let leftovers = Self::leftover_settings(&data);
        if !leftovers.is_empty() {
            let mut saved = data.saved.clone();
            for (secret, _) in &leftovers {
                if let Some(settings) = saved.plugins.get_mut(&secret.plugin) {
                    settings.config.remove(&secret.key);
                }
            }
            self.persist(&saved)?;
            data.saved = saved;
        }
        self.inner.grants.record(
            AuditEntry::new("secrets.legacy_discarded", Actor::User)
                .detail(&format!("deleted {}", before.entries.len())),
        );
        Ok(before.entries.len())
    }
}
