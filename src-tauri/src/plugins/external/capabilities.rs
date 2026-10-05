//! Capabilities: what a plugin asks for, what the person granted, and whether
//! one operation is covered.
//!
//! The sandbox has no way out, so a capability is always something LaunchDeck
//! does on a plugin's behalf. Three things are kept apart on purpose:
//!
//! * the **request**, in the plugin's manifest (written by its author);
//! * the **grant**, in `permissions.json` (written by the person, never by a
//!   plugin, never by a new version of one);
//! * the **use**, checked against both on every call.
//!
//! Only `network` exists. A second capability is a second request type and a
//! second map in [`PluginGrants`]; nothing here is built ahead of that.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::{Host, Url};

use crate::audit::{Actor, Audit, AuditEntry};
use crate::events::now_ms;
use crate::persistence::write_private;

/// Origins one plugin may ask for.
pub const MAX_HOSTS: usize = 16;
const GRANTS_SCHEMA: u32 = 1;

// ---------------------------------------------------------------------------
// Origins and how far they reach
// ---------------------------------------------------------------------------

/// How far a destination is from this computer. Shown in the permission
/// prompt, and enforced when a name is resolved: an origin may only resolve to
/// addresses within its own reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    /// `localhost` and loopback addresses: this computer.
    Loopback,
    /// Private, link-local and unique-local addresses, `.local` and bare names.
    Local,
    /// Everything else.
    Internet,
}

/// Where an address is, or `None` for an address nothing may connect to
/// (unspecified, broadcast, multicast, reserved).
pub fn reach_of_ip(ip: IpAddr) -> Option<Reach> {
    match ip {
        IpAddr::V4(v4) => reach_of_v4(v4),
        IpAddr::V6(v6) => {
            // `::ffff:127.0.0.1` is 127.0.0.1 spelled differently.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return reach_of_v4(v4);
            }
            let seg = v6.segments();
            if v6 == Ipv6Addr::LOCALHOST {
                Some(Reach::Loopback)
            } else if v6.is_unspecified() || v6.is_multicast() {
                None
            } else if seg[0] & 0xfe00 == 0xfc00 || seg[0] & 0xffc0 == 0xfe80 {
                // fc00::/7 unique local, fe80::/10 link-local.
                Some(Reach::Local)
            } else {
                Some(Reach::Internet)
            }
        }
    }
}

fn reach_of_v4(ip: Ipv4Addr) -> Option<Reach> {
    let o = ip.octets();
    if ip.is_loopback() {
        Some(Reach::Loopback)
    } else if o[0] == 0 || ip.is_broadcast() || ip.is_multicast() || o[0] >= 240 {
        None
    } else if ip.is_private() || ip.is_link_local() || (o[0] == 100 && o[1] & 0xc0 == 64) {
        // 10/8, 172.16/12, 192.168/16, 169.254/16, and 100.64/10 (carrier NAT).
        Some(Reach::Local)
    } else {
        Some(Reach::Internet)
    }
}

/// Name endings that mean "inside this network" and are never public names.
const LOCAL_SUFFIXES: &[&str] = &[".local", ".lan", ".internal", ".home.arpa"];

fn reach_of_name(name: &str) -> Reach {
    if name == "localhost" || name.ends_with(".localhost") {
        Reach::Loopback
    } else if !name.contains('.') || LOCAL_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        Reach::Local
    } else {
        Reach::Internet
    }
}

/// Whether an address a name resolved to is inside what the origin may reach.
/// A local origin may be this computer; an internet origin may be neither.
pub fn reach_allows(origin: Reach, address: Reach) -> bool {
    match origin {
        Reach::Loopback => address == Reach::Loopback,
        Reach::Local => address != Reach::Internet,
        Reach::Internet => address == Reach::Internet,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OriginError {
    #[error("`{0}` is not a URL")]
    NotAUrl(String),
    #[error("only http and https are allowed, not `{0}`")]
    Scheme(String),
    #[error("a user name or password in the URL is not allowed")]
    Credentials,
    #[error("the URL has no host")]
    NoHost,
    #[error("`{0}` is not a host name LaunchDeck accepts")]
    BadHost(String),
    #[error("nothing may connect to `{0}`")]
    Unroutable(String),
    #[error(
        "write only the scheme, host and port, for example https://api.example.com (found `{0}`)"
    )]
    NotBare(String),
}

