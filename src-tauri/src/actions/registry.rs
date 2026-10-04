//! The unified action registry.
//!
//! It owns no table. It reads the plugin registry (where built-in providers,
//! Agent Control and every installed plugin already register), so what the
//! library shows and what can execute can never drift apart. Registration is
//! therefore one step: a provider that is registered has its actions listed.

use std::collections::HashSet;
use std::sync::Arc;

use super::definition::{
    definition_id, ActionDefinition, Assignment, Availability, Catalog, Cause, Issue, ProviderInfo,
    ProviderKind, ProviderStatus,
};
use super::params::{normalize, validate_defs};
use crate::plugins::external::PluginStatus;
use crate::plugins::{ActionDef, ActionError, Plugin, PluginManifest, PluginRegistry};

/// Why an assignment cannot be run or saved.
#[derive(Debug, Clone, PartialEq)]
pub enum PrepareError {
    UnknownProvider(String),
    UnknownAction { provider: String, action: String },
    Invalid(Vec<Issue>),
}

impl PrepareError {
    pub fn issues(&self) -> Vec<Issue> {
        match self {
            PrepareError::UnknownProvider(p) => {
                vec![Issue::new(format!("`{p}` is not installed or was removed"))]
            }
            PrepareError::UnknownAction { provider, action } => {
                vec![Issue::new(format!("`{provider}` has no action `{action}`"))]
            }
            PrepareError::Invalid(issues) => issues.clone(),
        }
    }
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text: Vec<String> = self.issues().into_iter().map(|i| i.message).collect();
        write!(f, "{}", text.join("; "))
    }
}

impl std::error::Error for PrepareError {}

impl From<PrepareError> for ActionError {
    fn from(e: PrepareError) -> Self {
        match e {
            PrepareError::UnknownProvider(p) => ActionError::UnknownPlugin(p),
            PrepareError::UnknownAction { provider, action } => ActionError::UnknownAction {
                plugin: provider,
                action,
            },
            invalid @ PrepareError::Invalid(_) => ActionError::InvalidArgs(invalid.to_string()),
        }
    }
}

struct Entry {
    plugin: Arc<dyn Plugin>,
    manifest: PluginManifest,
}

#[derive(Clone)]
pub struct ActionRegistry {
    plugins: PluginRegistry,
}

fn rank(kind: ProviderKind) -> u8 {
    match kind {
        ProviderKind::Builtin => 0,
        ProviderKind::Module => 1,
        ProviderKind::Plugin => 2,
    }
}

fn provider_state(m: &PluginManifest) -> (ProviderStatus, Option<String>) {
    match &m.external {
        None => (ProviderStatus::Ready, None),
        Some(e) => match e.status {
            PluginStatus::Enabled => (ProviderStatus::Ready, None),
            PluginStatus::Disabled => (
                ProviderStatus::Disabled,
                Some("The plugin is turned off in Plugins.".to_string()),
            ),
            PluginStatus::Error => (
                ProviderStatus::Error,
                Some(match &e.error {
                    Some(why) => format!("The plugin could not start: {why}"),
                    None => "The plugin could not start.".to_string(),
                }),
            ),
        },
    }
}

