//! What the artifact tests share: the real built-in catalog and an
//! environment, both from a headless runtime, and a reader that runs the
//! structural checks of a file in the inspect order (plan 7.3, from step 3).

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
        devices,
        recent_applications: Vec::new(),
    }
}

/// A file read as an importer will: the pre-pass, the envelope, the
/// manifest, the digest, the content and the cross-checks.
pub(super) fn read(text: &str) -> Result<ArtifactV1, Rejection> {
    use RejectionCode as R;
    prepass(text)?;
    let envelope: Envelope =
        serde_json::from_str(text).map_err(|e| Rejection::new(R::NotArtifact, e.to_string()))?;
    let kind = ArtifactKind::from_format(&envelope.format)
        .ok_or_else(|| Rejection::new(R::NotArtifact, "not an artifact"))?;
    if envelope.format_version != FORMAT_VERSION {
        return Err(Rejection::new(R::UnsupportedFormatVersion, "version"));
    }
    let manifest: ManifestV1 = serde_json::from_str(envelope.manifest.get())
        .map_err(|e| Rejection::at(R::InvalidManifest, "/manifest", e.to_string()))?;
    check_manifest(&manifest)?;
    if content_digest(envelope.content.get()) != manifest.integrity.content {
        return Err(Rejection::new(R::Integrity, "digest"));
    }
    let content: ContentV1 = serde_json::from_str(envelope.content.get())
        .map_err(|e| Rejection::at(R::InvalidContent, "/content", e.to_string()))?;
    check_content(&content)?;
    check_cross(kind, &manifest, &content)?;
    Ok(ArtifactV1 {
        kind,
        manifest,
        content,
    })
}
