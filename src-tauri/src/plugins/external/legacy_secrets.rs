//! Secrets an earlier version kept in plain text: the `secrets.json` file a
//! system without a credential store fell back to, and values in
//! `plugins.json` under a key the plugin's manifest names as a credential (a
//! setting marked `secret` by a manifest of schema 1 to 3, or a setting of an
//! older version that the current manifest declares under `secrets`).
//!
//! They are listed by name in the host's overview and on the plugin's card,
//! never handed to a plugin, and moved into the credential store or deleted
//! only when the person asks.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::host::{Data, HostError, PluginHost, PluginInfo, SavedState};
use super::log::Level;
use super::secrets::{move_into, LegacyContents, LegacyFile, Moved, SecretRef, SecretStore};
use super::wasm_plugin::{credential_keys, WasmPlugin};
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

/// What a plugin's card says about its credentials that LaunchDeck does not
/// use. Names only, never values.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecretMigration {
    /// Migration required: values an earlier version kept in plain text in
    /// `plugins.json`. Never handed to the plugin; moved into the credential
    /// store or removed only when the person asks.
    pub migration_required: Vec<String>,
    /// In the credential store and still unused: this version names them only
    /// as settings marked `secret` (schemas 1 to 3), and a module never
    /// receives a stored secret. A version that declares them under `secrets`
    /// (schema 4) can use them.
    pub needs_update: Vec<String>,
}

/// A value from `plugins.json` as the text the credential store would keep.
/// Nothing for `null` or an empty string, which hold no secret.
fn plain_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The values `plugins.json` holds in plain text under a key that `plugin`'s
/// manifest names as a credential. In key order.
fn plain_values(plugin: &WasmPlugin, saved: &SavedState) -> Vec<(SecretRef, String)> {
    let Some(settings) = saved.plugins.get(plugin.id()) else {
        return Vec::new();
    };
    let mut keys: Vec<&str> = credential_keys(plugin.spec()).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|key| {
            let value = plain_text(settings.config.get(key)?)?;
            Some((SecretRef::new(plugin.id(), key), value))
        })
        .collect()
}

/// [`move_into`] held to one more rule for values in `plugins.json`: a plain
/// value may be removed only when the store holds exactly that value
/// afterwards. A different value already in the store is not replaced, and
/// the plain one is not removed either; the person removes whichever is wrong.
fn move_settings(store: &dyn SecretStore, entries: &[(SecretRef, String)]) -> Moved {
    let mut done = move_into(store, entries);
    let (same, different): (Vec<SecretRef>, Vec<SecretRef>) =
        done.kept.drain(..).partition(|secret| {
            let plain = entries.iter().find(|(s, _)| s == secret).map(|(_, v)| v);
            matches!(store.get(secret), Ok(Some(held)) if Some(&held) == plain)
        });
    done.kept = same;
    done.failed.extend(different.into_iter().map(|secret| {
        (
            secret,
            "the credential store already holds a different value for it".to_string(),
        )
    }));
    done
}