/// One destination: scheme, host and port, normalized. Two origins are the
/// same exactly when their text is the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Origin {
    /// `https://api.example.com`, `http://192.168.1.20:8123`. The default port
    /// of the scheme is left out; hosts are lower-case, IDNs are punycode.
    text: String,
    reach: Reach,
}

impl Origin {
    /// The origin of any URL (its path and query are ignored).
    pub fn of_url(url: &Url) -> Result<Origin, OriginError> {
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(OriginError::Scheme(scheme.to_string()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(OriginError::Credentials);
        }
        let host = url.host().ok_or(OriginError::NoHost)?;
        let (name, reach) = match host {
            Host::Domain(d) => {
                let ok = !d.is_empty()
                    && d.len() <= 253
                    && !d.ends_with('.')
                    && d.split('.').all(|label| {
                        !label.is_empty()
                            && label.len() <= 63
                            && !label.starts_with('-')
                            && !label.ends_with('-')
                            && label
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    });
                if !ok {
                    return Err(OriginError::BadHost(d.to_string()));
                }
                (d.to_string(), reach_of_name(d))
            }
            Host::Ipv4(ip) => (
                ip.to_string(),
                reach_of_ip(IpAddr::V4(ip))
                    .ok_or_else(|| OriginError::Unroutable(ip.to_string()))?,
            ),
            Host::Ipv6(ip) => (
                format!("[{ip}]"),
                reach_of_ip(IpAddr::V6(ip))
                    .ok_or_else(|| OriginError::Unroutable(ip.to_string()))?,
            ),
        };
        // `port()` is `None` for the scheme's default, which is how it stays out of the text.
        let text = match url.port() {
            Some(port) => format!("{scheme}://{name}:{port}"),
            None => format!("{scheme}://{name}"),
        };
        Ok(Origin { text, reach })
    }

    /// The origin of a URL written by a person or a plugin.
    pub fn of(text: &str) -> Result<Origin, OriginError> {
        let url = Url::parse(text.trim()).map_err(|_| OriginError::NotAUrl(short(text)))?;
        Origin::of_url(&url)
    }

    /// An origin as a manifest declares it: scheme, host and port and nothing
    /// more, so what is asked for is exactly what is shown.
    pub fn declared(text: &str) -> Result<Origin, OriginError> {
        if text.contains('*') {
            return Err(OriginError::BadHost(short(text)));
        }
        let url = Url::parse(text).map_err(|_| OriginError::NotAUrl(short(text)))?;
        let origin = Origin::of_url(&url)?;
        let bare =
            matches!(url.path(), "" | "/") && url.query().is_none() && url.fragment().is_none();
        if !bare {
            return Err(OriginError::NotBare(short(text)));
        }
        Ok(origin)
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn reach(&self) -> Reach {
        self.reach
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn short(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

// ---------------------------------------------------------------------------
// What a manifest asks for
// ---------------------------------------------------------------------------

/// One host of a manifest: written out, or "wherever this setting points".
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum HostSpec {
    Origin(String),
    Setting {
        /// Key of a text setting of this plugin that holds a URL.
        setting: String,
    },
}

/// The `network` capability as a manifest declares it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkDecl {
    /// Why the plugin needs it, in a sentence the person reads before deciding.
    pub purpose: String,
    pub hosts: Vec<HostSpec>,
}

/// The `capabilities` of a manifest. Schemas 1 to 3 wrote an array that had to
/// be empty; schema 4 writes an object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub network: Option<NetworkDecl>,
    /// Names from an old-style array, kept only to refuse them by name.
    pub legacy: Vec<String>,
    /// True when the manifest wrote an object (schema 4 syntax).
    pub object: bool,
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Object {
            #[serde(default)]
            network: Option<NetworkDecl>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            List(Vec<String>),
            Object(Object),
        }
        Ok(match Either::deserialize(d)? {
            Either::List(legacy) => Capabilities {
                network: None,
                legacy,
                object: false,
            },
            Either::Object(o) => Capabilities {
                network: o.network,
                legacy: vec![],
                object: true,
            },
        })
    }
}

/// Where a requested origin comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HostSource {
    /// Written in the manifest.
    Declared,
    /// The origin of the URL in this setting.
    Setting { key: String },
}

/// One destination a plugin asks for now, with its settings as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedHost {
    pub origin: Origin,
    pub source: HostSource,
}