impl ActionRegistry {
    pub fn new(plugins: PluginRegistry) -> Self {
        Self { plugins }
    }

    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugins
    }

    /// Providers in a fixed order: built-in, modules, then installed plugins,
    /// each by name. The same order decides who keeps a contested id.
    fn ordered(&self) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .plugins
            .all()
            .into_iter()
            .map(|plugin| {
                let manifest = plugin.manifest();
                Entry { plugin, manifest }
            })
            .collect();
        entries.sort_by(|a, b| {
            (
                rank(a.manifest.kind),
                a.manifest.name.to_lowercase(),
                &a.manifest.id,
            )
                .cmp(&(
                    rank(b.manifest.kind),
                    b.manifest.name.to_lowercase(),
                    &b.manifest.id,
                ))
        });
        entries
    }

    /// Everything the Action Library shows, with availability evaluated now.
    pub fn catalog(&self) -> Catalog {
        let mut providers = Vec::new();
        let mut actions = Vec::new();
        let mut observables = Vec::new();
        let mut issues = Vec::new();
        let mut claimed: HashSet<String> = HashSet::new();
        let mut claimed_observables: HashSet<String> = HashSet::new();
        for Entry { plugin, manifest } in self.ordered() {
            let (status, reason) = provider_state(&manifest);
            providers.push(ProviderInfo {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                kind: manifest.kind,
                version: manifest.version.clone(),
                description: manifest.description.clone(),
                status,
                reason: reason.clone(),
            });
            for def in &manifest.actions {
                let id = definition_id(&manifest.id, &def.id);
                if let Err(why) = check_def(def) {
                    issues.push(format!("{id} was left out: {why}"));
                    continue;
                }
                if !claimed.insert(id.clone()) {
                    issues.push(format!(
                        "{id} was left out: another provider already uses this id"
                    ));
                    continue;
                }
                let availability = match (&status, &reason) {
                    (ProviderStatus::Ready, _) => plugin.availability(&def.id),
                    (_, why) => Availability::unavailable(
                        Cause::Provider,
                        why.clone()
                            .unwrap_or_else(|| "The provider is not running.".into()),
                    ),
                };
                actions.push(ActionDefinition {
                    id,
                    provider: manifest.id.clone(),
                    action: def.id.clone(),
                    name: def.label.clone(),
                    description: def.description.clone(),
                    category: def
                        .category
                        .clone()
                        .unwrap_or_else(|| manifest.name.clone()),
                    keywords: def.keywords.clone(),
                    params: def.params.clone(),
                    availability,
                    privileged: def.privileged,
                    appearance: def.appearance.clone(),
                });
            }
            for def in plugin.observables() {
                if def.provider != manifest.id {
                    issues.push(format!(
                        "{} was left out: it is declared by `{}`",
                        def.id, manifest.id
                    ));
                    continue;
                }
                if let Err(why) = def.check() {
                    issues.push(format!("{} was left out: {why}", def.id));
                    continue;
                }
                if !claimed_observables.insert(def.id.clone()) {
                    issues.push(format!(
                        "{} was left out: another provider already uses this id",
                        def.id
                    ));
                    continue;
                }
                observables.push(def);
            }
        }
        Catalog {
            providers,
            actions,
            observables,
            issues,
        }
    }

    /// The provider and its declaration of one action, if both exist and the
    /// action is not shadowed by an earlier provider.
    fn resolve(
        &self,
        provider: &str,
        action: &str,
    ) -> Result<(Arc<dyn Plugin>, ActionDef), PrepareError> {
        let entries = self.ordered();
        let Some(me) = entries.iter().find(|e| e.manifest.id == provider) else {
            return Err(PrepareError::UnknownProvider(provider.to_string()));
        };
        let unknown = || PrepareError::UnknownAction {
            provider: provider.to_string(),
            action: action.to_string(),
        };
        let def = me
            .manifest
            .actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(unknown)?;
        if check_def(def).is_err() {
            return Err(unknown());
        }
        let id = definition_id(provider, action);
        let owner = entries.iter().find(|e| {
            e.manifest
                .actions
                .iter()
                .any(|a| definition_id(&e.manifest.id, &a.id) == id && check_def(a).is_ok())
        });
        if owner.is_some_and(|o| o.manifest.id != provider) {
            return Err(unknown());
        }
        Ok((me.plugin.clone(), def.clone()))
    }

    /// One definition, with availability evaluated now.
    pub fn definition(&self, provider: &str, action: &str) -> Option<ActionDefinition> {
        let id = definition_id(provider, action);
        self.catalog().actions.into_iter().find(|d| d.id == id)
    }

    /// The only gate to saving or running an assignment: the provider and the
    /// action must exist, the arguments must fit the schema (defaults are
    /// filled in) and the provider's own check must pass. Returns the
    /// normalized assignment.
    pub fn prepare(&self, assignment: &Assignment) -> Result<Assignment, PrepareError> {
        let (plugin, def) = self.resolve(&assignment.provider, &assignment.action)?;
        let args = normalize(&def.params, &assignment.args).map_err(PrepareError::Invalid)?;
        plugin
            .check(&assignment.action, &args)
            .map_err(|issue| PrepareError::Invalid(vec![issue]))?;
        Ok(Assignment {
            provider: assignment.provider.clone(),
            action: assignment.action.clone(),
            args,
        })
    }

    /// Run a provider action that has already been prepared.
    pub(crate) fn plugin(&self, provider: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.get(provider)
    }
}

