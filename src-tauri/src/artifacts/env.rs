//! What export and import know about this installation, as plain data: the
//! app's version, the Context Engine's namespace, the registered providers
//! with the definitions of their actions, the devices and the applications
//! seen recently.
//!
//! Nothing here holds the runtime or reaches it: `artifacts::service` builds an
//! [`Environment`] from the runtime's getters, tests build one by hand, and
//! everything that reads it stays a pure function.

use std::collections::BTreeMap;

use crate::actions::definition::{Catalog, ProviderKind, ProviderStatus};
use crate::actions::params::ParamDef;
use crate::context::identity::ApplicationIdentity;
use crate::devices::{DeviceCapabilities, DeviceKind};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Environment {
    /// This build's version, written as `createdWith.version`.
    pub app_version: String,
    /// The namespace the Context Engine observes on this system (`macos`,
    /// `windows`, `x11`); none where it observes nothing.
    pub namespace: Option<String>,
    /// Every registered provider, by id.
    pub providers: BTreeMap<String, KnownProvider>,
    pub devices: Vec<KnownDevice>,
    /// Applications seen recently, to offer as answers to an application input.
    pub recent_applications: Vec<ApplicationIdentity>,
}

/// A registered provider. `Builtin` and `Module` providers are part of
/// LaunchDeck (host providers); `Plugin` ones are installed.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownProvider {
    pub kind: ProviderKind,
    /// For display.
    pub name: String,
    /// As the provider declares it: a plugin's manifest version.
    pub version: String,
    pub status: ProviderStatus,
    /// Its actions, by action id.
    pub actions: BTreeMap<String, KnownAction>,
}

/// What an action's definition says that export and import need.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownAction {
    /// The parameters, among them the references to a surface or a page.
    pub params: Vec<ParamDef>,
    /// Runs something on the computer or the network on the person's behalf.
    pub privileged: bool,
}

/// A device the runtime lists. Its id is never part of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownDevice {
    pub name: String,
    /// The device family, such as `launchpad-x`.
    pub model: String,
    pub kind: DeviceKind,
    pub connected: bool,
    pub capabilities: DeviceCapabilities,
}

impl Environment {
    pub fn provider(&self, id: &str) -> Option<&KnownProvider> {
        self.providers.get(id)
    }

    /// The definition of `provider`'s `action`, when both are registered.
    pub fn action(&self, provider: &str, action: &str) -> Option<&KnownAction> {
        self.provider(provider)?.actions.get(action)
    }

    /// The providers of an Action Library catalog, each with its actions,
    /// including those of a provider that is off or failed to load.
    pub fn providers_from(catalog: &Catalog) -> BTreeMap<String, KnownProvider> {
        let mut providers: BTreeMap<String, KnownProvider> = catalog
            .providers
            .iter()
            .map(|p| {
                let known = KnownProvider {
                    kind: p.kind,
                    name: p.name.clone(),
                    version: p.version.clone(),
                    status: p.status,
                    actions: BTreeMap::new(),
                };
                (p.id.clone(), known)
            })
            .collect();
        for def in &catalog.actions {
            if let Some(provider) = providers.get_mut(&def.provider) {
                let action = KnownAction {
                    params: def.params.clone(),
                    privileged: def.privileged,
                };
                provider.actions.insert(def.action.clone(), action);
            }
        }
        providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::params::{ParamKind, RefTarget};
    use crate::artifacts::testkit;

    #[tokio::test]
    async fn the_catalog_gives_every_provider_and_its_actions() {
        let catalog = testkit::built_in_catalog().await;
        let providers = Environment::providers_from(&catalog);
        assert_eq!(providers.len(), catalog.providers.len());
        let env = Environment {
            providers,
            ..Environment::default()
        };
        let system = env.provider("system").unwrap();
        assert_eq!(system.kind, ProviderKind::Builtin);
        assert_eq!(system.status, ProviderStatus::Ready);
        assert!(!system.version.is_empty());
        assert_eq!(
            env.provider("agent-control").unwrap().kind,
            ProviderKind::Module
        );
        let open = env.action("navigation", "openSurface").unwrap();
        let surface = &open.params[0];
        assert_eq!(
            (surface.kind, surface.target),
            (ParamKind::Reference, Some(RefTarget::Surface))
        );
        assert!(!open.privileged);
        assert!(env.action("shell", "execute").unwrap().privileged);
        assert!(env.action("shell", "nothing").is_none());
        assert!(env.action("acme.notes", "mark").is_none());
    }
}
