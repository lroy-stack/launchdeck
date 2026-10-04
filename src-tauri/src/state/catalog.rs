//! The Observable Catalog: everything a control can observe, in one list.
//!
//! ```text
//!   plugin manifests ─┐
//!   Agent Control ────┼─► Source (who offers what) ─┐
//!   the runtime ──────┘                             ├─► build ─► ObservableCatalog ─► UI
//!                      StateHub (values, lifecycle) ┘
//! ```
//!
//! One entry per observable, **state or event**, whoever offers it. A person
//! (or the future Feedback Editor) picks from this list and never from a
//! provider-specific one. The entry says what the observable is (its
//! [`ObservableDefinition`]), who offers it, whether it can be observed right
//! now, and, for a state, what it reads now.
//!
//! The values come from the hub and nowhere else: the UI shows what Rust says
//! and does not rebuild a status from events. Definitions change rarely
//! (a plugin is installed or replaced); values change often. They travel
//! apart: the whole catalog when definitions change, an [`ObservablesDelta`]
//! when only values or the lifecycle of a provider did.
//!
//! Two kinds of observable, kept apart:
//!
//! * a **state** has a current value and a status (`known`, `unknown`,
//!   `unavailable`). One that takes arguments (`slot`, `session`) has many
//!   *instances*; the entry lists the ones that hold a value now, and an
//!   address that is not listed reads `unknown` while its provider runs;
//! * an **event** has no value. It is listed so a rule can name it, with the
//!   payload fields a rule may test.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use super::address::StateKey;
use super::definition::ObservableDefinition;
use super::hub::{Changes, Observed, StateHub};
use super::value::Scalar;
use crate::actions::definition::{Availability, Cause, ProviderInfo, ProviderStatus};

/// Instances of one scoped state the catalog lists. The count is exact; the
/// list is cut so one chatty provider cannot make the catalog large.
pub const MAX_LISTED_INSTANCES: usize = 64;

/// What one provider offers to observe, before the hub is consulted.
#[derive(Debug, Clone)]
pub struct Source {
    pub info: ProviderInfo,
    pub observables: Vec<ObservableDefinition>,
}

/// One instance of a state: where, and what it reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// The arguments that pick the instance; empty for a state without any.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Scalar>,
    #[serde(flatten)]
    pub observed: Observed,
}

/// The part of an entry that changes while definitions stay the same.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    /// Can it be observed right now? A disabled or failed plugin, or a
    /// provider that is not running, makes its observables unavailable.
    pub availability: Availability,
    /// States only. A state without arguments always has exactly one instance
    /// (`known`, `unknown` or `unavailable`). A state with arguments lists the
    /// instances that hold a value, up to [`MAX_LISTED_INSTANCES`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    /// Instances that hold a value now (may be more than `instances` lists).
    pub instance_count: usize,
}

/// One observable: its definition, its provider and what it reads now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservableEntry {
    #[serde(flatten)]
    pub definition: ObservableDefinition,
    /// The provider's display name, so a list needs no second lookup.
    pub provider_name: String,
    #[serde(flatten)]
    pub live: Live,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservableCatalog {
    /// Providers that offer something to observe, in a fixed order: built-in,
    /// modules, then installed plugins.
    pub providers: Vec<ProviderInfo>,
    pub observables: Vec<ObservableEntry>,
    /// Definitions that were left out and why (a duplicate id, a bad schema).
    pub issues: Vec<String>,
}

/// What changed about one observable since the catalog was read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveUpdate {
    pub id: String,
    #[serde(flatten)]
    pub live: Live,
}

/// What the UI receives when something changes. Never the whole catalog.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservablesDelta {
    /// Definitions or providers were added, removed or replaced: read the
    /// catalog again. Everything else in this message is then up to date too.
    pub catalog: bool,
    /// Providers whose status (running, disabled, failed) may have changed.
    pub providers: Vec<ProviderInfo>,
    /// Observables whose availability or values changed.
    pub observables: Vec<LiveUpdate>,
}

impl ObservablesDelta {
    pub fn is_empty(&self) -> bool {
        !self.catalog && self.providers.is_empty() && self.observables.is_empty()
    }
}

