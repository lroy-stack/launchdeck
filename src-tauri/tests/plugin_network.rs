//! The network capability, from inside the sandbox.
//!
//! A real plugin (`tests/fixtures/plugins/net-probe`, built with the public
//! SDK) makes the requests; the real plugin host loads it; the real broker
//! decides; a real server on a loopback port answers and counts what reached
//! it. Nothing is mocked on LaunchDeck's side of the wire.
//!
//! Each test writes the manifest it needs, so one module covers every case.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use launchdeck_lib::events::EventBus;
use launchdeck_lib::plugins::external::capabilities::{Decision, GrantStatus, Grants};
use launchdeck_lib::plugins::external::secrets::{MemoryStore, SecretRef, SecretStore};
use launchdeck_lib::plugins::external::{Limits, PluginHost, PluginStatus};
use launchdeck_lib::plugins::{ActionError, PluginContext, PluginRegistry};
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::{StateAddress, StateHub};
use serde_json::{json, Value};

#[path = "support/http_server.rs"]
#[allow(dead_code)]
mod http_server;
#[path = "support/wat_plugins.rs"]
#[allow(dead_code)]
mod wat;

use http_server::{Reply, TestServer};

const PROBE: &str = "test.probe";

fn probe_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plugins/net-probe/net-probe.wasm");
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (build it: sh scripts/build-example-plugins.sh net-probe)",
            path.display()
        )
    })
}

/// A schema 4 manifest for the probe, asking for `hosts`.
fn manifest(id: &str, version: &str, hosts: Value) -> Value {
    let text = |name: &str, required: bool| json!({ "name": name, "label": name, "type": "string", "required": required, "maxLength": 8000 });
    let number = |name: &str| json!({ "name": name, "label": name, "type": "number" });
    let request = || {
        json!([
            text("method", false),
            text("url", true),
            text("body", false),
            text("secret", false),
            number("timeoutMs"),
            number("count"),
            number("bodySize")
        ])
    };
    json!({
        "schemaVersion": 4, "id": id, "name": "Probe", "version": version, "author": "tests",
        "abi": 1, "sdk": "0.2.0", "main": "net-probe.wasm",
        "capabilities": { "network": { "purpose": "Testing the network capability.", "hosts": hosts } },
        "config": [
            { "key": "target", "type": "string", "label": "Target" },
            { "key": "initUrl", "type": "string" }
        ],
        "actions": [
            { "id": "fetch", "params": request() },
            { "id": "burst", "params": request() },
            { "id": "fetch_then_trap", "params": request() },
            { "id": "fetch_then_work", "params": request() },
            { "id": "init_result" },
            { "id": "fail" }
        ],
        "states": [{ "id": "status", "name": "Last status", "type": "string" }]
    })
}

struct World {
    host: PluginHost,
    registry: PluginRegistry,
    store: Arc<MemoryStore>,
    hub: StateHub,
    bus: EventBus,
    plugins: PathBuf,
    data: PathBuf,
    _root: tempfile::TempDir,
}

fn install(plugins: &Path, folder: &str, manifest: &Value) {
    let dir = plugins.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("net-probe.wasm"), probe_wasm()).unwrap();
}

/// A host with the given plugins installed, each in a folder named after its id.
fn world(manifests: &[Value]) -> World {
    let root = tempfile::tempdir().unwrap();
    let (plugins, data) = (root.path().join("plugins"), root.path().join("data"));
    std::fs::create_dir_all(&data).unwrap();
    for m in manifests {
        install(&plugins, m["id"].as_str().unwrap(), m);
    }
    let grants = Arc::new(Grants::new(
        Some(data.join("permissions.json")),
        Some(data.join("audit.jsonl")),
    ));
    let store = Arc::new(MemoryStore::new());
    let registry = PluginRegistry::default();
    let hub = StateHub::new();
    let host = PluginHost::with_parts(
        vec![plugins.clone()],
        Some(data.join("plugins.json")),
        registry.clone(),
        // The module's own time is short on purpose: a network wait must not count against it.
        Limits {
            fuel: 100_000_000,
            timeout: Duration::from_millis(600),
            memory_bytes: 16 * 1024 * 1024,
        },
        hub.clone(),
        grants.clone(),
        store.clone(),
    );
    host.scan();
    assert!(
        host.issues().is_empty(),
        "a test manifest was refused: {:?}",
        host.issues()
    );
    World {
        host,
        registry,
        store,
        hub,
        bus: EventBus::new(),
        plugins,
        data,
        _root: root,
    }
}

