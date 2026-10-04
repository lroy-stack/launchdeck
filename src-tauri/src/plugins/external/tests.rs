//! Boundary tests for a sandboxed plugin behind the `Plugin` trait: normalized
//! results, event publication, and every way a hostile module can misbehave.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::log::{Level, Origin};
use super::sandbox::Limits;
use super::testkit::*;
use super::PluginStatus;
use crate::events::{Event, EventBus};
use crate::plugins::{ActionError, Plugin};

const OK: &str = r#"{"status":"ok","value":{"x":1}}"#;
const EVT: &str = r#"{"type":"t.evt","payload":{"n":1}}"#;

fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<Event> {
    let mut out = vec![];
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

fn call(p: &dyn Plugin, bus: &EventBus, action: &str, args: Value) -> Result<Value, ActionError> {
    p.invoke(&ctx("t.plug", bus), action, &args)
}

fn failed_text(e: ActionError) -> String {
    match e {
        ActionError::Failed(m) => m,
        other => panic!("expected Failed, got {other:?}"),
    }
}

// --------------------------------------------------------------------------
// Results
// --------------------------------------------------------------------------

#[tokio::test]
async fn success_is_returned_as_the_value() {
    let p = load(
        Wat::new().reply(OK).build(),
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    assert_eq!(p.status().0, PluginStatus::Enabled);
    let bus = EventBus::new();
    assert_eq!(call(&*p, &bus, "a", json!({})).unwrap(), json!({"x": 1}));
}

#[tokio::test]
async fn failure_is_normalized_and_the_plugin_stays_healthy() {
    let wasm = Wat::new()
        .reply(r#"{"status":"failed","error":"no such note","code":"not_found"}"#)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let err = call(&*p, &bus, "a", json!({})).unwrap_err();
    assert_eq!(err, ActionError::Failed("no such note (not_found)".into()));
    assert_eq!(p.status().0, PluginStatus::Enabled);
}

#[tokio::test]
async fn arguments_are_checked_before_the_module_runs() {
    // This module traps on any invoke: if the host called it, the plugin would be in error.
    let wasm = Wat::new().body("unreachable").build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    assert!(matches!(
        call(&*p, &bus, "a", json!({"s": 5})),
        Err(ActionError::InvalidArgs(_))
    ));
    assert!(matches!(
        call(&*p, &bus, "a", json!({"nope": 1})),
        Err(ActionError::InvalidArgs(_))
    ));
    assert!(matches!(
        call(&*p, &bus, "ghost", json!({})),
        Err(ActionError::UnknownAction { .. })
    ));
    assert_eq!(p.status().0, PluginStatus::Enabled);
}

#[tokio::test]
async fn module_state_persists_between_calls() {
    let body = format!(
        "(global.set $calls (i32.add (global.get $calls) (i32.const 1)))
         (if (i32.eq (global.get $calls) (i32.const 1))
           (then {first}) (else {second}))
         (i32.const 0)",
        first = respond_at(0, r#"{"status":"ok","value":"first"}"#.len()),
        second = respond_at(100, r#"{"status":"ok","value":"second"}"#.len()),
    );
    let wasm = Wat::new()
        .data(0, r#"{"status":"ok","value":"first"}"#)
        .data(100, r#"{"status":"ok","value":"second"}"#)
        .body(&body)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    assert_eq!(call(&*p, &bus, "a", json!({})).unwrap(), json!("first"));
    assert_eq!(call(&*p, &bus, "a", json!({})).unwrap(), json!("second"));
}

// --------------------------------------------------------------------------
// Events
// --------------------------------------------------------------------------

fn publishing(event_json: &str, extra: &str) -> Vec<u8> {
    Wat::new()
        .data(EVENT_AT, event_json)
        .data(0, OK)
        .body(&format!(
            "{} {extra} (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(event_json),
            OK.len()
        ))
        .build()
}

#[tokio::test]
async fn declared_events_reach_the_bus_with_the_plugin_as_source() {
    let p = load(
        publishing(EVT, ""),
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    call(&*p, &bus, "a", json!({})).unwrap();
    let events = drain(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, "t.plug");
    assert_eq!(events[0].kind, "t.evt");
    assert_eq!(events[0].payload, json!({"n": 1}));
}

#[tokio::test]
async fn a_plugin_cannot_choose_its_event_source() {
    let spoof = r#"{"type":"t.evt","source":"runtime","payload":{}}"#;
    let p = load(
        publishing(spoof, ""),
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    call(&*p, &bus, "a", json!({})).unwrap();
    let events = drain(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, "t.plug", "the host decides the source");
}

#[tokio::test]
async fn undeclared_or_host_owned_event_types_are_dropped() {
    for bad in [
        r#"{"type":"other.evt"}"#,
        r#"{"type":"control.pressed","payload":{}}"#,
        r#"{"type":"plugin.state.changed","payload":{"key":"a","value":"b"}}"#,
        r#"{"type":"bad type"}"#,
        r#"{"payload":{}}"#,
        "not json",
        r#"[1,2]"#,
    ] {
        let p = load(
            publishing(bad, ""),
            manifest_json("t.plug", &["a"], &["t.evt"]),
            Limits::default(),
        );
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        // The call itself still succeeds; only the event is refused.
        call(&*p, &bus, "a", json!({})).unwrap();
        assert!(drain(&mut rx).is_empty(), "{bad} should not be published");
        assert!(
            p.log()
                .entries()
                .iter()
                .any(|l| l.origin == Origin::Host && l.message.contains("rejected")),
            "{bad} should leave a diagnostic"
        );
    }
}

#[tokio::test]
async fn oversized_events_are_dropped_without_being_read() {
    let wasm = Wat::new()
        .data(0, OK)
        .body(&format!(
            "(drop (call $publish (i32.const 2000) (i32.const 20000))) {} (i32.const 0)",
            respond_at(0, OK.len())
        ))
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    call(&*p, &bus, "a", json!({})).unwrap();
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn events_per_call_are_capped() {
    let body = format!(
        "(global.set $calls (i32.const 0))
         (loop $l
           {publish}
           (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
           (br_if $l (i32.lt_u (global.get $calls) (i32.const 100))))
         {respond} (i32.const 0)",
        publish = Wat::publish_code(EVT),
        respond = respond_at(0, OK.len()),
    );
    let wasm = Wat::new()
        .data(EVENT_AT, EVT)
        .data(0, OK)
        .body(&body)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    call(&*p, &bus, "a", json!({})).unwrap();
    assert_eq!(drain(&mut rx).len(), 32);
}

#[tokio::test]
async fn a_failed_call_publishes_nothing() {
    // The module publishes, then traps.
    let wasm = Wat::new()
        .data(EVENT_AT, EVT)
        .body(&format!("{} unreachable", Wat::publish_code(EVT)))
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    assert!(call(&*p, &bus, "a", json!({})).is_err());
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn events_survive_a_plugin_reported_failure() {
    // A plugin may publish `x.failed` and also return failure; both are facts.
    let body = format!(
        "{} {} (i32.const 0)",
        Wat::publish_code(EVT),
        respond_at(0, r#"{"status":"failed","error":"e"}"#.len())
    );
    let wasm = Wat::new()
        .data(EVENT_AT, EVT)
        .data(0, r#"{"status":"failed","error":"e"}"#)
        .body(&body)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    assert!(call(&*p, &bus, "a", json!({})).is_err());
    assert_eq!(drain(&mut rx).len(), 1);
}

// --------------------------------------------------------------------------
// Isolation: crashes and runaway modules
// --------------------------------------------------------------------------

#[tokio::test]
async fn a_trap_marks_the_plugin_unhealthy_and_is_reported_as_a_failure() {
    let wasm = Wat::new().body("unreachable").build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("plugin `t.plug` failed"), "{msg}");
    let (status, error) = p.status();
    assert_eq!(status, PluginStatus::Error);
    assert!(error.unwrap().contains("aborted"));
    // Later calls fail fast without touching the dead instance.
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("unavailable"), "{msg}");
    assert!(p.log().entries().iter().any(|l| l.level == Level::Error));
}

#[tokio::test]
async fn an_unhealthy_plugin_recovers_when_reactivated() {
    let bad = Wat::new().body("unreachable").build();
    let p = load(bad, manifest_json("t.plug", &["a"], &[]), Limits::default());
    let bus = EventBus::new();
    assert!(call(&*p, &bus, "a", json!({})).is_err());
    p.activate(&Wat::new().reply(OK).build(), &Default::default());
    assert_eq!(p.status().0, PluginStatus::Enabled);
    assert!(call(&*p, &bus, "a", json!({})).is_ok());
}

#[tokio::test]
async fn an_endless_loop_runs_out_of_fuel_and_is_stopped() {
    let wasm = Wat::new().body("(loop $l (br $l)) (i32.const 0)").build();
    let p = load(wasm, manifest_json("t.plug", &["a"], &[]), quick());
    let bus = EventBus::new();
    let started = Instant::now();
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("CPU budget"), "{msg}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(p.status().0, PluginStatus::Error);
}

#[tokio::test]
async fn an_endless_loop_is_also_stopped_by_the_wall_clock() {
    // Effectively unlimited fuel, so only the epoch deadline can stop it.
    let limits = Limits {
        fuel: i64::MAX as u64,
        timeout: Duration::from_millis(150),
        memory_bytes: 2 * 1024 * 1024,
    };
    let wasm = Wat::new().body("(loop $l (br $l)) (i32.const 0)").build();
    let p = load(wasm, manifest_json("t.plug", &["a"], &[]), limits);
    let bus = EventBus::new();
    let started = Instant::now();
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("did not finish within 150 ms"), "{msg}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(p.status().0, PluginStatus::Error);
}

#[tokio::test]
async fn a_hung_plugin_does_not_block_listing_plugins() {
    let limits = Limits {
        fuel: i64::MAX as u64,
        timeout: Duration::from_millis(600),
        memory_bytes: 2 * 1024 * 1024,
    };
    let wasm = Wat::new().body("(loop $l (br $l)) (i32.const 0)").build();
    let p = load(wasm, manifest_json("t.plug", &["a"], &[]), limits);
    let runner = p.clone();
    let bus = EventBus::new();
    let c = ctx("t.plug", &bus);
    let handle = std::thread::spawn(move || {
        let _ = runner.invoke(&c, "a", &json!({}));
    });
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    let _ = p.manifest(); // what a UI snapshot does
    assert!(
        t.elapsed() < Duration::from_millis(200),
        "manifest() waited for the call"
    );
    handle.join().unwrap();
}

#[tokio::test]
async fn runaway_recursion_is_a_stack_overflow_trap() {
    let wasm = Wat::new()
        .funcs("(func $r (result i32) (i32.add (call $r) (i32.const 1)))")
        .body("(drop (call $r)) (i32.const 0)")
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("stack"), "{msg}");
    assert_eq!(p.status().0, PluginStatus::Error);
}

#[tokio::test]
async fn memory_growth_beyond_the_cap_is_refused_inside_the_module() {
    let refused = r#"{"status":"ok","value":"refused"}"#;
    let granted = r#"{"status":"ok","value":"granted"}"#;
    let grow = |pages: u32| {
        format!(
            "(if (i32.eq (memory.grow (i32.const {pages})) (i32.const -1))
               (then {r}) (else {g}))",
            r = respond_at(0, refused.len()),
            g = respond_at(100, granted.len())
        )
    };
    let wasm = Wat::new()
        .data(0, refused)
        .data(100, granted)
        .body(&dispatch(&[('a', &grow(1)), ('b', &grow(1000))]))
        .build();
    let p = load(wasm, manifest_json("t.plug", &["a", "b"], &[]), quick());
    let bus = EventBus::new();
    assert_eq!(call(&*p, &bus, "a", json!({})).unwrap(), json!("granted"));
    assert_eq!(call(&*p, &bus, "b", json!({})).unwrap(), json!("refused"));
    assert_eq!(p.status().0, PluginStatus::Enabled);
}

#[tokio::test]
async fn one_failing_plugin_leaves_the_others_untouched() {
    let bad = load(
        Wat::new().body("unreachable").build(),
        manifest_json("t.bad", &["a"], &[]),
        Limits::default(),
    );
    let good = load(
        Wat::new().reply(OK).build(),
        manifest_json("t.good", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    assert!(bad.invoke(&ctx("t.bad", &bus), "a", &json!({})).is_err());
    assert_eq!(bad.status().0, PluginStatus::Error);
    assert_eq!(good.status().0, PluginStatus::Enabled);
    assert!(good.invoke(&ctx("t.good", &bus), "a", &json!({})).is_ok());
}

// --------------------------------------------------------------------------
// Malformed output
// --------------------------------------------------------------------------

#[tokio::test]
async fn malformed_responses_fail_the_call_but_not_the_plugin() {
    for bad in [
        "not json",
        "[1,2]",
        "{}",
        r#"{"status":"pending"}"#,
        r#"{"status":42}"#,
    ] {
        let p = load(
            Wat::new().reply(bad).build(),
            manifest_json("t.plug", &["a"], &[]),
            Limits::default(),
        );
        let bus = EventBus::new();
        let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
        assert!(msg.contains("unusable response"), "{bad}: {msg}");
        assert_eq!(p.status().0, PluginStatus::Enabled, "{bad}");
    }
}

#[tokio::test]
async fn protocol_violations_fail_the_call_but_not_the_plugin() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("no response", Wat::new().body("(i32.const 0)").build()),
        ("internal error", Wat::new().body("(i32.const 3)").build()),
        (
            "too large",
            Wat::new()
                .body("(call $respond (i32.const 0) (i32.const 70000)) (i32.const 0)")
                .build(),
        ),
        (
            "twice",
            Wat::new()
                .data(0, OK)
                .body(&format!("{0} {0} (i32.const 0)", respond_at(0, OK.len())))
                .build(),
        ),
    ];
    for (name, wasm) in cases {
        let p = load(
            wasm,
            manifest_json("t.plug", &["a"], &[]),
            Limits::default(),
        );
        let bus = EventBus::new();
        let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
        assert!(msg.contains("plugin `t.plug`"), "{name}: {msg}");
        assert_eq!(p.status().0, PluginStatus::Enabled, "{name}");
    }
}

#[tokio::test]
async fn an_oversized_request_is_refused_before_the_module_sees_it() {
    // A `json` setting has no length cap of its own, so the host's request limit is what answers.
    let mut manifest = manifest_json("t.plug", &["a"], &[]);
    manifest["actions"][0]["args"] = json!([{"name":"s","type":"json"}]);
    let p = load(Wat::new().reply(OK).build(), manifest, Limits::default());
    let bus = EventBus::new();
    let big = "x".repeat(70 * 1024);
    let msg = failed_text(call(&*p, &bus, "a", json!({"s": big})).unwrap_err());
    assert!(msg.contains("larger than"), "{msg}");
    assert_eq!(p.status().0, PluginStatus::Enabled);
}

#[tokio::test]
async fn a_bad_pointer_is_a_trap() {
    let wasm = Wat::new()
        .body("(call $respond (i32.const -1) (i32.const 10)) (i32.const 0)")
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    assert!(call(&*p, &bus, "a", json!({})).is_err());
    assert_eq!(p.status().0, PluginStatus::Error);
}

// --------------------------------------------------------------------------
// Loading
// --------------------------------------------------------------------------

fn load_error(wasm: Vec<u8>) -> String {
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let (status, error) = p.status();
    assert_eq!(status, PluginStatus::Error);
    error.unwrap()
}

#[tokio::test]
async fn modules_asking_for_system_access_do_not_load() {
    let wasm = Wat::new()
        .import(r#"(import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))"#)
        .build();
    let e = load_error(wasm);
    assert!(
        e.contains("wasi_snapshot_preview1::fd_write") && e.contains("no system access"),
        "{e}"
    );
    let wasm = Wat::new()
        .import(r#"(import "env" "abort" (func (param i32 i32 i32 i32)))"#)
        .build();
    assert!(load_error(wasm).contains("env::abort"));
    let wasm = Wat::new()
        .import(r#"(import "launchdeck" "shell" (func (param i32 i32)))"#)
        .build();
    assert!(load_error(wasm).contains("launchdeck::shell"));
}

#[tokio::test]
async fn broken_modules_do_not_load() {
    assert!(
        load_error(b"\0asm\x01\0\0\0garbage".to_vec()).contains("not a valid WebAssembly module")
    );
    assert!(load_error(Wat::new().abi(2).build()).contains("protocol 2"));
    assert!(load_error(Wat::new().without_memory_export().build()).contains("`memory`"));
    assert!(load_error(Wat::new().without_handle().build()).contains("ld_handle"));
}

#[tokio::test]
async fn an_init_failure_is_isolated_and_reported() {
    let failed = r#"{"status":"failed","error":"boom"}"#;
    let wasm = Wat::new()
        .data(500, failed)
        .init(&format!(
            "{} (return (i32.const 0))",
            respond_at(500, failed.len())
        ))
        .build();
    let e = load_error(wasm);
    assert!(e.contains("init failed: boom"), "{e}");
    let wasm = Wat::new().init("unreachable").build();
    assert!(load_error(wasm).contains("init"));
    let wasm = Wat::new()
        .init("(loop $l (br $l)) (return (i32.const 0))")
        .build();
    let p = load(wasm, manifest_json("t.plug", &["a"], &[]), quick());
    assert_eq!(p.status().0, PluginStatus::Error);
}

#[tokio::test]
async fn requests_put_op_first_so_a_minimal_guest_can_tell_them_apart() {
    let p = load(
        Wat::new().reply(OK).build(),
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let req = super::wasm_plugin::init_request(p.spec(), &serde_json::Map::new());
    assert!(req.starts_with(r#"{"op":"init","abi":1,"host":"#), "{req}");
    assert!(req.contains(r#""config":{}"#));
    assert_eq!(req.as_bytes()[9], b'i');
    let inv = super::wasm_plugin::invoke_request("go", &json!({"a": 1}));
    assert_eq!(inv, r#"{"op":"invoke","action":"go","args":{"a":1}}"#);
    assert_eq!(inv.as_bytes()[9], b'v');
    assert_eq!(inv.as_bytes()[25], b'g');
}

// --------------------------------------------------------------------------
// Enable, disable
// --------------------------------------------------------------------------

#[tokio::test]
async fn a_disabled_plugin_never_runs() {
    // Traps if it is ever invoked.
    let wasm = Wat::new().body("unreachable").build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    p.deactivate();
    let bus = EventBus::new();
    let msg = failed_text(call(&*p, &bus, "a", json!({})).unwrap_err());
    assert!(msg.contains("disabled"), "{msg}");
    assert_eq!(p.status().0, PluginStatus::Disabled, "not marked as failed");
    assert!(p.log().entries().iter().all(|l| l.level != Level::Error));
}

// --------------------------------------------------------------------------
// Diagnostics
// --------------------------------------------------------------------------

#[tokio::test]
async fn plugin_log_lines_are_scoped_sanitized_and_bounded() {
    let text = "hello\nFAKE host line";
    let body = format!(
        "(call $log (i32.const 2) (i32.const {LOG_AT}) (i32.const {})) {} (i32.const 0)",
        text.len(),
        respond_at(0, OK.len())
    );
    let wasm = Wat::new()
        .data(LOG_AT, text)
        .data(0, OK)
        .body(&body)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    call(&*p, &bus, "a", json!({})).unwrap();
    let lines: Vec<_> = p
        .log()
        .entries()
        .into_iter()
        .filter(|l| l.origin == Origin::Plugin)
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, Level::Warn);
    assert_eq!(lines[0].message, "hello FAKE host line");
}

#[tokio::test]
async fn log_spam_is_capped_per_call() {
    let body = format!(
        "(global.set $calls (i32.const 0))
         (loop $l
           (call $log (i32.const 1) (i32.const {LOG_AT}) (i32.const 2))
           (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
           (br_if $l (i32.lt_u (global.get $calls) (i32.const 500))))
         {} (i32.const 0)",
        respond_at(0, OK.len())
    );
    let wasm = Wat::new()
        .data(LOG_AT, "hi")
        .data(0, OK)
        .body(&body)
        .build();
    let p = load(
        wasm,
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    call(&*p, &bus, "a", json!({})).unwrap();
    let from_plugin = p
        .log()
        .entries()
        .iter()
        .filter(|l| l.origin == Origin::Plugin)
        .count();
    assert_eq!(from_plugin, 50);
}

// --------------------------------------------------------------------------
// Concurrency and cost
// --------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_calls_to_one_plugin_are_serialized_safely() {
    let p = load(
        Wat::new().reply(OK).build(),
        manifest_json("t.plug", &["a"], &[]),
        Limits::default(),
    );
    let bus = EventBus::new();
    let handle = tokio::runtime::Handle::current();
    std::thread::scope(|s| {
        for _ in 0..8 {
            let p = p.clone();
            let bus = bus.clone();
            let handle = handle.clone();
            s.spawn(move || {
                for _ in 0..50 {
                    let c =
                        crate::plugins::PluginContext::new("t.plug", bus.clone(), handle.clone());
                    assert!(p.invoke(&c, "a", &json!({})).is_ok());
                }
            });
        }
    });
    assert_eq!(p.status().0, PluginStatus::Enabled);
}

#[tokio::test]
async fn invocation_and_load_cost_are_small() {
    let t = Instant::now();
    let p = load(
        publishing(EVT, ""),
        manifest_json("t.plug", &["a"], &["t.evt"]),
        Limits::default(),
    );
    let load_time = t.elapsed();
    let bus = EventBus::new();
    let n = 2000;
    let t = Instant::now();
    for _ in 0..n {
        call(&*p, &bus, "a", json!({})).unwrap();
    }
    let per_call = t.elapsed() / n;
    eprintln!("load {load_time:?}, invoke+publish {per_call:?} per call (debug build)");
    // Generous: a debug build on a loaded CI machine must still be far from a pad-press budget.
    assert!(load_time < Duration::from_secs(2));
    assert!(per_call < Duration::from_millis(5), "{per_call:?}");
}