fn unavailable(reason: impl Into<String>) -> Availability {
    Availability::unavailable(Cause::Provider, reason)
}

/// Can this observable be observed now?
fn availability(source: &Source, def: &ObservableDefinition, hub: &StateHub) -> Availability {
    if source.info.status != ProviderStatus::Ready {
        return unavailable(
            source
                .info
                .reason
                .clone()
                .unwrap_or_else(|| "The provider is not running.".into()),
        );
    }
    // Only a state depends on the hub: an event is announced when it happens.
    if def.is_state() && hub.provider_is_up(&def.provider) != Some(true) {
        return unavailable("The provider is not reporting right now.");
    }
    Availability::Available
}

/// The values of one entry, from the hub.
fn live(source: &Source, def: &ObservableDefinition, hub: &StateHub) -> Live {
    let availability = availability(source, def, hub);
    if !def.is_state() {
        return Live {
            availability,
            instances: Vec::new(),
            instance_count: 0,
        };
    }
    if def.params.is_empty() {
        // One instance, whatever its status: `unavailable` is a reading too.
        let observed = hub.read(&StateKey::new(&def.provider, &def.local, &BTreeMap::new()));
        let known = usize::from(observed.value.is_some());
        return Live {
            availability,
            instances: vec![Instance {
                args: BTreeMap::new(),
                observed,
            }],
            instance_count: known,
        };
    }
    let view = hub.state_view(&def.provider, &def.local);
    let values = view.map(|v| v.values).unwrap_or_default();
    let instance_count = values.len();
    let instances = values
        .into_iter()
        .take(MAX_LISTED_INSTANCES)
        .map(|(key, observed)| Instance {
            args: key.address().args,
            observed,
        })
        .collect();
    Live {
        availability,
        instances,
        instance_count,
    }
}

/// Build the catalog from what providers offer and what the hub holds.
///
/// Definitions that cannot be listed are left out and reported in `issues`:
/// one that names another provider, one that fails its own check, and a
/// duplicate id (the earlier provider keeps it).
pub fn build(sources: Vec<Source>, hub: &StateHub) -> ObservableCatalog {
    let mut providers = Vec::new();
    let mut observables = Vec::new();
    let mut issues = Vec::new();
    let mut claimed: HashSet<String> = HashSet::new();
    for source in sources {
        let mut offered = 0usize;
        for def in &source.observables {
            if def.provider != source.info.id {
                issues.push(format!(
                    "{} was left out: it is declared by `{}`",
                    def.id, source.info.id
                ));
                continue;
            }
            if let Err(why) = def.check() {
                issues.push(format!("{} was left out: {why}", def.id));
                continue;
            }
            if !claimed.insert(def.id.clone()) {
                issues.push(format!(
                    "{} was left out: another provider already uses this id",
                    def.id
                ));
                continue;
            }
            offered += 1;
            observables.push(ObservableEntry {
                definition: def.clone(),
                provider_name: source.info.name.clone(),
                live: live(&source, def, hub),
            });
        }
        if offered > 0 {
            providers.push(source.info);
        }
    }
    ObservableCatalog {
        providers,
        observables,
        issues,
    }
}

/// What changed, as the UI needs to hear it. Built from what the hub reported
/// since the last time and the catalog as it is now.
///
/// * a change of definitions (`declared`) says only `catalog: true`: the UI
///   reads the catalog again and the rest of the message is empty;
/// * a change of lifecycle (`providers`) refreshes the provider's status and
///   every observable of it, because a provider that stopped makes events
///   unavailable as well as states;
/// * a change of a value (`keys`) refreshes that one state.
///
/// Work is bounded by what changed: one pass over the sources and one hub read
/// per refreshed observable, not a rebuild of the catalog.
pub fn delta(sources: &[Source], hub: &StateHub, changes: &Changes) -> ObservablesDelta {
    if changes.is_empty() {
        return ObservablesDelta::default();
    }
    if !changes.declared.is_empty() {
        return ObservablesDelta {
            catalog: true,
            ..ObservablesDelta::default()
        };
    }
    let mut out = ObservablesDelta::default();
    let states: HashSet<(&str, &str)> = changes
        .keys
        .iter()
        .map(|key| (key.provider(), key.id()))
        .collect();
    for (at, source) in sources.iter().enumerate() {
        let lifecycle = changes.providers.contains(&source.info.id);
        if !lifecycle && !states.iter().any(|(p, _)| *p == source.info.id) {
            continue;
        }
        let mut listed_any = false;
        for def in &source.observables {
            // A lifecycle change reaches everything a provider offers (an event
            // is unavailable while its provider is); a value reaches its state.
            let refreshed = lifecycle
                || (def.is_state()
                    && states.contains(&(def.provider.as_str(), def.local.as_str())));
            if !refreshed || !is_listed(sources, at, def) {
                continue;
            }
            listed_any = true;
            out.observables.push(LiveUpdate {
                id: def.id.clone(),
                live: live(source, def, hub),
            });
        }
        if lifecycle && listed_any {
            out.providers.push(source.info.clone());
        }
    }
    out
}

