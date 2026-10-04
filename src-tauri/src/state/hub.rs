//! The state hub: the current value of every observable state, and nothing else.
//!
//! ```text
//!   provider ──set / clear──►  ┌──────────┐  ──read──►  evaluator
//!   host ──up / down / declare─►  │  StateHub │
//!                              └──────────┘  ──drain──►  "these keys changed"
//! ```
//!
//! What it is not:
//!
//! * **Not an event bus.** Writes never travel through the broadcast channel,
//!   so a slow reader cannot make a write disappear. A burst of writes to one
//!   key is one entry in the dirty set.
//! * **Not a history.** One value per key. The previous one is gone.
//! * **Not persistent.** A value is never written to disk. After a restart every
//!   state is `unknown` until its provider reports again.
//!
//! Locking: one mutex, and it is a *leaf*: nothing is called while it is held
//! (no callback, no await, no other lock). A caller may therefore hold its own
//! lock and write here, which is what Agent Control and the runtime do.
//!
//! Provider lifecycle, which is the only way a value can be left behind by a
//! provider that is no longer the one that wrote it:
//!
//! ```text
//!   (absent) ──declare──► declared, down ──up──► up, every value unknown
//!   up ──set──► that value known        up ──down──► down, every value unavailable
//!   down ──up──► up, every value unknown (never the old value)
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tokio::sync::Notify;

use super::address::{StateAddress, StateKey};
use super::definition::{Fit, ObservableDefinition};
use super::value::Scalar;
use crate::actions::params::normalize;
use crate::events::now_ms;

/// States one provider may declare.
pub const MAX_STATES_PER_PROVIDER: usize = 64;
/// Instances (distinct addresses with a value) one provider may hold.
pub const MAX_INSTANCES_PER_PROVIDER: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The provider reported a value and it is the current one.
    Known,
    /// The state exists but has no value yet: the provider (or the app) just
    /// started and has not reported.
    Unknown,
    /// The provider cannot answer: disabled, failed, uninstalled, or the state
    /// is not declared by it.
    Unavailable,
}

/// What a read returns: a value only with status `known`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observed {
    pub status: Status,
    pub value: Option<Scalar>,
    /// Milliseconds since the Unix epoch of the last report.
    pub updated_at: Option<u64>,
}

impl Observed {
    pub fn known(value: Scalar, at: u64) -> Self {
        Self {
            status: Status::Known,
            value: Some(value),
            updated_at: Some(at),
        }
    }

