//! What export and import know about this installation: as plain data, the
//! app's version, the Context Engine's namespace, the registered providers
//! with the definitions of their actions, who offers states and events, the
//! devices and the applications seen recently; and, for import, the
//! runtime's own checks through a [`PlanHost`].
//!
//! Nothing here holds the runtime's state: `artifacts::service` builds an
//! [`Environment`] from the runtime's getters and implements [`PlanHost`]
//! over its checks, tests build both by hand, and everything that reads it
//! stays a function of what it is given.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::actions::definition::{Assignment, Catalog, Issue, ProviderKind, ProviderStatus};
use crate::actions::params::ParamDef;
use crate::context::identity::ApplicationIdentity;
use crate::devices::{DeviceCapabilities, DeviceKind};
use crate::state::definition::ObservableDefinition;

#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// This build's version, written as `createdWith.version`.
    pub app_version: String,
    /// The namespace the Context Engine observes on this system (`macos`,
    /// `windows`, `x11`); none where it observes nothing.
    pub namespace: Option<String>,
    /// Every registered provider, by id.
    pub providers: BTreeMap<String, KnownProvider>,
    /// Who offers states or events, by provider id: the Observable
    /// Catalog's providers (the runtime, the devices, Agent Control, the
    /// plugins). An event rule naming any other source listens to an
    /// outside program.
    pub observable_sources: BTreeSet<String>,
    pub devices: Vec<KnownDevice>,
    /// Applications seen recently, to offer as answers to an application input.
    pub recent_applications: Vec<ApplicationIdentity>,
    /// The runtime's checks, which import runs on what it would save. None:
    /// nothing is checked against what is installed (export needs nothing).
    pub host: Option<Arc<dyn PlanHost>>,
}

/// What import asks of the running app beyond plain data: the checks the
/// runtime itself makes before it saves an assignment or a state rule.
/// `artifacts::service` implements it over the runtime, tests with a double.
/// An implementation must not take the runtime's workspace lock: the plan is
/// built again inside `Runtime::commit_workspace`, which holds it.
pub trait PlanHost: Send + Sync {
    /// The gate every assignment passes before it is saved
    /// (`Runtime::validate_assignment`): the provider and action exist, the
    /// arguments fit, the provider's own check passes. An issue may name
    /// the parameter it is about.
    fn validate_assignment(&self, assignment: &Assignment) -> Result<Assignment, Vec<Issue>>;

    /// The definition of a state or an event a provider declares now, as the
    /// state hub holds it.
    fn state_definition(&self, provider: &str, id: &str) -> Option<ObservableDefinition>;
}

impl fmt::Debug for dyn PlanHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlanHost")
    }
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
