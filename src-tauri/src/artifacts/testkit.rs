//! What the artifact tests share: the real built-in catalog and an
//! environment, both from a headless runtime, and a file read as an
//! importer reads it.

use std::sync::Arc;

use super::env::{Environment, KnownDevice};
use super::*;
use crate::actions::definition::Catalog;
use crate::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use crate::plugins::keyboard::{Combo, Keyboard};
use crate::plugins::ActionError;
use crate::surfaces::default_workspace;

struct NoKeyboard;

impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// A runtime with every built-in provider, the virtual device and no plugin.
pub(super) async fn headless() -> Arc<Runtime> {
    Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    })
    .await
}

pub(super) async fn built_in_catalog() -> Catalog {
    headless().await.catalog()
}

/// What a headless runtime knows, with `app_version` as the app's version
/// (fixed in golden files, so a release does not change them).
pub(super) async fn environment(app_version: &str) -> Environment {
    let rt = headless().await;
    let devices = rt
        .list_devices()
        .into_iter()
        .map(|d| KnownDevice {
            name: d.identity.name,
            model: d.identity.model,
            kind: d.identity.kind,
            connected: d.connected,
            capabilities: d.capabilities,
        })
        .collect();
    Environment {
        app_version: app_version.into(),
        namespace: None,
        providers: Environment::providers_from(&rt.catalog()),
        observable_sources: rt
            .observables()
            .providers
            .into_iter()
            .map(|p| p.id)
            .collect(),
        devices,
        recent_applications: Vec::new(),
        host: None,
    }
}

/// A file read as an importer reads it, by [`inspect`].
pub(super) fn read(text: &str) -> Result<ArtifactV1, Rejection> {
    let parsed = inspect(text.as_bytes(), "file")?;
    Ok(ArtifactV1 {
        kind: parsed.kind,
        manifest: parsed.manifest,
        content: parsed.content,
    })
}