/// A declaration the registry is willing to list.
fn check_def(def: &ActionDef) -> Result<(), String> {
    if def.label.trim().is_empty() {
        return Err("it has no name".into());
    }
    if let Some(a) = &def.appearance {
        a.check()?;
    }
    validate_defs(&def.params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::params::ParamDef;
    use crate::core::model::Rgb;
    use crate::plugins::{PluginContext, StateDef};
    use serde_json::{json, Value};

    /// A provider whose declarations and availability the test controls.
    struct Fake {
        id: String,
        kind: ProviderKind,
        actions: Vec<ActionDef>,
        unavailable: Option<String>,
    }

    impl Fake {
        fn new(id: &str, actions: Vec<ActionDef>) -> Arc<Fake> {
            Arc::new(Fake {
                id: id.into(),
                kind: ProviderKind::Builtin,
                actions,
                unavailable: None,
            })
        }
    }

    impl Plugin for Fake {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.clone(),
                name: self.id.to_uppercase(),
                version: "1.0.0".into(),
                description: String::new(),
                kind: self.kind,
                actions: self.actions.clone(),
                states: Vec::<StateDef>::new(),
                permissions: vec![],
                external: None,
            }
        }
        fn invoke(&self, _: &PluginContext, _: &str, args: &Value) -> Result<Value, ActionError> {
            Ok(args.clone())
        }
        fn availability(&self, _action: &str) -> Availability {
            match &self.unavailable {
                Some(r) => Availability::unavailable(Cause::State, r.clone()),
                None => Availability::Available,
            }
        }
        fn check(&self, action: &str, args: &Value) -> Result<(), Issue> {
            if action == "picky" && args["word"] == "no" {
                return Err(Issue::param("word", "not that word"));
            }
            Ok(())
        }
    }

    fn registry(plugins: Vec<Arc<dyn Plugin>>) -> ActionRegistry {
        let reg = PluginRegistry::default();
        for p in plugins {
            reg.register(p);
        }
        ActionRegistry::new(reg)
    }

    fn simple(id: &str) -> ActionDef {
        ActionDef::new(id, &format!("Do {id}"), "Does it.")
    }

    #[test]
    fn the_catalog_lists_every_registered_action_with_its_provider() {
        let reg = registry(vec![
            Fake::new(
                "alpha",
                vec![simple("one"), simple("two").category("Group")],
            ),
            Fake::new("beta", vec![simple("one")]),
        ]);
        let c = reg.catalog();
        let ids: Vec<&str> = c.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["alpha.one", "alpha.two", "beta.one"]);
        assert_eq!(c.providers.len(), 2);
        assert_eq!(
            c.actions[0].category, "ALPHA",
            "defaults to the provider name"
        );
        assert_eq!(c.actions[1].category, "Group");
        assert!(c.issues.is_empty());
    }

    #[test]
    fn ids_come_from_provider_and_action_never_from_the_display_name() {
        let renamed = ActionDef::new("approve", "Accept", "Same action, new name.");
        let reg = registry(vec![Fake::new("agents", vec![renamed])]);
        let d = reg.definition("agents", "approve").unwrap();
        assert_eq!(
            (d.id.as_str(), d.name.as_str()),
            ("agents.approve", "Accept")
        );
        // An assignment saved before the rename still resolves.
        let a = Assignment::new("agents", "approve", json!({}));
        assert!(reg.prepare(&a).is_ok());
    }

    #[test]
    fn a_duplicate_id_is_left_out_and_reported() {
        // `a.b` + `c.d` and `a.b.c` + `d` both spell `a.b.c.d`.
        let reg = registry(vec![
            Fake::new("a.b", vec![simple("c.d")]),
            Fake::new("a.b.c", vec![simple("d")]),
        ]);
        let c = reg.catalog();
        assert_eq!(c.actions.len(), 1);
        assert_eq!(c.actions[0].provider, "a.b");
        assert_eq!(c.issues.len(), 1);
        assert!(c.issues[0].contains("a.b.c.d"));
        // The loser cannot be run either.
        assert!(matches!(
            reg.prepare(&Assignment::new("a.b.c", "d", json!({}))),
            Err(PrepareError::UnknownAction { .. })
        ));
        assert!(reg
            .prepare(&Assignment::new("a.b", "c.d", json!({})))
            .is_ok());
    }

    #[test]
    fn a_bad_schema_keeps_one_action_out_not_the_provider() {
        let bad = simple("bad")
            .param(ParamDef::string("x", "X"))
            .param(ParamDef::string("x", "Y"));
        let reg = registry(vec![Fake::new("p", vec![simple("good"), bad])]);
        let c = reg.catalog();
        assert_eq!(c.actions.len(), 1);
        assert_eq!(c.actions[0].id, "p.good");
        assert!(c.issues[0].contains("p.bad") && c.issues[0].contains("twice"));
    }

    #[test]
    fn built_in_modules_and_plugins_are_ordered_and_labelled_by_kind() {
        let mut module = Fake {
            id: "mod".into(),
            kind: ProviderKind::Module,
            actions: vec![simple("m")],
            unavailable: None,
        };
        module.kind = ProviderKind::Module;
        let mut plug = Fake {
            id: "acme.x".into(),
            kind: ProviderKind::Plugin,
            actions: vec![simple("p")],
            unavailable: None,
        };
        plug.kind = ProviderKind::Plugin;
        let reg = registry(vec![
            Arc::new(plug),
            Arc::new(module),
            Fake::new("zed", vec![simple("b")]),
        ]);
        let kinds: Vec<ProviderKind> = reg.catalog().providers.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [
                ProviderKind::Builtin,
                ProviderKind::Module,
                ProviderKind::Plugin
            ]
        );
    }

    #[test]
    fn availability_comes_from_the_provider_and_the_action_stays_listed() {
        let busy = Arc::new(Fake {
            id: "busy".into(),
            kind: ProviderKind::Builtin,
            actions: vec![simple("go")],
            unavailable: Some("nothing is waiting".into()),
        });
        let reg = registry(vec![busy, Fake::new("free", vec![simple("go")])]);
        let c = reg.catalog();
        let busy = c.actions.iter().find(|a| a.provider == "busy").unwrap();
        assert_eq!(
            busy.availability,
            Availability::unavailable(Cause::State, "nothing is waiting")
        );
        assert!(c
            .actions
            .iter()
            .find(|a| a.provider == "free")
            .unwrap()
            .availability
            .is_available());
    }

    #[test]
    fn prepare_normalizes_defaults_and_runs_the_provider_check() {
        let def = simple("picky")
            .param(ParamDef::string("word", "Word").required())
            .param(ParamDef::boolean("loud", "Loud").default_value(json!(false)));
        let reg = registry(vec![Fake::new("p", vec![def])]);
        let ok = reg
            .prepare(&Assignment::new("p", "picky", json!({ "word": "yes" })))
            .unwrap();
        assert_eq!(ok.args, json!({ "word": "yes", "loud": false }));
        let err = reg
            .prepare(&Assignment::new("p", "picky", json!({ "word": "no" })))
            .unwrap_err();
        assert_eq!(err.issues()[0].param.as_deref(), Some("word"));
        let err = reg
            .prepare(&Assignment::new("p", "picky", json!({})))
            .unwrap_err();
        assert_eq!(err.to_string(), "Word is required");
        assert!(matches!(
            reg.prepare(&Assignment::new("nope", "x", json!({}))),
            Err(PrepareError::UnknownProvider(_))
        ));
        assert!(matches!(
            reg.prepare(&Assignment::new("p", "nope", json!({}))),
            Err(PrepareError::UnknownAction { .. })
        ));
    }

    #[test]
    fn appearance_hints_travel_with_the_definition() {
        let reg = registry(vec![Fake::new(
            "p",
            vec![simple("a").look("Approve", Rgb::new(0, 200, 0), "✓")],
        )]);
        let d = reg.definition("p", "a").unwrap();
        let hint = d.appearance.unwrap();
        assert_eq!(hint.label.as_deref(), Some("Approve"));
        let bad = simple("b").look("", Rgb::new(0, 0, 0), "x");
        let reg = registry(vec![Fake::new("q", vec![bad])]);
        assert!(reg.catalog().actions.is_empty());
    }

    #[test]
    fn registry_reads_the_live_plugin_table() {
        let plugins = PluginRegistry::default();
        let reg = ActionRegistry::new(plugins.clone());
        assert!(reg.catalog().actions.is_empty());
        plugins.register(Fake::new("late", vec![simple("x")]));
        assert_eq!(reg.catalog().actions.len(), 1);
        plugins.unregister("late");
        assert!(reg.catalog().actions.is_empty());
    }
}