    pub fn unknown() -> Self {
        Self {
            status: Status::Unknown,
            value: None,
            updated_at: None,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            status: Status::Unavailable,
            value: None,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetError {
    #[error("provider `{0}` is not registered")]
    NoProvider(String),
    #[error("provider `{0}` is not running")]
    ProviderDown(String),
    #[error("provider `{provider}` declares no state `{id}`")]
    Undeclared { provider: String, id: String },
    #[error("{0}")]
    BadArgs(String),
    #[error("`{id}` takes {expected}, not {got}")]
    WrongType {
        id: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("provider `{0}` holds too many instances")]
    TooMany(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetOutcome {
    /// The stored value is different from what was there (or nothing was).
    pub changed: bool,
    /// An enum value the definition does not list. Stored anyway.
    pub unlisted: bool,
}

/// What changed since the last [`StateHub::drain`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changes {
    /// States whose observed value or status changed one by one.
    pub keys: HashSet<StateKey>,
    /// Providers whose lifecycle or declarations changed: every state of
    /// theirs may read differently now.
    pub providers: HashSet<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.providers.is_empty()
    }
}

struct Stored {
    value: Scalar,
    at: u64,
}

#[derive(Default)]
struct Provider {
    up: bool,
    defs: BTreeMap<String, ObservableDefinition>,
    values: HashMap<StateKey, Stored>,
}

#[derive(Default)]
struct Inner {
    providers: HashMap<String, Provider>,
    dirty: Changes,
}

struct Shared {
    inner: Mutex<Inner>,
    wake: Notify,
}

#[derive(Clone)]
pub struct StateHub {
    shared: Arc<Shared>,
}

impl Default for StateHub {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHub {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner::default()),
                wake: Notify::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wake(&self) {
        self.shared.wake.notify_one();
    }

    // -----------------------------------------------------------------------
    // Provider lifecycle
    // -----------------------------------------------------------------------

    /// Declare (replace) the states a provider offers. All or nothing: a bad
    /// definition refuses the whole list and nothing changes. Values of states
    /// that are no longer declared are dropped; the others stay. The provider
    /// is not started by this (see [`provider_up`](Self::provider_up)).
    pub fn declare(&self, provider: &str, defs: Vec<ObservableDefinition>) -> Result<(), String> {
        if defs.len() > MAX_STATES_PER_PROVIDER {
            return Err(format!(
                "`{provider}` declares more than {MAX_STATES_PER_PROVIDER} states"
            ));
        }
        let mut map = BTreeMap::new();
        for d in defs {
            if d.provider != provider {
                return Err(format!("`{}` belongs to another provider", d.id));
            }
            if !d.is_state() {
                return Err(format!("`{}` is an event, not a state", d.id));
            }
            d.check()?;
            let local = d.local.clone();
            if map.insert(local.clone(), d).is_some() {
                return Err(format!("`{provider}.{local}` is declared twice"));
            }
        }
        let mut g = self.lock();
        let p = g.providers.entry(provider.to_string()).or_default();
        if p.defs == map {
            return Ok(());
        }
        p.defs = map;
        let defs = &p.defs;
        p.values.retain(|k, _| defs.contains_key(k.id()));
        g.dirty.providers.insert(provider.to_string());
        drop(g);
        self.wake();
        Ok(())
    }

    /// The provider is running. Every one of its states is `unknown` from now
    /// until it reports, whatever it held before: a provider that starts again
    /// has lost its memory, so an old value must not pass for a current one.
    pub fn provider_up(&self, provider: &str) {
        let mut g = self.lock();
        let p = g.providers.entry(provider.to_string()).or_default();
        p.up = true;
        p.values.clear();
        g.dirty.providers.insert(provider.to_string());
        drop(g);
        self.wake();
    }

    /// The provider stopped, failed or was disabled: its states are
    /// `unavailable` and nothing it said is kept.
    pub fn provider_down(&self, provider: &str) {
        let mut g = self.lock();
        let Some(p) = g.providers.get_mut(provider) else {
            return;
        };
        p.up = false;
        p.values.clear();
        g.dirty.providers.insert(provider.to_string());
        drop(g);
        self.wake();
    }

    /// The provider is gone (uninstalled): declarations and values with it.
    pub fn remove_provider(&self, provider: &str) {
        let mut g = self.lock();
        if g.providers.remove(provider).is_some() {
            g.dirty.providers.insert(provider.to_string());
            drop(g);
            self.wake();
        }
    }

    /// Declare the states of a provider that is always running (Agent Control,
    /// the Demo plugin) and start it if it is not. A provider that is already
    /// up keeps the values it has reported.
    pub fn ensure_provider(
        &self,
        provider: &str,
        defs: Vec<ObservableDefinition>,
    ) -> Result<(), String> {
        self.declare(provider, defs)?;
        let up = self.lock().providers.get(provider).is_some_and(|p| p.up);
        if !up {
            self.provider_up(provider);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    /// A provider reports the current value of one of its states.
    pub fn set(&self, addr: &StateAddress, value: Scalar) -> Result<SetOutcome, SetError> {
        let mut g = self.lock();
        let p = g
            .providers
            .get_mut(&addr.provider)
            .ok_or_else(|| SetError::NoProvider(addr.provider.clone()))?;
        if !p.up {
            return Err(SetError::ProviderDown(addr.provider.clone()));
        }
        let def = p.defs.get(&addr.id).ok_or_else(|| SetError::Undeclared {
            provider: addr.provider.clone(),
            id: addr.id.clone(),
        })?;
        let key = canonical_key(def, addr)?;
        let spec = def.value.as_ref().ok_or_else(|| SetError::Undeclared {
            provider: addr.provider.clone(),
            id: addr.id.clone(),
        })?;
        let unlisted = match spec.fit(&value) {
            Fit::Declared => false,
            Fit::Unlisted => true,
            Fit::WrongType => {
                return Err(SetError::WrongType {
                    id: addr.id.clone(),
                    expected: expected_of(spec.kind),
                    got: value.kind(),
                })
            }
        };
        let existing = p.values.get(&key);
        if existing.is_none() && p.values.len() >= MAX_INSTANCES_PER_PROVIDER {
            return Err(SetError::TooMany(addr.provider.clone()));
        }
        let changed = existing.is_none_or(|s| !s.value.same_as(&value));
        p.values.insert(
            key.clone(),
            Stored {
                value,
                at: now_ms(),
            },
        );
        if changed {
            g.dirty.keys.insert(key);
            drop(g);
            self.wake();
        }
        Ok(SetOutcome { changed, unlisted })
    }

    /// One instance has no value any more (a session ended): it reads
    /// `unknown` again. Returns whether there was a value.
    pub fn clear(&self, addr: &StateAddress) -> bool {
        let mut g = self.lock();
        let Some(p) = g.providers.get_mut(&addr.provider) else {
            return false;
        };
        let key = match p.defs.get(&addr.id) {
            Some(def) => canonical_key(def, addr).unwrap_or_else(|_| addr.key()),
            None => addr.key(),
        };
        let removed = p.values.remove(&key).is_some();
        if removed {
            g.dirty.keys.insert(key);
            drop(g);
            self.wake();
        }
        removed
    }

    // -----------------------------------------------------------------------
    // Reading
    // -----------------------------------------------------------------------

    /// The current observation of one state. Never blocks on a provider.
    pub fn read(&self, key: &StateKey) -> Observed {
        let g = self.lock();
        let Some(p) = g.providers.get(key.provider()) else {
            return Observed::unavailable();
        };
        if !p.up || !p.defs.contains_key(key.id()) {
            return Observed::unavailable();
        }
        match p.values.get(key) {
            Some(s) => Observed::known(s.value.clone(), s.at),
            None => Observed::unknown(),
        }
    }

    pub fn read_address(&self, addr: &StateAddress) -> Observed {
        self.read(&addr.key())
    }

    /// Is this provider registered and running?
    pub fn provider_is_up(&self, provider: &str) -> Option<bool> {
        self.lock().providers.get(provider).map(|p| p.up)
    }

    /// Every state declared by every provider, whether it is running or not.
    pub fn definitions(&self) -> Vec<ObservableDefinition> {
        let g = self.lock();
        let mut ids: Vec<&String> = g.providers.keys().collect();
        ids.sort();
        ids.into_iter()
            .flat_map(|id| g.providers[id].defs.values().cloned())
            .collect()
    }

    /// The declaration of one state, if its provider is registered and offers it.
    pub fn definition(&self, provider: &str, id: &str) -> Option<ObservableDefinition> {
        self.lock()
            .providers
            .get(provider)
            .and_then(|p| p.defs.get(id))
            .cloned()
    }

    /// Every value that is currently known, with its age. For diagnostics and
    /// tests; nothing is built from this.
    pub fn known(&self) -> Vec<(StateKey, Observed)> {
        let g = self.lock();
        let mut out: Vec<(StateKey, Observed)> = g
            .providers
            .values()
            .filter(|p| p.up)
            .flat_map(|p| {
                p.values
                    .iter()
                    .map(|(k, s)| (k.clone(), Observed::known(s.value.clone(), s.at)))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    // -----------------------------------------------------------------------
    // Invalidation
    // -----------------------------------------------------------------------

    /// Wait until something changed. A change that happened before the call is
    /// not lost: the wake-up is remembered. May return with nothing to drain.
    pub async fn changed(&self) {
        self.shared.wake.notified().await;
    }

    /// Take what changed since the last drain. Writes to one key between two
    /// drains are one entry.
    pub fn drain(&self) -> Changes {
        std::mem::take(&mut self.lock().dirty)
    }
}

fn expected_of(kind: super::definition::ValueType) -> &'static str {
    use super::definition::ValueType::*;
    match kind {
        Enum | String => "a text",
        Boolean => "a boolean",
        Number => "a number",
    }
}

/// The key of an address after its arguments were checked against the
/// definition's parameter schema (unknown arguments refused, defaults filled).
fn canonical_key(def: &ObservableDefinition, addr: &StateAddress) -> Result<StateKey, SetError> {
    if def.params.is_empty() {
        if !addr.args.is_empty() {
            return Err(SetError::BadArgs(format!(
                "`{}` takes no arguments",
                addr.id
            )));
        }
        return Ok(addr.key());
    }
    let normalized = normalize(&def.params, &addr.args_json()).map_err(|issues| {
        SetError::BadArgs(
            issues
                .into_iter()
                .map(|i| i.message)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let full = StateAddress::from_args_json(&addr.provider, &addr.id, &normalized)
        .map_err(SetError::BadArgs)?;
    Ok(full.key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::params::ParamDef;
    use crate::state::definition::Role;

    fn level() -> ObservableDefinition {
        ObservableDefinition::state("example.counter", "level", "Level", "").one_of(&[
            ("zero", "Empty", Some(Role::Inactive)),
            ("low", "Low", None),
            ("high", "High", None),
        ])
    }

    fn slot_status() -> ObservableDefinition {
        ObservableDefinition::state("agent-control", "slot.status", "Track", "")
            .param(
                ParamDef::number("slot", "Track")
                    .range(0.0, 7.0)
                    .integer()
                    .required(),
            )
            .one_of(&[("idle", "Idle", None), ("running", "Running", None)])
    }

    fn counter() -> (StateHub, StateAddress) {
        let hub = StateHub::new();
        hub.declare("example.counter", vec![level()]).unwrap();
        hub.provider_up("example.counter");
        (hub, StateAddress::new("example.counter", "level"))
    }

    #[test]
    fn a_state_is_unknown_until_its_provider_reports() {
        let (hub, addr) = counter();
        assert_eq!(hub.read_address(&addr), Observed::unknown());
        let out = hub.set(&addr, Scalar::text("high")).unwrap();
        assert_eq!(
            out,
            SetOutcome {
                changed: true,
                unlisted: false
            }
        );
        let o = hub.read_address(&addr);
        assert_eq!(o.status, Status::Known);
        assert_eq!(o.value, Some(Scalar::text("high")));
        assert!(o.updated_at.is_some());
    }

    #[test]
    fn what_is_not_declared_or_not_running_is_unavailable() {
        let hub = StateHub::new();
        let addr = StateAddress::new("example.counter", "level");
        assert_eq!(
            hub.read_address(&addr),
            Observed::unavailable(),
            "no provider"
        );
        hub.declare("example.counter", vec![level()]).unwrap();
        assert_eq!(
            hub.read_address(&addr),
            Observed::unavailable(),
            "declared, not started"
        );
        hub.provider_up("example.counter");
        assert_eq!(hub.read_address(&addr).status, Status::Unknown);
        assert_eq!(
            hub.read_address(&StateAddress::new("example.counter", "missing")),
            Observed::unavailable(),
            "a state the provider does not declare"
        );
    }

    #[test]
    fn a_provider_that_stops_and_returns_never_gets_its_old_value_back() {
        let (hub, addr) = counter();
        hub.set(&addr, Scalar::text("high")).unwrap();
        hub.provider_down("example.counter");
        assert_eq!(hub.read_address(&addr), Observed::unavailable());
        assert_eq!(
            hub.set(&addr, Scalar::text("low")),
            Err(SetError::ProviderDown("example.counter".into())),
            "a stopped provider cannot publish"
        );
        hub.provider_up("example.counter");
        assert_eq!(hub.read_address(&addr), Observed::unknown());
        hub.set(&addr, Scalar::text("low")).unwrap();
        assert_eq!(hub.read_address(&addr).value, Some(Scalar::text("low")));
    }

    #[test]
    fn a_restart_without_a_stop_also_forgets() {
        let (hub, addr) = counter();
        hub.set(&addr, Scalar::text("high")).unwrap();
        hub.provider_up("example.counter");
        assert_eq!(hub.read_address(&addr), Observed::unknown());
    }

    #[test]
    fn removing_a_provider_removes_everything_of_it() {
        let (hub, addr) = counter();
        hub.set(&addr, Scalar::text("high")).unwrap();
        hub.remove_provider("example.counter");
        assert_eq!(hub.read_address(&addr), Observed::unavailable());
        assert!(hub.definitions().is_empty());
        assert!(hub.provider_is_up("example.counter").is_none());
    }

    #[test]
    fn instances_of_one_definition_are_independent() {
        let hub = StateHub::new();
        hub.ensure_provider("agent-control", vec![slot_status()])
            .unwrap();
        let s = |n: i64| StateAddress::new("agent-control", "slot.status").with("slot", n);
        hub.set(&s(2), Scalar::text("running")).unwrap();
        hub.set(&s(3), Scalar::text("idle")).unwrap();
        assert_eq!(hub.read_address(&s(2)).value, Some(Scalar::text("running")));
        assert_eq!(hub.read_address(&s(3)).value, Some(Scalar::text("idle")));
        assert_eq!(hub.read_address(&s(4)).status, Status::Unknown);
        // 2 and 2.0 are the same instance.
        let float = StateAddress::new("agent-control", "slot.status").with("slot", 2.0);
        assert_eq!(
            hub.read_address(&float).value,
            Some(Scalar::text("running"))
        );
        assert!(hub.clear(&s(2)));
        assert_eq!(hub.read_address(&s(2)).status, Status::Unknown);
        assert_eq!(hub.read_address(&s(3)).status, Status::Known);
    }

    #[test]
    fn arguments_are_checked_against_the_definition() {
        let hub = StateHub::new();
        hub.ensure_provider("agent-control", vec![slot_status()])
            .unwrap();
        let w = |a: StateAddress| hub.set(&a, Scalar::text("idle")).unwrap_err();
        let base = || StateAddress::new("agent-control", "slot.status");
        assert!(
            matches!(w(base()), SetError::BadArgs(_)),
            "required argument missing"
        );
        assert!(
            matches!(w(base().with("slot", 9i64)), SetError::BadArgs(_)),
            "out of range"
        );
        assert!(
            matches!(w(base().with("slot", 1.5)), SetError::BadArgs(_)),
            "not whole"
        );
        assert!(matches!(
            w(base().with("slot", 1i64).with("extra", true)),
            SetError::BadArgs(_)
        ));
        let (hub2, plain) = counter();
        assert!(matches!(
            hub2.set(&plain.clone().with("x", 1i64), Scalar::text("low")),
            Err(SetError::BadArgs(_))
        ));
    }

    #[test]
    fn values_are_checked_against_the_declared_type() {
        let (hub, addr) = counter();
        assert!(matches!(
            hub.set(&addr, Scalar::Num(3.0)),
            Err(SetError::WrongType { .. })
        ));
        // An enum value the definition does not list is stored and flagged.
        let out = hub.set(&addr, Scalar::text("buffering")).unwrap();
        assert!(out.unlisted && out.changed);
        assert_eq!(
            hub.read_address(&addr).value,
            Some(Scalar::text("buffering"))
        );
        assert!(matches!(
            hub.set(
                &StateAddress::new("example.counter", "nope"),
                Scalar::text("x")
            ),
            Err(SetError::Undeclared { .. })
        ));
        assert!(matches!(
            hub.set(&StateAddress::new("ghost.provider", "x"), Scalar::text("x")),
            Err(SetError::NoProvider(_))
        ));
    }

    #[test]
    fn declarations_are_all_or_nothing_and_drop_stale_values() {
        let (hub, addr) = counter();
        hub.set(&addr, Scalar::text("high")).unwrap();
        let bad = ObservableDefinition::state("example.counter", "Bad Id", "X", "").number();
        assert!(hub.declare("example.counter", vec![level(), bad]).is_err());
        assert_eq!(
            hub.read_address(&addr).status,
            Status::Known,
            "nothing changed"
        );
        let other = ObservableDefinition::state("someone.else", "x", "X", "").number();
        assert!(hub.declare("example.counter", vec![other]).is_err());
        let event = ObservableDefinition::event("example.counter", "e", "E", "");
        assert!(hub.declare("example.counter", vec![event]).is_err());
        // A new declaration without `level` drops its value and makes it unavailable.
        let count = ObservableDefinition::state("example.counter", "count", "Count", "").number();
        hub.declare("example.counter", vec![count]).unwrap();
        assert_eq!(hub.read_address(&addr), Observed::unavailable());
        // Declaring the same list again is not a change.
        hub.drain();
        let count = ObservableDefinition::state("example.counter", "count", "Count", "").number();
        hub.declare("example.counter", vec![count]).unwrap();
        assert!(hub.drain().is_empty());
    }

    #[test]
    fn only_real_changes_are_dirty_and_a_burst_is_one_entry() {
        let (hub, addr) = counter();
        hub.drain();
        hub.set(&addr, Scalar::text("low")).unwrap();
        hub.set(&addr, Scalar::text("high")).unwrap();
        hub.set(&addr, Scalar::text("high")).unwrap();
        let c = hub.drain();
        assert_eq!(c.keys.len(), 1);
        assert!(c.providers.is_empty());
        assert!(hub.drain().is_empty(), "drain empties the set");
        // Writing the value it already has is not news.
        hub.set(&addr, Scalar::text("high")).unwrap();
        assert!(hub.drain().is_empty());
    }

    #[test]
    fn lifecycle_changes_mark_the_provider_not_each_key() {
        let (hub, _) = counter();
        hub.drain();
        hub.provider_down("example.counter");
        let c = hub.drain();
        assert!(c.providers.contains("example.counter") && c.keys.is_empty());
        hub.provider_up("example.counter");
        assert!(hub.drain().providers.contains("example.counter"));
        // Down for something that was never registered is silent.
        hub.provider_down("ghost.provider");
        assert!(hub.drain().is_empty());
    }

    #[test]
    fn ensure_keeps_what_an_always_on_provider_reported() {
        let hub = StateHub::new();
        hub.ensure_provider("example.counter", vec![level()])
            .unwrap();
        let addr = StateAddress::new("example.counter", "level");
        hub.set(&addr, Scalar::text("low")).unwrap();
        hub.ensure_provider("example.counter", vec![level()])
            .unwrap();
        assert_eq!(hub.read_address(&addr).value, Some(Scalar::text("low")));
    }

    #[test]
    fn a_provider_cannot_hold_unbounded_instances() {
        let hub = StateHub::new();
        let def = ObservableDefinition::state("big.one", "session", "Session", "")
            .param(ParamDef::string("id", "Id").required())
            .text();
        hub.ensure_provider("big.one", vec![def]).unwrap();
        for n in 0..MAX_INSTANCES_PER_PROVIDER {
            let a = StateAddress::new("big.one", "session").with("id", format!("s{n}").as_str());
            hub.set(&a, Scalar::text("x")).unwrap();
        }
        let one_more = StateAddress::new("big.one", "session").with("id", "extra");
        assert!(matches!(
            hub.set(&one_more, Scalar::text("x")),
            Err(SetError::TooMany(_))
        ));
        // Updating one that exists is still fine.
        let first = StateAddress::new("big.one", "session").with("id", "s0");
        assert!(hub.set(&first, Scalar::text("y")).is_ok());
    }

    #[tokio::test]
    async fn a_write_before_the_wait_is_not_lost() {
        let (hub, addr) = counter();
        hub.drain();
        hub.set(&addr, Scalar::text("low")).unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(200), hub.changed())
            .await
            .expect("the wake-up was remembered");
        assert_eq!(hub.drain().keys.len(), 1);
    }

    #[test]
    fn many_threads_write_and_read_without_losing_or_deadlocking() {
        let hub = StateHub::new();
        hub.ensure_provider("agent-control", vec![slot_status()])
            .unwrap();
        let writers: Vec<_> = (0..8)
            .map(|t| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    let a =
                        StateAddress::new("agent-control", "slot.status").with("slot", t as i64);
                    for n in 0..2000 {
                        let v = if n % 2 == 0 { "running" } else { "idle" };
                        hub.set(&a, Scalar::text(v)).unwrap();
                        let _ = hub.read_address(&a);
                    }
                })
            })
            .collect();
        let reader = {
            let hub = hub.clone();
            std::thread::spawn(move || {
                let mut seen = 0;
                for _ in 0..2000 {
                    seen += hub.drain().keys.len();
                    let _ = hub.known();
                    let _ = hub.definitions();
                }
                seen
            })
        };
        for w in writers {
            w.join().unwrap();
        }
        reader.join().unwrap();
        // 2000 writes alternate, so the last one wrote "idle" for every slot.
        for t in 0..8i64 {
            let a = StateAddress::new("agent-control", "slot.status").with("slot", t);
            assert_eq!(hub.read_address(&a).value, Some(Scalar::text("idle")));
        }
    }
}