impl World {
    fn invoke_as(&self, id: &str, action: &str, args: Value) -> Result<Value, ActionError> {
        let ctx = PluginContext::new(id, self.bus.clone(), tokio::runtime::Handle::current())
            .with_hub(self.hub.clone());
        self.registry
            .get(id)
            .ok_or_else(|| ActionError::UnknownPlugin(id.into()))?
            .invoke(&ctx, action, &args)
    }

    fn invoke(&self, action: &str, args: Value) -> Result<Value, ActionError> {
        self.invoke_as(PROBE, action, args)
    }

    fn get(&self, url: &str) -> Value {
        self.invoke("fetch", json!({ "url": url }))
            .expect("the action itself succeeds")
    }

    fn grant(&self, origin: &str) {
        self.host
            .set_permission(PROBE, origin, Some(Decision::Granted))
            .unwrap();
    }

    fn status(&self, id: &str) -> PluginStatus {
        self.host.get(id).unwrap().plugin.external.unwrap().status
    }
}

fn hello() -> TestServer {
    TestServer::start(|_| Reply::text(200, "hello"))
}

fn probe(hosts: Value) -> Value {
    manifest(PROBE, "1.0.0", hosts)
}

// ---------------------------------------------------------------------------
// Permission
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_granted_request_leaves_the_sandbox_and_the_answer_comes_back() {
    let server = TestServer::start(|r| {
        Reply::json(
            200,
            &format!(r#"{{"method":"{}","got":"{}"}}"#, r.method, r.body),
        )
    });
    let w = world(&[probe(json!([server.origin()]))]);
    assert_eq!(w.status(PROBE), PluginStatus::Enabled);
    w.grant(&server.origin());

    let r = w
        .invoke(
            "fetch",
            json!({ "method": "POST", "url": server.url("/things"), "body": "payload" }),
        )
        .unwrap();
    assert_eq!(
        (r["ok"].as_bool(), r["status"].as_i64()),
        (Some(true), Some(200))
    );
    assert_eq!(r["body"], r#"{"method":"POST","got":"payload"}"#);
    assert_eq!(r["contentType"], "application/json");
    assert_eq!(server.hits(), 1);
    // What the module reported during the same call arrived too.
    let seen = w.hub.read_address(&StateAddress::new(PROBE, "status"));
    assert_eq!(
        (seen.status, seen.value.map(|v| v.look_name())),
        (Status::Known, Some("200".into()))
    );
    assert!(w.host.get(PROBE).unwrap().network_failure.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn text_survives_the_trip_in_both_directions() {
    // Quotes, backslashes, line breaks, a control character, two-byte and four-byte characters.
    let tricky = "say \"hi\" \\ back\nnew line\ttab \u{1} ñ € 😀 </script>";
    let server = TestServer::start(|r| match r.target.as_str() {
        "/echo" => Reply::text(200, &r.body),
        "/size" => Reply::text(200, &r.body.len().to_string()),
        // 24 KiB of text that needs escaping all the way: within what an action may return.
        _ => Reply::text(200, &"a\"b\n".repeat(6 * 1024)),
    });
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    let r = w
        .invoke(
            "fetch",
            json!({ "method": "POST", "url": server.url("/echo"), "body": tricky }),
        )
        .unwrap();
    assert_eq!(
        server.received()[0].body,
        tricky,
        "what the server received"
    );
    assert_eq!(r["body"], tricky, "what the plugin read back");
    // Large texts cost what they weigh (the SDK's JSON is linear): the largest
    // body a plugin may send, and a long answer full of escapes.
    let r = w
        .invoke(
            "fetch",
            json!({ "method": "POST", "url": server.url("/size"), "bodySize": 256 * 1024 }),
        )
        .unwrap();
    assert_eq!(r["body"], (256 * 1024).to_string());
    let r = w.get(&server.url("/long"));
    assert_eq!(r["body"].as_str().unwrap(), "a\"b\n".repeat(6 * 1024));
}

#[tokio::test(flavor = "multi_thread")]
async fn network_access_is_denied_until_the_person_grants_the_exact_origin() {
    let (asked, other) = (hello(), hello());
    let w = world(&[probe(json!([asked.origin()]))]);

    // Requested, undecided.
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert_eq!(view.status, GrantStatus::NeedsApproval);
    assert_eq!(view.purpose, "Testing the network capability.");
    assert_eq!(view.hosts[0].origin, asked.origin());
    let r = w.get(&asked.url("/"));
    assert_eq!(
        (r["ok"].as_bool(), r["error"].as_str()),
        (Some(false), Some("permission_denied"))
    );
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("waiting for approval"),
        "{r}"
    );

    // Denied.
    w.host
        .set_permission(PROBE, &asked.origin(), Some(Decision::Denied))
        .unwrap();
    assert_eq!(
        w.host.get(PROBE).unwrap().network.unwrap().status,
        GrantStatus::Denied
    );
    assert_eq!(w.get(&asked.url("/"))["error"], "permission_denied");

    // An origin the manifest does not ask for cannot even be granted.
    assert!(w
        .host
        .set_permission(PROBE, &other.origin(), Some(Decision::Granted))
        .is_err());
    assert_eq!(w.get(&other.url("/"))["error"], "host_not_allowed");
    assert_eq!(
        (asked.hits(), other.hits()),
        (0, 0),
        "nothing reached either server"
    );

    // The Plugins screen can say what went wrong.
    let failure = w.host.get(PROBE).unwrap().network_failure.unwrap();
    assert_eq!(
        (failure.code.as_str(), failure.origin.as_deref()),
        ("host_not_allowed", Some(other.origin().as_str()))
    );

    // Granted: it works. Revoked: it stops.
    w.grant(&asked.origin());
    assert_eq!(
        w.host.get(PROBE).unwrap().network.unwrap().status,
        GrantStatus::Granted
    );
    assert_eq!(w.get(&asked.url("/"))["body"], "hello");
    w.host.set_permission(PROBE, &asked.origin(), None).unwrap();
    assert_eq!(w.get(&asked.url("/"))["error"], "permission_denied");
    assert_eq!(asked.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_urls_wrong_schemes_and_other_ports_never_reach_a_server() {
    let (server, other) = (hello(), hello());
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    for (url, error) in [
        ("not a url".to_string(), "invalid_request"),
        ("ftp://127.0.0.1/".to_string(), "invalid_request"),
        ("file:///etc/passwd".to_string(), "invalid_request"),
        (
            format!("http://user:pw@{}/", server.addr),
            "invalid_request",
        ),
        (
            server.url("/").replace("http://", "https://"),
            "host_not_allowed",
        ),
        (other.url("/"), "host_not_allowed"),
        (
            format!("http://localhost:{}/", server.addr.port()),
            "host_not_allowed",
        ),
    ] {
        assert_eq!(w.get(&url)["error"], error, "{url}");
    }
    assert_eq!((server.hits(), other.hits()), (0, 0));
    // The explicit port that was granted is reached.
    assert_eq!(w.get(&server.url("/"))["status"], 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_followed_only_where_a_grant_allows_it() {
    let forbidden = hello();
    let allowed_target = hello();
    let (to_forbidden, to_allowed) = (forbidden.url("/steal"), allowed_target.url("/fine"));
    let first = TestServer::start(move |r| match r.target.as_str() {
        "/out" => Reply::redirect(302, &to_forbidden),
        _ => Reply::redirect(302, &to_allowed),
    });
    let w = world(&[probe(json!([
        first.origin(),
        allowed_target.origin(),
        forbidden.origin()
    ]))]);
    w.grant(&first.origin());
    w.grant(&allowed_target.origin());

    assert_eq!(w.get(&first.url("/in"))["body"], "hello");
    let r = w.get(&first.url("/out"));
    assert_eq!(r["error"], "redirect_not_allowed");
    assert!(
        r["message"].as_str().unwrap().contains(&forbidden.origin()),
        "{r}"
    );
    assert_eq!(
        forbidden.hits(),
        0,
        "the origin that was not granted saw nothing"
    );
}

// ---------------------------------------------------------------------------
// The sandbox boundary
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_module_gets_the_network_functions_only_if_its_manifest_asks() {
    // The same module, with a manifest that asks for nothing.
    let mut quiet = probe(json!(["https://unused.example"]));
    quiet.as_object_mut().unwrap().remove("capabilities");
    let w = world(&[quiet]);
    let info = w.host.get(PROBE).unwrap();
    let external = info.plugin.external.unwrap();
    assert_eq!(external.status, PluginStatus::Error);
    let why = external.error.unwrap();
    assert!(
        why.contains("launchdeck::http_re") && why.contains("does not ask for the network"),
        "{why}"
    );
    assert!(info.network.is_none());
    assert!(w
        .invoke("fetch", json!({ "url": "http://127.0.0.1:9/" }))
        .is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_module_cannot_go_around_the_broker_with_sockets_of_its_own() {
    let server = hello();
    let w = world(&[]);
    // Modules that import what a socket needs, with a manifest that does have the network capability.
    for (i, import) in [
        r#"(import "wasi_snapshot_preview1" "sock_accept" (func (param i32 i32 i32) (result i32)))"#,
        r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#,
        r#"(import "wasi:sockets/tcp@0.2.0" "connect" (func (param i32 i32) (result i32)))"#,
        r#"(import "env" "connect" (func (param i32 i32 i32) (result i32)))"#,
        r#"(import "launchdeck" "socket" (func (param i32) (result i32)))"#,
    ]
    .iter()
    .enumerate()
    {
        let id = format!("test.wasi{i}");
        let dir = w.plugins.join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = manifest(&id, "1.0.0", json!([server.origin()]));
        m["main"] = json!("m.wasm");
        std::fs::write(dir.join("plugin.json"), m.to_string()).unwrap();
        std::fs::write(dir.join("m.wasm"), wat::Wat::new().import(import).reply(r#"{"status":"ok"}"#).build()).unwrap();
    }
    w.host.scan();
    for i in 0..5 {
        let id = format!("test.wasi{i}");
        let external = w.host.get(&id).unwrap().plugin.external.unwrap();
        assert_eq!(external.status, PluginStatus::Error, "{id}");
        assert!(
            external
                .error
                .unwrap()
                .contains("which LaunchDeck does not provide"),
            "{id}"
        );
    }
    assert_eq!(server.hits(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_plugin_makes_no_requests() {
    let server = hello();
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    w.host.set_enabled(PROBE, false).unwrap();
    let e = w
        .invoke("fetch", json!({ "url": server.url("/") }))
        .unwrap_err();
    assert!(e.to_string().contains("disabled"), "{e}");
    assert_eq!(server.hits(), 0);
    // The grant is the person's and stays; the plugin works again when it is turned on.
    w.host.set_enabled(PROBE, true).unwrap();
    assert_eq!(w.get(&server.url("/"))["body"], "hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_network_is_closed_while_a_plugin_starts() {
    let server = hello();
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    w.host
        .set_config(PROBE, "initUrl", json!(server.url("/at-start")))
        .unwrap();
    assert_eq!(
        w.status(PROBE),
        PluginStatus::Enabled,
        "the plugin still starts"
    );
    let r = w.invoke("init_result", json!({})).unwrap();
    assert_eq!(r["error"], "invalid_request");
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("while a plugin starts"),
        "{r}"
    );
    assert_eq!(server.hits(), 0);
}

// ---------------------------------------------------------------------------
// Limits and failures
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn waiting_for_a_server_is_not_the_modules_own_time() {
    // The module may run for 600 ms; the server takes longer than that to answer.
    let server = TestServer::start(|_| Reply::text(200, "slow").after(Duration::from_millis(1200)));
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    let started = Instant::now();
    let r = w
        .invoke("fetch_then_work", json!({ "url": server.url("/") }))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(1100));
    assert_eq!(r["body"], "slow");
    assert_eq!(
        w.status(PROBE),
        PluginStatus::Enabled,
        "the wait did not trip the module's deadline"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_that_times_out_fails_the_request_not_the_plugin() {
    let server = TestServer::start(|_| Reply::text(200, "late").after(Duration::from_secs(5)));
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    let started = Instant::now();
    let r = w
        .invoke(
            "fetch",
            json!({ "url": server.url("/"), "timeoutMs": 1000 }),
        )
        .unwrap();
    assert_eq!(r["error"], "timeout");
    assert!(
        started.elapsed() < Duration::from_millis(3000),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(w.status(PROBE), PluginStatus::Enabled);
    // The next request is unaffected.
    let quick = hello();
    let w2 = world(&[probe(json!([quick.origin()]))]);
    w2.grant(&quick.origin());
    assert_eq!(w2.get(&quick.url("/"))["status"], 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_and_malformed_answers_are_errors_the_plugin_can_read() {
    let server = TestServer::start(|r| match r.target.as_str() {
        "/big" => Reply::Response {
            status: 200,
            headers: vec![],
            body: vec![b'x'; 1024 * 1024 + 1],
            delay: Duration::ZERO,
        },
        "/garbage" => Reply::Raw(b"\x01\x02 not http at all".to_vec()),
        "/binary" => Reply::Response {
            status: 200,
            headers: vec![],
            body: vec![0xff, 0xfe, 0xfd],
            delay: Duration::ZERO,
        },
        _ => Reply::text(200, &r.body.len().to_string()),
    });
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    assert_eq!(w.get(&server.url("/big"))["error"], "response_too_large");
    assert_eq!(w.get(&server.url("/binary"))["error"], "invalid_response");
    let garbage = w.get(&server.url("/garbage"));
    assert!(
        ["invalid_response", "network_error"].contains(&garbage["error"].as_str().unwrap()),
        "{garbage}"
    );

    let post = |n: usize| {
        w.invoke(
            "fetch",
            json!({ "method": "POST", "url": server.url("/echo"), "bodySize": n }),
        )
        .unwrap()
    };
    assert_eq!(post(1000)["body"], "1000");
    let before = server.hits();
    assert_eq!(post(256 * 1024 + 1)["error"], "request_too_large");
    assert_eq!(server.hits(), before, "an oversized request is not sent");
    assert_eq!(
        w.status(PROBE),
        PluginStatus::Enabled,
        "none of this stopped the plugin"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_call_cannot_make_an_unbounded_number_of_requests() {
    let server = hello();
    let w = world(&[probe(json!([server.origin()]))]);
    w.grant(&server.origin());
    let r = w
        .invoke("burst", json!({ "url": server.url("/"), "count": 40 }))
        .unwrap();
    assert_eq!(
        (r["answered"].as_i64(), r["lastError"].as_str()),
        (Some(24), Some("rate_limited"))
    );
    assert_eq!(server.hits(), 24);
    // The next call starts with a full allowance.
    assert_eq!(w.get(&server.url("/"))["status"], 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_that_dies_during_a_call_fails_that_call_and_nothing_else() {
    let server = hello();
    let mut other = manifest("test.other", "1.0.0", json!([server.origin()]));
    other["name"] = json!("Other");
    let w = world(&[probe(json!([server.origin()])), other]);
    w.grant(&server.origin());
    w.host
        .set_permission("test.other", &server.origin(), Some(Decision::Granted))
        .unwrap();

    let e = w
        .invoke("fetch_then_trap", json!({ "url": server.url("/") }))
        .unwrap_err();
    assert!(e.to_string().contains("failed"), "{e}");
    assert_eq!(
        server.hits(),
        1,
        "the request had gone out before the module died"
    );
    assert_eq!(w.status(PROBE), PluginStatus::Error);
    // What it said before dying is not delivered, and the hub is not left with a guess.
    assert_eq!(
        w.hub
            .read_address(&StateAddress::new(PROBE, "status"))
            .status,
        Status::Unavailable
    );
    // It refuses further calls instead of hanging, and the other plugin never noticed.
    assert!(w
        .invoke("fetch", json!({ "url": server.url("/") }))
        .is_err());
    let r = w
        .invoke_as("test.other", "fetch", json!({ "url": server.url("/") }))
        .unwrap();
    assert_eq!(r["body"], "hello");
    // A reload brings it back, with its grant intact.
    w.host.reload(PROBE).unwrap();
    assert_eq!(w.get(&server.url("/"))["body"], "hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_wait_on_the_network_at_the_same_time_and_one_plugin_takes_turns() {
    let server = TestServer::start(|_| Reply::text(200, "ok").after(Duration::from_millis(500)));
    let ids = ["test.a", "test.b", "test.c"];
    let manifests: Vec<Value> = ids
        .iter()
        .map(|id| manifest(id, "1.0.0", json!([server.origin()])))
        .collect();
    let w = Arc::new(world(&manifests));
    for id in ids {
        w.host
            .set_permission(id, &server.origin(), Some(Decision::Granted))
            .unwrap();
    }
    let url = server.url("/");

    // Three plugins, one request each: they overlap.
    let started = Instant::now();
    let tasks: Vec<_> = ids
        .iter()
        .map(|id| {
            let (w, url, id) = (w.clone(), url.clone(), id.to_string());
            tokio::task::spawn_blocking(move || {
                w.invoke_as(&id, "fetch", json!({ "url": url })).unwrap()["status"].clone()
            })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap(), 200);
    }
    let together = started.elapsed();
    assert!(
        together < Duration::from_millis(1300),
        "three 500 ms requests of three plugins took {together:?}"
    );

    // One plugin, two calls at once: both are answered, one after the other.
    let started = Instant::now();
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let (w, url) = (w.clone(), url.clone());
            tokio::task::spawn_blocking(move || {
                w.invoke_as("test.a", "fetch", json!({ "url": url }))
                    .unwrap()["status"]
                    .clone()
            })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap(), 200);
    }
    assert!(
        started.elapsed() >= Duration::from_millis(950),
        "one instance handles one call at a time"
    );
}

// ---------------------------------------------------------------------------
// A new version, a changed setting
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_new_version_that_asks_for_more_gets_nothing_more_by_itself() {
    let (old, new) = (hello(), hello());
    let w = world(&[probe(json!([old.origin()]))]);
    w.grant(&old.origin());
    assert_eq!(w.get(&old.url("/"))["status"], 200);

    // The update: one more destination.
    install(
        &w.plugins,
        PROBE,
        &manifest(PROBE, "1.1.0", json!([old.origin(), new.origin()])),
    );
    assert_eq!(w.host.scan().reloaded, 1);
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert_eq!(
        view.status,
        GrantStatus::NeedsApproval,
        "the plugin as a whole needs a decision"
    );
    let status = |origin: &str| {
        view.hosts
            .iter()
            .find(|h| h.origin == origin)
            .unwrap()
            .status
    };
    assert_eq!(
        status(&old.origin()),
        GrantStatus::Granted,
        "what was granted stays granted"
    );
    assert_eq!(status(&new.origin()), GrantStatus::NeedsApproval);

    assert_eq!(
        w.get(&old.url("/"))["status"],
        200,
        "the old destination keeps working"
    );
    assert_eq!(w.get(&new.url("/"))["error"], "permission_denied");
    assert_eq!(new.hits(), 0);

    // The trail says who asked for what and who decided what, and nothing else.
    let trail = std::fs::read_to_string(w.data.join("audit.jsonl")).unwrap();
    let lines: Vec<Value> = trail
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let said: Vec<(String, String)> = lines
        .iter()
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_string(),
                l["detail"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        said,
        vec![
            (
                "permission.requested".to_string(),
                format!("network {}", old.origin())
            ),
            (
                "permission.granted".to_string(),
                format!("network {}", old.origin())
            ),
            (
                "permission.requested".to_string(),
                format!("network {}", new.origin())
            ),
        ]
    );
    assert!(lines.iter().all(|l| l["pluginId"] == PROBE));

    // A version that drops a destination keeps the old decision on file, covering nothing.
    install(
        &w.plugins,
        PROBE,
        &manifest(PROBE, "1.2.0", json!([new.origin()])),
    );
    w.host.scan();
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert_eq!(view.stale.len(), 1);
    assert_eq!(
        w.get(&old.url("/"))["error"],
        "host_not_allowed",
        "a grant for what is no longer asked for covers nothing"
    );
    w.host.set_permission(PROBE, &old.origin(), None).unwrap();
    assert!(w.host.get(PROBE).unwrap().network.unwrap().stale.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_named_by_a_setting_is_asked_for_as_the_exact_origin_typed() {
    let (first, second) = (hello(), hello());
    let w = world(&[probe(json!([{ "setting": "target" }]))]);
    // Nothing typed yet: nothing concrete to grant.
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert!(view.hosts.is_empty());
    assert_eq!(
        (view.status, view.unresolved[0].setting.as_str()),
        (GrantStatus::NeedsApproval, "target")
    );

    w.host
        .set_config(
            PROBE,
            "target",
            json!(format!("{}/some/path", first.origin())),
        )
        .unwrap();
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert_eq!(
        view.hosts[0].origin,
        first.origin(),
        "the origin of what was typed, not the path"
    );
    assert_eq!(
        view.hosts[0].status,
        GrantStatus::NeedsApproval,
        "typing an address grants nothing"
    );
    assert_eq!(w.get(&first.url("/"))["error"], "permission_denied");
    w.grant(&first.origin());
    assert_eq!(w.get(&first.url("/"))["status"], 200);

    // A different address is a different destination: the old grant does not follow it.
    w.host
        .set_config(PROBE, "target", json!(second.origin()))
        .unwrap();
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert_eq!(
        (view.hosts[0].origin.as_str(), view.hosts[0].status),
        (second.origin().as_str(), GrantStatus::NeedsApproval)
    );
    assert_eq!(view.stale[0].origin, first.origin());
    assert_eq!(w.get(&second.url("/"))["error"], "permission_denied");
    assert_eq!(w.get(&first.url("/"))["error"], "host_not_allowed");
    assert_eq!((first.hits(), second.hits()), (1, 0));

    // Something that is not an address asks for nothing.
    w.host
        .set_config(PROBE, "target", json!("ftp://nope"))
        .unwrap();
    let view = w.host.get(PROBE).unwrap().network.unwrap();
    assert!(
        view.hosts.is_empty() && view.unresolved[0].reason.contains("only http and https"),
        "{:?}",
        view.unresolved
    );
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

fn with_secret(origin: &str) -> Value {
    let mut m = probe(json!([origin]));
    m["secrets"] = json!([{ "key": "token", "label": "Access token", "host": origin }]);
    m
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_reaches_the_server_and_nothing_else() {
    const SECRET: &str = "hunter2-very-secret-value";
    let server = TestServer::start(|r| {
        Reply::text(
            200,
            if r.header("authorization").is_some() {
                "authenticated"
            } else {
                "anonymous"
            },
        )
    });
    let w = world(&[with_secret(&server.origin())]);
    w.grant(&server.origin());
    let ask = || {
        w.invoke(
            "fetch",
            json!({ "url": server.url("/api"), "secret": "token" }),
        )
        .unwrap()
    };

    // Missing: a clear failure, and no request.
    let info = w.host.get(PROBE).unwrap();
    assert!(!info.secrets[0].is_set);
    assert_eq!(
        info.secrets[0].origin.as_deref(),
        Some(server.origin().as_str())
    );
    let r = ask();
    assert_eq!(r["error"], "secret_missing");
    assert_eq!(server.hits(), 0);

    assert!(
        w.host.set_secret(PROBE, "nope", SECRET).is_err(),
        "only declared secrets can be set"
    );
    let info = w.host.set_secret(PROBE, "token", SECRET).unwrap();
    assert!(info.secrets[0].is_set);
    let r = ask();
    assert_eq!(r["body"], "authenticated");
    assert_eq!(
        server.received()[0].header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );

    // The value is in the store and at the server. It is nowhere else.
    assert_eq!(
        w.store
            .get(&SecretRef::new(PROBE, "token"))
            .unwrap()
            .as_deref(),
        Some(SECRET)
    );
    let mut places = vec![
        ("what the plugin received", r.to_string()),
        (
            "the plugin list",
            serde_json::to_string(&w.host.list()).unwrap(),
        ),
        (
            "the plugin's log",
            serde_json::to_string(&w.host.logs(PROBE).unwrap()).unwrap(),
        ),
        (
            "the manifest",
            std::fs::read_to_string(w.plugins.join(PROBE).join("plugin.json")).unwrap(),
        ),
    ];
    for file in ["plugins.json", "permissions.json", "audit.jsonl"] {
        if let Ok(text) = std::fs::read_to_string(w.data.join(file)) {
            places.push((file, text));
        }
    }
    for (place, text) in &places {
        assert!(
            !text.contains(SECRET) && !text.contains("hunter2"),
            "the secret is in {place}"
        );
    }
    assert!(serde_json::to_string(&w.host.logs(PROBE).unwrap())
        .unwrap()
        .contains("secret `token` was set"));

    // Removed: authentication stops cleanly.
    let info = w.host.clear_secret(PROBE, "token").unwrap();
    assert!(!info.secrets[0].is_set);
    assert_eq!(w.store.get(&SecretRef::new(PROBE, "token")).unwrap(), None);
    assert_eq!(ask()["error"], "secret_missing");
    assert_eq!(server.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_is_not_sent_to_another_granted_origin() {
    let (home, other) = (hello(), hello());
    let mut m = probe(json!([home.origin(), other.origin()]));
    m["secrets"] = json!([{ "key": "token", "label": "Access token", "host": home.origin() }]);
    let w = world(&[m]);
    w.grant(&home.origin());
    w.grant(&other.origin());
    w.host.set_secret(PROBE, "token", "s3cr3t").unwrap();
    let r = w
        .invoke("fetch", json!({ "url": other.url("/"), "secret": "token" }))
        .unwrap();
    assert_eq!(r["error"], "host_not_allowed");
    assert!(
        r["message"].as_str().unwrap().contains("only ever sent to"),
        "{r}"
    );
    assert_eq!(other.hits(), 0);
    assert_eq!(
        w.invoke("fetch", json!({ "url": home.url("/"), "secret": "token" }))
            .unwrap()["status"],
        200
    );
    assert_eq!(
        home.received()[0].header("authorization"),
        Some("Bearer s3cr3t")
    );
}