/// A host the manifest names through a setting that does not give an origin yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedHost {
    pub setting: String,
    /// Nothing is set, or why what is set is not usable.
    pub reason: String,
}

/// What a plugin's manifest and settings ask for, resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkRequest {
    pub purpose: String,
    pub hosts: Vec<RequestedHost>,
    pub unresolved: Vec<UnresolvedHost>,
}

impl NetworkRequest {
    /// Turn the declaration into origins, reading setting-based hosts from the
    /// configuration the plugin runs with.
    pub fn resolve(decl: &NetworkDecl, config: &Map<String, Value>) -> NetworkRequest {
        let mut out = NetworkRequest {
            purpose: decl.purpose.clone(),
            ..NetworkRequest::default()
        };
        let mut seen = HashSet::new();
        for spec in &decl.hosts {
            let (origin, source) = match spec {
                HostSpec::Origin(text) => match Origin::declared(text) {
                    Ok(o) => (o, HostSource::Declared),
                    // Validated when the manifest was read; never reached.
                    Err(_) => continue,
                },
                HostSpec::Setting { setting } => {
                    let value = config
                        .get(setting)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if value.is_empty() {
                        out.unresolved.push(UnresolvedHost {
                            setting: setting.clone(),
                            reason: "Not set yet.".into(),
                        });
                        continue;
                    }
                    match Origin::of(value) {
                        Ok(o) => (
                            o,
                            HostSource::Setting {
                                key: setting.clone(),
                            },
                        ),
                        Err(e) => {
                            out.unresolved.push(UnresolvedHost {
                                setting: setting.clone(),
                                reason: format!("{e}."),
                            });
                            continue;
                        }
                    }
                }
            };
            if seen.insert(origin.clone()) {
                out.hosts.push(RequestedHost { origin, source });
            }
        }
        out
    }

    pub fn requests(&self, origin: &Origin) -> bool {
        self.hosts.iter().any(|h| &h.origin == origin)
    }

    /// The origin a setting-based or written-out host stands for now.
    pub fn origin_of(&self, spec: &HostSpec) -> Option<&Origin> {
        self.hosts
            .iter()
            .map(|h| (&h.origin, &h.source))
            .find_map(|(o, s)| match (spec, s) {
                (HostSpec::Setting { setting }, HostSource::Setting { key }) if setting == key => {
                    Some(o)
                }
                (HostSpec::Origin(text), HostSource::Declared)
                    if Origin::declared(text).ok().as_ref() == Some(o) =>
                {
                    Some(o)
                }
                _ => None,
            })
    }
}

// ---------------------------------------------------------------------------
// What the person decided
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Granted,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    decision: Decision,
    /// When it was decided, milliseconds since the epoch.
    at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginGrants {
    /// Origin text to the decision about it.
    #[serde(default)]
    network: BTreeMap<String, Record>,
    /// Origins already recorded as asked for, so the audit trail says it once.
    #[serde(default)]
    requested: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedGrants {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    plugins: BTreeMap<String, PluginGrants>,
}

/// Where a requested origin stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GrantStatus {
    Granted,
    Denied,
    NeedsApproval,
}

/// One origin in the Plugins screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    pub origin: String,
    pub reach: Reach,
    pub status: GrantStatus,
    pub source: HostSource,
    /// When it was granted or denied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<u64>,
}

/// A decision on file for an origin the plugin no longer asks for. It covers nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleGrant {
    pub origin: String,
    pub decision: Decision,
}

