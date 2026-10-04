//! Context engine (architecture only in v0.1).
//!
//! The runtime does not poll the foreground app yet; a manual surface switcher
//! is enough for the prototype. This module pins down the seam: something
//! produces a `ContextSnapshot`, and a pure function decides which surface it
//! suggests. A future provider (focused app, project, time, device state,
//! plugin state) only has to fill in the snapshot.

use super::model::Workspace;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextSnapshot {
    /// Foreground application identifier (bundle id, executable name, ...).
    pub foreground_app: Option<String>,
}

/// First surface whose `context.foregroundApps` contains the foreground app
/// (case-insensitive). Returns `None` when nothing matches, so the caller keeps
/// the current surface.
pub fn suggest_surface(workspace: &Workspace, ctx: &ContextSnapshot) -> Option<String> {
    let app = ctx.foreground_app.as_ref()?.to_ascii_lowercase();
    workspace
        .surfaces
        .iter()
        .find(|s| {
            s.context.as_ref().is_some_and(|c| {
                c.foreground_apps
                    .iter()
                    .any(|a| a.to_ascii_lowercase() == app)
            })
        })
        .map(|s| s.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::default_workspace;

    #[test]
    fn foreground_app_suggests_a_surface() {
        let ws = default_workspace();
        let ctx = ContextSnapshot {
            foreground_app: Some("com.microsoft.VSCode".into()),
        };
        assert_eq!(suggest_surface(&ws, &ctx).as_deref(), Some("development"));
    }

    #[test]
    fn unknown_or_missing_context_suggests_nothing() {
        let ws = default_workspace();
        assert_eq!(suggest_surface(&ws, &ContextSnapshot::default()), None);
        let ctx = ContextSnapshot {
            foreground_app: Some("com.example.unknown".into()),
        };
        assert_eq!(suggest_surface(&ws, &ctx), None);
    }
}