/// Would [`build`] list this definition? The same three rules, asked of one
/// definition instead of all of them.
fn is_listed(sources: &[Source], at: usize, def: &ObservableDefinition) -> bool {
    let source = &sources[at];
    if def.provider != source.info.id || def.check().is_err() {
        return false;
    }
    let earlier = sources[..at].iter().flat_map(|s| {
        s.observables
            .iter()
            .filter(move |o| o.provider == s.info.id)
    });
    let same_source = source
        .observables
        .iter()
        .take_while(|other| !std::ptr::eq(*other, def))
        .filter(|o| o.provider == source.info.id);
    // Only an earlier definition that was itself listed holds the id.
    !earlier
        .chain(same_source)
        .any(|other| other.id == def.id && other.check().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::definition::ProviderKind;
    use crate::actions::params::ParamDef;
    use crate::state::address::StateAddress;
    use crate::state::definition::{ObservableKind, Role};

    fn info(id: &str, status: ProviderStatus) -> ProviderInfo {
        ProviderInfo {
            id: id.into(),
            name: id.to_uppercase(),
            kind: ProviderKind::Plugin,
            version: "1.0.0".into(),
            description: String::new(),
            status,
            reason: match status {
                ProviderStatus::Ready => None,
                ProviderStatus::Disabled => Some("The plugin is turned off in Plugins.".into()),
                ProviderStatus::Error => Some("The plugin could not start.".into()),
            },
        }
    }

    fn level() -> ObservableDefinition {
        ObservableDefinition::state("acme.counter", "level", "Level", "How full.").one_of(&[
            ("zero", "Empty", Some(Role::Inactive)),
            ("high", "High", None),
        ])
    }

    fn status() -> ObservableDefinition {
        ObservableDefinition::state("acme.counter", "status", "Status", "")
            .param(
                ParamDef::string("channel", "Channel")
                    .max_length(32)
                    .required(),
            )
            .one_of(&[("ok", "Fine", None), ("bad", "Broken", None)])
    }

    fn done() -> ObservableDefinition {
        ObservableDefinition::event("acme.counter", "counter.changed", "Changed", "")
    }

    fn source(status: ProviderStatus) -> Source {
        Source {
            info: info("acme.counter", status),
            observables: vec![level(), status_def(), done()],
        }
    }

    fn status_def() -> ObservableDefinition {
        status()
    }

    fn hub_with(defs: Vec<ObservableDefinition>) -> StateHub {
        let hub = StateHub::new();
        let states: Vec<_> = defs.into_iter().filter(|d| d.is_state()).collect();
        hub.declare("acme.counter", states).unwrap();
        hub.provider_up("acme.counter");
        hub
    }

    fn entry<'a>(c: &'a ObservableCatalog, id: &str) -> &'a ObservableEntry {
        c.observables
            .iter()
            .find(|o| o.definition.id == id)
            .unwrap()
    }

    #[test]
    fn one_list_holds_states_and_events_with_their_provider() {
        let hub = hub_with(vec![level(), status()]);
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let ids: Vec<&str> = c
            .observables
            .iter()
            .map(|o| o.definition.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "acme.counter.level",
                "acme.counter.status",
                "acme.counter.counter.changed"
            ]
        );
        assert!(c.issues.is_empty());
        assert_eq!(c.providers.len(), 1);
        assert_eq!(
            entry(&c, "acme.counter.level").provider_name,
            "ACME.COUNTER"
        );
        assert_eq!(
            entry(&c, "acme.counter.counter.changed").definition.kind,
            ObservableKind::Event
        );
    }

    #[test]
    fn an_unscoped_state_has_one_instance_that_is_unknown_then_known() {
        let hub = hub_with(vec![level(), status()]);
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let e = entry(&c, "acme.counter.level");
        assert_eq!(e.live.availability, Availability::Available);
        assert_eq!(e.live.instances.len(), 1);
        assert_eq!(e.live.instances[0].observed, Observed::unknown());
        assert_eq!(e.live.instance_count, 0);

        hub.set(
            &StateAddress::new("acme.counter", "level"),
            Scalar::text("high"),
        )
        .unwrap();
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let e = entry(&c, "acme.counter.level");
        assert_eq!(
            e.live.instances[0].observed.status,
            crate::state::hub::Status::Known
        );
        assert_eq!(
            e.live.instances[0].observed.value,
            Some(Scalar::text("high"))
        );
        assert_eq!(e.live.instance_count, 1);
    }

    #[test]
    fn a_scoped_state_lists_the_instances_that_hold_a_value() {
        let hub = hub_with(vec![level(), status()]);
        for (channel, v) in [("b", "bad"), ("a", "ok")] {
            hub.set(
                &StateAddress::new("acme.counter", "status").with("channel", channel),
                Scalar::text(v),
            )
            .unwrap();
        }
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let e = entry(&c, "acme.counter.status");
        assert_eq!(e.live.instance_count, 2);
        let listed: Vec<(String, Option<Scalar>)> = e
            .live
            .instances
            .iter()
            .map(|i| (i.args["channel"].to_string(), i.observed.value.clone()))
            .collect();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|(_, v)| *v == Some(Scalar::text("bad"))));
        let json = serde_json::to_value(e).unwrap();
        assert_eq!(json["instances"][0]["args"]["channel"], "a");
        assert_eq!(json["instances"][0]["status"], "known");
        assert_eq!(json["params"][0]["name"], "channel");
    }

    #[test]
    fn a_long_list_is_cut_but_the_count_is_exact() {
        let hub = hub_with(vec![level(), status()]);
        for i in 0..(MAX_LISTED_INSTANCES + 10) {
            hub.set(
                &StateAddress::new("acme.counter", "status")
                    .with("channel", Scalar::text(format!("c{i:03}"))),
                Scalar::text("ok"),
            )
            .unwrap();
        }
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let e = entry(&c, "acme.counter.status");
        assert_eq!(e.live.instances.len(), MAX_LISTED_INSTANCES);
        assert_eq!(e.live.instance_count, MAX_LISTED_INSTANCES + 10);
    }

    #[test]
    fn a_provider_that_is_down_makes_everything_it_offers_unavailable() {
        let hub = hub_with(vec![level(), status()]);
        hub.set(
            &StateAddress::new("acme.counter", "level"),
            Scalar::text("high"),
        )
        .unwrap();
        hub.provider_down("acme.counter");
        let c = build(vec![source(ProviderStatus::Disabled)], &hub);
        for o in &c.observables {
            assert_eq!(
                o.live.availability,
                Availability::unavailable(Cause::Provider, "The plugin is turned off in Plugins."),
                "{}",
                o.definition.id
            );
        }
        let e = entry(&c, "acme.counter.level");
        assert_eq!(e.live.instances[0].observed, Observed::unavailable());
        assert_eq!(e.live.instance_count, 0, "nothing it said is kept");
        assert!(entry(&c, "acme.counter.status").live.instances.is_empty());
    }

    #[test]
    fn coming_back_reads_unknown_never_the_old_value() {
        let hub = hub_with(vec![level(), status()]);
        let addr = StateAddress::new("acme.counter", "level");
        hub.set(&addr, Scalar::text("high")).unwrap();
        hub.provider_down("acme.counter");
        hub.provider_up("acme.counter");
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let e = entry(&c, "acme.counter.level");
        assert_eq!(e.live.instances[0].observed, Observed::unknown());
        assert_eq!(e.live.availability, Availability::Available);
    }

    #[test]
    fn what_cannot_be_listed_is_left_out_and_reported() {
        let hub = hub_with(vec![level()]);
        let stranger = ObservableDefinition::state("other.thing", "x", "X", "").number();
        let mut bad = level();
        bad.local = "Bad Id".into();
        bad.id = "acme.counter.Bad Id".into();
        let dup = Source {
            info: info("acme.counter.more", ProviderStatus::Ready),
            observables: vec![
                ObservableDefinition::state("acme.counter.more", "x", "X", "").number(),
            ],
        };
        // `acme.counter` + `level` and `acme` + `counter.level` spell one id.
        let squatter = Source {
            info: info("acme", ProviderStatus::Ready),
            observables: vec![
                ObservableDefinition::state("acme", "counter.level", "L", "").number(),
            ],
        };
        let mut first = source(ProviderStatus::Ready);
        first.observables.push(stranger);
        first.observables.push(bad);
        let c = build(vec![first, dup, squatter], &hub);
        assert_eq!(c.issues.len(), 3, "{:?}", c.issues);
        assert!(c.issues[0].contains("other.thing.x"));
        assert!(c.issues[2].contains("another provider already uses this id"));
        assert!(
            c.providers.iter().all(|p| p.id != "acme"),
            "a provider with nothing listed is not listed"
        );
        assert_eq!(c.providers.len(), 2);
    }

    #[test]
    fn serialises_the_definition_and_the_live_part_side_by_side() {
        let hub = hub_with(vec![level(), status()]);
        let c = build(vec![source(ProviderStatus::Ready)], &hub);
        let v = serde_json::to_value(entry(&c, "acme.counter.level")).unwrap();
        assert_eq!(v["id"], "acme.counter.level");
        assert_eq!(v["providerName"], "ACME.COUNTER");
        assert_eq!(v["kind"], "state");
        assert_eq!(v["value"]["type"], "enum");
        assert_eq!(v["availability"]["status"], "available");
        assert_eq!(v["instances"][0]["status"], "unknown");
        assert_eq!(v["instanceCount"], 0);
        let ev = serde_json::to_value(entry(&c, "acme.counter.counter.changed")).unwrap();
        assert_eq!(ev["kind"], "event");
        assert!(ev.get("instances").is_none());
    }

    #[test]
    fn a_value_change_refreshes_only_the_state_that_changed() {
        let hub = hub_with(vec![level(), status()]);
        let sources = vec![source(ProviderStatus::Ready)];
        let addr = StateAddress::new("acme.counter", "level");
        hub.drain();
        hub.set(&addr, Scalar::text("high")).unwrap();
        let d = delta(&sources, &hub, &hub.drain());
        assert!(!d.catalog && d.providers.is_empty());
        assert_eq!(d.observables.len(), 1);
        assert_eq!(d.observables[0].id, "acme.counter.level");
        assert_eq!(
            d.observables[0].live.instances[0].observed.value,
            Some(Scalar::text("high"))
        );
    }

    #[test]
    fn a_lifecycle_change_refreshes_the_provider_and_every_observable_of_it() {
        let hub = hub_with(vec![level(), status()]);
        hub.drain();
        hub.provider_down("acme.counter");
        let sources = vec![source(ProviderStatus::Error)];
        let d = delta(&sources, &hub, &hub.drain());
        assert!(!d.catalog);
        assert_eq!(d.providers.len(), 1);
        assert_eq!(d.providers[0].status, ProviderStatus::Error);
        let ids: Vec<&str> = d.observables.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "acme.counter.level",
                "acme.counter.status",
                "acme.counter.counter.changed"
            ],
            "events become unavailable too"
        );
        assert!(d
            .observables
            .iter()
            .all(|o| matches!(o.live.availability, Availability::Unavailable { .. })));
    }

    #[test]
    fn a_change_of_definitions_asks_for_the_catalog_again() {
        let hub = hub_with(vec![level()]);
        hub.drain();
        hub.catalog_changed("acme.counter");
        let d = delta(&[source(ProviderStatus::Ready)], &hub, &hub.drain());
        assert!(d.catalog);
        assert!(d.observables.is_empty() && d.providers.is_empty());
        assert!(delta(&[], &hub, &Changes::default()).is_empty());
    }
}