/// The network permission of one plugin, for the Plugins screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    pub purpose: String,
    /// `granted` when every requested origin is, `needsApproval` when any waits
    /// for a decision, `denied` otherwise.
    pub status: GrantStatus,
    pub hosts: Vec<HostView>,
    pub unresolved: Vec<UnresolvedHost>,
    pub stale: Vec<StaleGrant>,
}

struct GrantsInner {
    saved: SavedGrants,
    audit: Audit,
}

/// Grants for every plugin. Has its own lock, never held while a plugin runs,
/// so the broker can ask during a call.
pub struct Grants {
    path: Option<PathBuf>,
    inner: Mutex<GrantsInner>,
}

fn load_grants(path: &Path) -> SavedGrants {
    let Ok(text) = std::fs::read_to_string(path) else {
        return SavedGrants::default();
    };
    match serde_json::from_str::<SavedGrants>(&text) {
        Ok(s) if s.schema_version <= GRANTS_SCHEMA => s,
        _ => {
            // Damaged, or written by a newer LaunchDeck. Fail closed: nothing is
            // granted, and the file is kept aside for the person.
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            let _ = std::fs::copy(path, PathBuf::from(bak));
            log::warn!("permissions.json could not be used; no plugin has any grant");
            SavedGrants::default()
        }
    }
}

impl Grants {
    /// `path = None` keeps grants in memory (tests). `audit_path` is the audit
    /// trail permission changes are written to.
    pub fn new(path: Option<PathBuf>, audit_path: Option<PathBuf>) -> Grants {
        Grants {
            inner: Mutex::new(GrantsInner {
                saved: path.as_deref().map(load_grants).unwrap_or_default(),
                audit: Audit::new(audit_path),
            }),
            path,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GrantsInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, saved: &SavedGrants) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut out = saved.clone();
        out.schema_version = GRANTS_SCHEMA;
        let bytes = serde_json::to_vec_pretty(&out).map_err(|e| e.to_string())?;
        write_private(path, &bytes).map_err(|e| format!("could not save permissions: {e}"))
    }

    /// The one question a use asks: has the person granted this origin to this
    /// plugin? Whether the plugin still requests it is the caller's half.
    pub fn decision(&self, plugin: &str, origin: &Origin) -> Option<Decision> {
        self.lock()
            .saved
            .plugins
            .get(plugin)
            .and_then(|p| p.network.get(origin.as_str()))
            .map(|r| r.decision)
    }

    /// Grant, deny, or (with `None`) take the decision back.
    pub fn decide(
        &self,
        plugin: &str,
        origin: &Origin,
        decision: Option<Decision>,
    ) -> Result<(), String> {
        let mut inner = self.lock();
        let mut saved = inner.saved.clone();
        let entry = saved.plugins.entry(plugin.to_string()).or_default();
        let before = entry.network.get(origin.as_str()).map(|r| r.decision);
        if before == decision {
            return Ok(());
        }
        match decision {
            Some(decision) => {
                entry.network.insert(
                    origin.as_str().to_string(),
                    Record {
                        decision,
                        at: now_ms(),
                    },
                );
            }
            None => {
                entry.network.remove(origin.as_str());
            }
        }
        self.persist(&saved)?;
        inner.saved = saved;
        let kind = match decision {
            Some(Decision::Granted) => "permission.granted",
            Some(Decision::Denied) => "permission.denied",
            None => "permission.revoked",
        };
        inner.audit.record(
            AuditEntry::new(kind, Actor::User)
                .plugin(plugin)
                .detail(&format!("network {origin}")),
        );
        Ok(())
    }

    /// Record, once per origin, that a plugin asked for it.
    pub fn note_requested(&self, plugin: &str, request: &NetworkRequest) {
        let mut inner = self.lock();
        let known = inner
            .saved
            .plugins
            .get(plugin)
            .map(|p| p.requested.clone())
            .unwrap_or_default();
        let new: Vec<&Origin> = request
            .hosts
            .iter()
            .map(|h| &h.origin)
            .filter(|o| !known.contains(o.as_str()))
            .collect();
        if new.is_empty() {
            return;
        }
        let mut saved = inner.saved.clone();
        let entry = saved.plugins.entry(plugin.to_string()).or_default();
        for origin in &new {
            entry.requested.insert(origin.as_str().to_string());
        }
        if let Err(e) = self.persist(&saved) {
            log::warn!("{e}");
        }
        inner.saved = saved;
        for origin in new {
            inner.audit.record(
                AuditEntry::new("permission.requested", Actor::System)
                    .plugin(plugin)
                    .detail(&format!("network {origin}")),
            );
        }
    }