/// The sentence for values that were not moved: which, and why.
fn not_moved(failed: &[(SecretRef, String)]) -> String {
    let each: Vec<String> = failed
        .iter()
        .map(|(secret, reason)| format!("`{}` could not be moved ({reason})", secret.key))
        .collect();
    format!(
        "Nothing was removed from plugins.json, so the plain {} still where {}: {}.",
        if failed.len() == 1 {
            "value is"
        } else {
            "values are"
        },
        if failed.len() == 1 {
            "it was"
        } else {
            "they were"
        },
        each.join("; ")
    )
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

    /// Values left in plain text in `plugins.json` under a key that a
    /// plugin's manifest names as a credential. In a fixed order (plugin,
    /// then key).
    fn leftover_settings(data: &Data) -> Vec<(SecretRef, String)> {
        data.plugins
            .values()
            .flat_map(|plugin| plain_values(plugin, &data.saved))
            .collect()
    }

    /// What `plugin`'s card says about credentials LaunchDeck does not use,
    /// when there are any.
    pub(super) fn secret_migration(data: &Data, plugin: &WasmPlugin) -> Option<SecretMigration> {
        let migration_required: Vec<String> = plain_values(plugin, &data.saved)
            .into_iter()
            .map(|(secret, _)| secret.key)
            .collect();
        let needs_update: Vec<String> = plugin
            .spec()
            .config
            .iter()
            .filter(|f| f.secret)
            .filter(|f| {
                data.secrets_set
                    .contains(&(plugin.id().to_string(), f.key.clone()))
            })
            .map(|f| f.key.clone())
            .collect();
        (!migration_required.is_empty() || !needs_update.is_empty()).then_some(SecretMigration {
            migration_required,
            needs_update,
        })
    }

    /// Say in `plugin`'s log which of its values sit unused in plain text.
    pub(super) fn note_plain_values(plugin: &WasmPlugin, saved: &SavedState) {
        for (secret, _) in plain_values(plugin, saved) {
            plugin.log().host(
                Level::Warn,
                &format!(
                    "the value of `{}` that an earlier version kept in plugins.json without encryption is not used: move it to the credential store or remove it, on the plugin's card",
                    secret.key
                ),
            );
        }
    }

    /// `secret` is now in the credential store: remember that, and say what
    /// it means for its plugin.
    fn now_stored(data: &mut Data, secret: &SecretRef) {
        let Some(plugin) = data.plugins.get(&secret.plugin).cloned() else {
            return;
        };
        if !credential_keys(plugin.spec()).any(|k| k == secret.key) {
            return;
        }
        data.secrets_set
            .insert((secret.plugin.clone(), secret.key.clone()));
        let declared = plugin.spec().secrets.iter().any(|s| s.key == secret.key);
        let said = if declared {
            format!(
                "secret `{}` is now kept in the credential store",
                secret.key
            )
        } else {
            format!(
                "secret `{}` is now kept in the credential store; this version of the plugin cannot use it, it needs a version that declares it under `secrets`",
                secret.key
            )
        };
        plugin.log().host(Level::Info, &said);
        Self::refresh_now(data, &secret.plugin);
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
    /// when everything in it is safely in the store (a value the store already
    /// holds is kept, and the plain one dropped); a value is removed from
    /// `plugins.json` only when the store holds exactly that value.
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
            let done = move_settings(store.as_ref(), &leftovers);
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
            Self::now_stored(&mut data, secret);
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

    /// Move the plain values of one plugin into the credential store: each is
    /// written, read back and compared, and only when every one of them is
    /// safely there are they removed from `plugins.json`. If anything fails,
    /// `plugins.json` is left as it was, what this attempt wrote to the store
    /// is deleted again, and the error says why.
    pub fn migrate_plugin_legacy_secrets(&self, id: &str) -> Result<PluginInfo, HostError> {
        let store = &self.inner.secrets;
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let status = store.status();
        if !status.available {
            return Err(HostError::Invalid(format!(
                "There is no secure credential store to move it into. {}",
                status.reason.unwrap_or_default()
            )));
        }
        let entries = plain_values(&plugin, &data.saved);
        if entries.is_empty() {
            return Ok(self.info(&data, &plugin));
        }
        let done = move_settings(store.as_ref(), &entries);
        let removed = if done.failed.is_empty() {
            self.persist_without(&mut data, id, &entries)
        } else {
            Err(HostError::Invalid(not_moved(&done.failed)))
        };
        if let Err(e) = removed {
            Self::undo_writes(&mut data, store.as_ref(), &done.moved);
            plugin.log().host(Level::Warn, &e.to_string());
            let detail = match done.failed.len() {
                0 => "plugins.json not saved, nothing removed".to_string(),
                n => format!("failed {n}, nothing removed"),
            };
            self.inner.grants.record(
                AuditEntry::new("secrets.migrated", Actor::User)
                    .plugin(id)
                    .detail(&detail),
            );
            return Err(e);
        }
        for (secret, _) in &entries {
            Self::now_stored(&mut data, secret);
        }
        self.inner.grants.record(
            AuditEntry::new("secrets.migrated", Actor::User)
                .plugin(id)
                .detail(&format!(
                    "moved {}, already stored {}",
                    done.moved.len(),
                    done.kept.len()
                )),
        );
        Ok(self.info(&data, &plugin))
    }

    /// Delete from the store what a failed migration wrote there, so that
    /// the store is as it was. One that cannot be deleted is still there, and
    /// is known to be.
    fn undo_writes(data: &mut Data, store: &dyn SecretStore, written: &[SecretRef]) {
        for secret in written {
            if store.delete(secret).is_err() {
                Self::now_stored(data, secret);
            }
        }
    }

    /// Remove `entries` from plugin `id`'s settings and save `plugins.json`.
    fn persist_without(
        &self,
        data: &mut Data,
        id: &str,
        entries: &[(SecretRef, String)],
    ) -> Result<(), HostError> {
        let mut saved = data.saved.clone();
        if let Some(settings) = saved.plugins.get_mut(id) {
            for (secret, _) in entries {
                settings.config.remove(&secret.key);
            }
        }
        self.persist(&saved)?;
        data.saved = saved;
        Ok(())
    }

    /// Delete the plain values of one plugin from `plugins.json` without
    /// moving them. Nothing else changes.
    pub fn remove_plugin_legacy_secrets(&self, id: &str) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let entries = plain_values(&plugin, &data.saved);
        if entries.is_empty() {
            return Ok(self.info(&data, &plugin));
        }
        self.persist_without(&mut data, id, &entries)?;
        for (secret, _) in &entries {
            plugin.log().host(
                Level::Info,
                &format!(
                    "the plain value of `{}` was removed from plugins.json",
                    secret.key
                ),
            );
        }
        self.inner.grants.record(
            AuditEntry::new("secrets.legacy_discarded", Actor::User)
                .plugin(id)
                .detail(&format!("deleted {}", entries.len())),
        );
        Ok(self.info(&data, &plugin))
    }
}