    /// What the Plugins screen shows about one plugin's network permission.
    pub fn view(&self, plugin: &str, request: &NetworkRequest) -> NetworkView {
        let inner = self.lock();
        let records = inner.saved.plugins.get(plugin).map(|p| &p.network);
        let hosts: Vec<HostView> = request
            .hosts
            .iter()
            .map(|h| {
                let record = records.and_then(|r| r.get(h.origin.as_str()));
                HostView {
                    origin: h.origin.as_str().to_string(),
                    reach: h.origin.reach(),
                    status: match record.map(|r| r.decision) {
                        Some(Decision::Granted) => GrantStatus::Granted,
                        Some(Decision::Denied) => GrantStatus::Denied,
                        None => GrantStatus::NeedsApproval,
                    },
                    source: h.source.clone(),
                    decided_at: record.map(|r| r.at),
                }
            })
            .collect();
        let status = if hosts.iter().any(|h| h.status == GrantStatus::NeedsApproval) {
            GrantStatus::NeedsApproval
        } else if hosts.iter().any(|h| h.status == GrantStatus::Denied) {
            GrantStatus::Denied
        } else if hosts.is_empty() {
            // Nothing concrete is asked for yet (a URL setting is empty).
            GrantStatus::NeedsApproval
        } else {
            GrantStatus::Granted
        };
        let stale = records
            .map(|r| {
                r.iter()
                    .filter(|(origin, _)| {
                        !request
                            .hosts
                            .iter()
                            .any(|h| h.origin.as_str() == origin.as_str())
                    })
                    .map(|(origin, record)| StaleGrant {
                        origin: origin.clone(),
                        decision: record.decision,
                    })
                    .collect()
            })
            .unwrap_or_default();
        NetworkView {
            purpose: request.purpose.clone(),
            status,
            hosts,
            unresolved: request.unresolved.clone(),
            stale,
        }
    }

    /// Remove a decision by its text, for an origin the plugin no longer asks
    /// for (it may no longer parse as one the current rules accept).
    pub fn forget(&self, plugin: &str, origin: &str) -> Result<(), String> {
        let mut inner = self.lock();
        let mut saved = inner.saved.clone();
        let removed = saved
            .plugins
            .get_mut(plugin)
            .and_then(|p| p.network.remove(origin));
        if removed.is_none() {
            return Ok(());
        }
        self.persist(&saved)?;
        inner.saved = saved;
        inner.audit.record(
            AuditEntry::new("permission.revoked", Actor::User)
                .plugin(plugin)
                .detail(&format!("network {origin}")),
        );
        Ok(())
    }

    /// Add a security event about plugins that is not a permission decision
    /// (plain secrets moved into the credential store, for example).
    pub fn record(&self, entry: AuditEntry) {
        self.lock().audit.record(entry);
    }

    /// Security events recorded since this process started, newest last.
    pub fn recent_audit(&self, limit: usize) -> Vec<AuditEntry> {
        self.lock().audit.recent(limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn origin(s: &str) -> Origin {
        Origin::of(s).unwrap()
    }

    #[test]
    fn origins_are_normalized_so_spelling_cannot_make_two_of_one() {
        for (written, text) in [
            ("https://API.Example.com/", "https://api.example.com"),
            (
                "https://api.example.com:443/v1?x=1#f",
                "https://api.example.com",
            ),
            ("http://example.com:80", "http://example.com"),
            ("http://example.com:8080/a/b", "http://example.com:8080"),
            ("https://bücher.example", "https://xn--bcher-kva.example"),
            // Every way of writing 127.0.0.1 is 127.0.0.1.
            ("http://2130706433/", "http://127.0.0.1"),
            ("http://0x7f.0.0.1/", "http://127.0.0.1"),
            ("http://0177.0.0.1/", "http://127.0.0.1"),
            ("http://127.1/", "http://127.0.0.1"),
            ("http://[::1]:8123/", "http://[::1]:8123"),
            (
                "  http://homeassistant.local:8123/lovelace  ",
                "http://homeassistant.local:8123",
            ),
        ] {
            assert_eq!(origin(written).as_str(), text, "{written}");
        }
        assert_ne!(
            origin("http://ha.local"),
            origin("https://ha.local"),
            "the scheme is part of it"
        );
        assert_ne!(
            origin("http://ha.local"),
            origin("http://ha.local:8123"),
            "and so is the port"
        );
    }

    #[test]
    fn urls_that_are_not_plain_http_destinations_are_refused() {
        assert_eq!(
            Origin::of("ftp://example.com"),
            Err(OriginError::Scheme("ftp".into()))
        );
        assert_eq!(
            Origin::of("file:///etc/passwd"),
            Err(OriginError::Scheme("file".into()))
        );
        assert_eq!(
            Origin::of("http://user:pw@example.com/"),
            Err(OriginError::Credentials)
        );
        assert_eq!(
            Origin::of("http://evil.com@127.0.0.1/"),
            Err(OriginError::Credentials)
        );
        assert!(matches!(
            Origin::of("example.com"),
            Err(OriginError::NotAUrl(_))
        ));
        assert!(matches!(
            Origin::of("http://"),
            Err(OriginError::NotAUrl(_))
        ));
        assert!(matches!(
            Origin::of("http://example.com./"),
            Err(OriginError::BadHost(_))
        ));
        assert!(matches!(
            Origin::of("http://0.0.0.0/"),
            Err(OriginError::Unroutable(_))
        ));
        assert!(matches!(
            Origin::of("http://224.0.0.1/"),
            Err(OriginError::Unroutable(_))
        ));
        assert!(matches!(
            Origin::of("http://255.255.255.255/"),
            Err(OriginError::Unroutable(_))
        ));
        assert!(matches!(
            Origin::of("http://[::]/"),
            Err(OriginError::Unroutable(_))
        ));
    }

    #[test]
    fn a_manifest_names_an_origin_and_nothing_else() {
        assert!(Origin::declared("https://api.example.com").is_ok());
        assert!(Origin::declared("http://192.168.1.20:8123/").is_ok());
        for bad in [
            "https://api.example.com/v1",
            "https://api.example.com/?a=1",
            "https://api.example.com/#x",
            "https://*.example.com",
            "*",
            "api.example.com",
            "https://",
        ] {
            assert!(Origin::declared(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_origin_has_a_reach() {
        for (text, reach) in [
            ("http://localhost:8123", Reach::Loopback),
            ("http://app.localhost", Reach::Loopback),
            ("http://127.0.0.1", Reach::Loopback),
            ("http://127.8.9.10", Reach::Loopback),
            ("http://[::1]", Reach::Loopback),
            ("http://[::ffff:127.0.0.1]", Reach::Loopback),
            ("http://homeassistant.local:8123", Reach::Local),
            ("http://homeassistant", Reach::Local),
            ("http://nas.lan", Reach::Local),
            ("http://router.home.arpa", Reach::Local),
            ("http://192.168.1.20", Reach::Local),
            ("http://10.0.0.5", Reach::Local),
            ("http://172.16.0.1", Reach::Local),
            ("http://172.31.255.255", Reach::Local),
            ("http://169.254.169.254", Reach::Local),
            ("http://100.64.0.1", Reach::Local),
            ("http://[fd00::1]", Reach::Local),
            ("http://[fe80::1]", Reach::Local),
            ("http://[::ffff:192.168.1.1]", Reach::Local),
            ("https://api.example.com", Reach::Internet),
            ("http://172.32.0.1", Reach::Internet),
            ("http://8.8.8.8", Reach::Internet),
            ("http://[2001:4860:4860::8888]", Reach::Internet),
        ] {
            assert_eq!(origin(text).reach(), reach, "{text}");
        }
    }

    #[test]
    fn a_name_may_only_resolve_within_its_reach() {
        use Reach::*;
        // An internet name that suddenly points inside is DNS rebinding.
        assert!(reach_allows(Internet, Internet));
        assert!(!reach_allows(Internet, Local));
        assert!(!reach_allows(Internet, Loopback));
        // A local name may be this computer, and may not be a public address.
        assert!(reach_allows(Local, Local));
        assert!(reach_allows(Local, Loopback));
        assert!(!reach_allows(Local, Internet));
        assert!(reach_allows(Loopback, Loopback));
        assert!(!reach_allows(Loopback, Local));
        assert!(!reach_allows(Loopback, Internet));
    }

    fn decl(hosts: Value) -> NetworkDecl {
        serde_json::from_value(json!({ "purpose": "Testing.", "hosts": hosts })).unwrap()
    }

    #[test]
    fn a_request_is_the_declared_origins_plus_what_settings_point_at() {
        let d = decl(
            json!(["https://api.example.com", { "setting": "baseUrl" }, { "setting": "other" }]),
        );
        let mut config = Map::new();
        let none = NetworkRequest::resolve(&d, &config);
        assert_eq!(none.hosts.len(), 1);
        assert_eq!(none.unresolved.len(), 2);
        assert_eq!(none.unresolved[0].reason, "Not set yet.");

        config.insert("baseUrl".into(), json!("http://homeassistant.local:8123/"));
        config.insert("other".into(), json!("not a url"));
        let r = NetworkRequest::resolve(&d, &config);
        assert_eq!(
            r.hosts
                .iter()
                .map(|h| h.origin.as_str())
                .collect::<Vec<_>>(),
            vec!["https://api.example.com", "http://homeassistant.local:8123"]
        );
        assert_eq!(
            r.hosts[1].source,
            HostSource::Setting {
                key: "baseUrl".into()
            }
        );
        assert_eq!(r.unresolved.len(), 1);
        assert!(
            r.unresolved[0].reason.contains("not a URL"),
            "{}",
            r.unresolved[0].reason
        );
        assert!(r.requests(&origin("http://homeassistant.local:8123")));
        assert!(!r.requests(&origin("http://homeassistant.local")));
        // The same origin twice is one request.
        config.insert("other".into(), json!("https://api.example.com/path"));
        assert_eq!(NetworkRequest::resolve(&d, &config).hosts.len(), 2);
    }

    #[test]
    fn nothing_is_granted_until_the_person_says_so_and_decisions_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let audit = dir.path().join("audit.jsonl");
        let ha = origin("http://homeassistant.local:8123");
        let request = NetworkRequest::resolve(
            &decl(json!(["http://homeassistant.local:8123"])),
            &Map::new(),
        );

        let grants = Grants::new(Some(path.clone()), Some(audit.clone()));
        assert_eq!(grants.decision("a.b", &ha), None);
        let view = grants.view("a.b", &request);
        assert_eq!(
            (view.status, view.hosts[0].status),
            (GrantStatus::NeedsApproval, GrantStatus::NeedsApproval)
        );
        assert_eq!(view.hosts[0].reach, Reach::Local);

        grants.decide("a.b", &ha, Some(Decision::Granted)).unwrap();
        assert_eq!(grants.decision("a.b", &ha), Some(Decision::Granted));
        assert_eq!(
            grants.decision("other.plugin", &ha),
            None,
            "a grant belongs to one plugin"
        );
        assert_eq!(
            grants.decision("a.b", &origin("https://homeassistant.local:8123")),
            None
        );
        assert_eq!(grants.view("a.b", &request).status, GrantStatus::Granted);

        // A new process reads the same decision.
        let again = Grants::new(Some(path.clone()), Some(audit.clone()));
        assert_eq!(again.decision("a.b", &ha), Some(Decision::Granted));
        again.decide("a.b", &ha, Some(Decision::Denied)).unwrap();
        assert_eq!(again.view("a.b", &request).status, GrantStatus::Denied);
        again.decide("a.b", &ha, None).unwrap();
        assert_eq!(again.decision("a.b", &ha), None);
        assert_eq!(
            again.view("a.b", &request).status,
            GrantStatus::NeedsApproval
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "permissions.json is private to the user");
        }
        let trail = std::fs::read_to_string(&audit).unwrap();
        let kinds: Vec<String> = trail
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "permission.granted",
                "permission.denied",
                "permission.revoked"
            ]
        );
        assert!(
            trail.contains("\"pluginId\":\"a.b\"")
                && trail.contains("network http://homeassistant.local:8123")
        );
    }

    #[test]
    fn a_new_origin_needs_its_own_approval_and_old_grants_stay() {
        let grants = Grants::new(None, None);
        let v1 = NetworkRequest::resolve(&decl(json!(["https://a.example"])), &Map::new());
        grants.note_requested("p.q", &v1);
        grants
            .decide("p.q", &origin("https://a.example"), Some(Decision::Granted))
            .unwrap();
        assert_eq!(grants.view("p.q", &v1).status, GrantStatus::Granted);

        // The next version asks for one more.
        let v2 = NetworkRequest::resolve(
            &decl(json!(["https://a.example", "https://b.example"])),
            &Map::new(),
        );
        grants.note_requested("p.q", &v2);
        let view = grants.view("p.q", &v2);
        assert_eq!(view.status, GrantStatus::NeedsApproval);
        assert_eq!(
            view.hosts.iter().map(|h| h.status).collect::<Vec<_>>(),
            vec![GrantStatus::Granted, GrantStatus::NeedsApproval]
        );
        assert_eq!(
            grants.decision("p.q", &origin("https://b.example")),
            None,
            "never granted by itself"
        );

        // And a version that drops one keeps the old decision on file, covering nothing.
        let v3 = NetworkRequest::resolve(&decl(json!(["https://b.example"])), &Map::new());
        let view = grants.view("p.q", &v3);
        assert_eq!(
            view.stale,
            vec![StaleGrant {
                origin: "https://a.example".into(),
                decision: Decision::Granted
            }]
        );
        grants.forget("p.q", "https://a.example").unwrap();
        assert!(grants.view("p.q", &v3).stale.is_empty());

        // "Requested" is said once per origin.
        grants.note_requested("p.q", &v2);
        let requested: Vec<String> = grants
            .recent_audit(50)
            .into_iter()
            .filter(|e| e.kind == "permission.requested")
            .map(|e| e.detail.unwrap())
            .collect();
        assert_eq!(
            requested,
            vec!["network https://a.example", "network https://b.example"]
        );
    }

    #[test]
    fn a_damaged_or_newer_permissions_file_grants_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        std::fs::write(&path, "{ not json").unwrap();
        let grants = Grants::new(Some(path.clone()), None);
        assert_eq!(grants.decision("a.b", &origin("https://a.example")), None);
        assert!(dir.path().join("permissions.json.bak").exists());
        std::fs::write(
            &path,
            r#"{"schemaVersion": 99, "plugins": {"a.b": {"network": {"https://a.example": {"decision": "granted", "at": 1}}}}}"#,
        )
        .unwrap();
        let grants = Grants::new(Some(path), None);
        assert_eq!(grants.decision("a.b", &origin("https://a.example")), None);
    }

    #[test]
    fn capabilities_read_the_old_array_and_the_new_object() {
        let old: Capabilities = serde_json::from_value(json!([])).unwrap();
        assert!(old.network.is_none() && old.legacy.is_empty() && !old.object);
        let named: Capabilities = serde_json::from_value(json!(["network"])).unwrap();
        assert_eq!(named.legacy, vec!["network"]);
        let new: Capabilities = serde_json::from_value(
            json!({ "network": { "purpose": "x", "hosts": ["https://a.example"] } }),
        )
        .unwrap();
        assert!(new.object && new.network.is_some());
        assert!(serde_json::from_value::<Capabilities>(json!({ "filesystem": {} })).is_err());
        assert!(serde_json::from_value::<Capabilities>(
            json!({ "network": { "purpose": "x", "hosts": [], "extra": 1 } })
        )
        .is_err());
    }
}
