//! A sandboxed plugin writing states: ownership, validation, limits and the
//! lifecycle. Everything here goes through a real module in the real sandbox
//! (a WAT guest) and the real hub; only the guest is hand-made so that it can
//! say exactly the wrong thing.

use serde_json::{json, Value};

use super::sandbox::Limits;
use super::testkit::*;
use super::PluginStatus;
use crate::events::{Event, EventBus};
use crate::plugins::{ActionError, Plugin};
use crate::state::definition::Write;
use crate::state::{Scalar, StateAddress, StateHub, Status};

const OK: &str = r#"{"status":"ok","value":{"x":1}}"#;
const ID: &str = "t.plug";

/// A schema 3 manifest with one unscoped enum, one scoped enum, a number, a
/// boolean and one event.
fn manifest() -> Value {
    json!({
        "schemaVersion": 3, "id": ID, "name": "Test", "version": "1.0.0", "author": "tests",
        "abi": 1, "main": "m.wasm",
        "actions": [{"id": "a"}],
        "events": [{"type": "t.evt"}],
        "states": [
            { "id": "level", "name": "Level", "type": "enum",
              "values": [{"value":"zero","label":"Empty"},{"value":"low","label":"Low"},
                         {"value":"high","label":"High"}] },
            { "id": "chan", "name": "Channel", "type": "enum",
              "values": [{"value":"up","label":"Up"},{"value":"down","label":"Down"}],
              "params": [{"name":"n","label":"N","type":"string","required":true,"maxLength":8}] },
            { "id": "vol", "name": "Volume", "type": "number" },
            { "id": "mute", "name": "Muted", "type": "boolean" }
        ]
    })
}

fn state_of(hub: &StateHub, id: &str) -> (Status, Option<Scalar>) {
    let o = hub.read(&StateAddress::new(ID, id).key());
    (o.status, o.value)
}

fn scoped(hub: &StateHub, n: &str) -> (Status, Option<Scalar>) {
    let o = hub.read(&StateAddress::new(ID, "chan").with("n", n).key());
    (o.status, o.value)
}

/// A guest that publishes `message` and then answers ok. The return code of
/// `publish` becomes the return value of `ld_handle`, so a refusal shows up as
/// "reported an internal error (code N)" and an acceptance as a normal reply.
fn publishing(message: &str) -> Vec<u8> {
    Wat::new()
        .data(EVENT_AT, message)
        .data(0, OK)
        .body(&format!(
            "(global.set $calls (call $publish (i32.const {EVENT_AT}) (i32.const {len})))
             (call $respond (i32.const 0) (i32.const {ok}))
             (global.get $calls)",
            len = message.len(),
            ok = OK.len()
        ))
        .build()
}

fn run(
    message: &str,
) -> (
    StateHub,
    Result<Value, ActionError>,
    std::sync::Arc<super::wasm_plugin::WasmPlugin>,
) {
    let hub = StateHub::new();
    let p = load_with_hub(publishing(message), manifest(), Limits::default(), &hub);
    let bus = EventBus::new();
    let r = p.invoke(&ctx(ID, &bus), "a", &json!({}));
    (hub, r, p)
}

fn code_of(r: Result<Value, ActionError>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(ActionError::Failed(m)) => m
            .rsplit("(code ")
            .next()
            .and_then(|t| t.trim_end_matches(')').parse().ok())
            .unwrap_or_else(|| panic!("no code in `{m}`")),
        Err(other) => panic!("{other:?}"),
    }
}

fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<Event> {
    let mut out = vec![];
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

// ---------------------------------------------------------------------------
// Publishing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_declared_state_is_stored_and_no_event_is_needed() {
    let hub = StateHub::new();
    let p = load_with_hub(
        publishing(r#"{"state":"level","value":"high"}"#),
        manifest(),
        Limits::default(),
        &hub,
    );
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(
        state_of(&hub, "level"),
        (Status::Known, Some(Scalar::text("high")))
    );
    assert!(drain(&mut rx).is_empty(), "the state needed no event");
}

#[tokio::test]
async fn every_state_type_can_be_set_and_cleared() {
    for (message, id, want) in [
        (r#"{"state":"vol","value":0.5}"#, "vol", Scalar::Num(0.5)),
        (r#"{"state":"vol","value":3}"#, "vol", Scalar::Num(3.0)),
        (
            r#"{"state":"mute","value":true}"#,
            "mute",
            Scalar::Bool(true),
        ),
    ] {
        let (hub, r, _) = run(message);
        assert_eq!(code_of(r), 0, "{message}");
        assert_eq!(state_of(&hub, id), (Status::Known, Some(want)), "{message}");
    }
    // Clear: back to unknown, not unavailable.
    let hub = StateHub::new();
    let set = load_with_hub(
        publishing(r#"{"state":"level","value":"low"}"#),
        manifest(),
        Limits::default(),
        &hub,
    );
    let bus = EventBus::new();
    set.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(state_of(&hub, "level").0, Status::Known);
    let clear = Wat::new()
        .data(EVENT_AT, r#"{"state":"level","clear":true}"#)
        .data(0, OK)
        .body(&format!(
            "{} (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(r#"{"state":"level","clear":true}"#),
            OK.len()
        ))
        .build();
    // Same provider, a second copy that does the clearing.
    let other = load_with_hub(clear, manifest(), Limits::default(), &hub);
    // Loading it started the provider again: everything was forgotten.
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    other.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
}

#[tokio::test]
async fn scoped_states_have_one_value_per_instance() {
    let two = Wat::new()
        .data(
            EVENT_AT,
            r#"{"state":"chan","args":{"n":"a"},"value":"up"}"#,
        )
        .data(2500, r#"{"state":"chan","args":{"n":"b"},"value":"down"}"#)
        .data(0, OK)
        .body(&format!(
            "{}
             (drop (call $publish (i32.const 2500) (i32.const {})))
             (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(r#"{"state":"chan","args":{"n":"a"},"value":"up"}"#),
            r#"{"state":"chan","args":{"n":"b"},"value":"down"}"#.len(),
            OK.len()
        ))
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(two, manifest(), Limits::default(), &hub);
    let bus = EventBus::new();
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(scoped(&hub, "a"), (Status::Known, Some(Scalar::text("up"))));
    assert_eq!(
        scoped(&hub, "b"),
        (Status::Known, Some(Scalar::text("down")))
    );
    assert_eq!(
        scoped(&hub, "c"),
        (Status::Unknown, None),
        "an instance nobody reported"
    );
}

#[tokio::test]
async fn the_last_write_to_an_instance_in_one_call_wins() {
    let m1 = r#"{"state":"level","value":"low"}"#;
    let m2 = r#"{"state":"level","value":"high"}"#;
    let wasm = Wat::new()
        .data(EVENT_AT, m1)
        .data(2500, m2)
        .data(0, OK)
        .body(&format!(
            "{}
             (drop (call $publish (i32.const 2500) (i32.const {})))
             (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(m1),
            m2.len(),
            OK.len()
        ))
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    hub.drain();
    p.invoke(&ctx(ID, &EventBus::new()), "a", &json!({}))
        .unwrap();
    assert_eq!(state_of(&hub, "level").1, Some(Scalar::text("high")));
    assert_eq!(
        hub.drain().keys.len(),
        1,
        "one entry in the next batch of changes"
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn what_does_not_fit_the_declaration_is_refused_with_a_code() {
    use super::sandbox::publish_code::*;
    let big = format!(r#"{{"state":"level","value":"{}"}}"#, "x".repeat(3000));
    let cases: Vec<(String, i32)> = vec![
        // Accepted.
        (r#"{"state":"level","value":"zero"}"#.into(), ACCEPTED),
        (
            r#"{"state":"level","args":{},"value":"zero"}"#.into(),
            ACCEPTED,
        ),
        (
            r#"{"state":"level","args":null,"value":"zero"}"#.into(),
            ACCEPTED,
        ),
        // Not declared by this plugin: the other providers' states do not exist for it.
        (r#"{"state":"nope","value":"x"}"#.into(), NOT_ALLOWED),
        (
            r#"{"state":"slot.status","args":{"slot":1},"value":"idle"}"#.into(),
            NOT_ALLOWED,
        ),
        (
            r#"{"state":"agent-control.slot.status","args":{"slot":1},"value":"idle"}"#.into(),
            NOT_ALLOWED,
        ),
        (
            r#"{"state":"system.anything","value":"x"}"#.into(),
            NOT_ALLOWED,
        ),
        (r#"{"state":"Level","value":"zero"}"#.into(), NOT_ALLOWED),
        // The module never names the provider.
        (
            r#"{"state":"level","provider":"agent-control","value":"zero"}"#.into(),
            MALFORMED,
        ),
        (
            r#"{"state":"level","source":"t.other","value":"zero"}"#.into(),
            MALFORMED,
        ),
        // Wrong type, an enum value that is not listed, no scalar.
        (r#"{"state":"level","value":5}"#.into(), INVALID),
        (r#"{"state":"level","value":"nope"}"#.into(), INVALID),
        (r#"{"state":"level","value":null}"#.into(), INVALID),
        (r#"{"state":"level","value":["zero"]}"#.into(), INVALID),
        (r#"{"state":"level","value":{"a":1}}"#.into(), INVALID),
        (r#"{"state":"vol","value":"loud"}"#.into(), INVALID),
        (r#"{"state":"mute","value":"yes"}"#.into(), INVALID),
        // Scope: an unscoped state takes none; a scoped one needs its argument, and only its own.
        (
            r#"{"state":"level","args":{"n":"a"},"value":"zero"}"#.into(),
            INVALID,
        ),
        (r#"{"state":"chan","value":"up"}"#.into(), INVALID),
        (
            r#"{"state":"chan","args":{"n":"a","extra":1},"value":"up"}"#.into(),
            INVALID,
        ),
        (
            r#"{"state":"chan","args":{"n":5},"value":"up"}"#.into(),
            INVALID,
        ),
        (
            r#"{"state":"chan","args":{"n":"waytoolongvalue"},"value":"up"}"#.into(),
            INVALID,
        ),
        (
            r#"{"state":"chan","args":[1],"value":"up"}"#.into(),
            INVALID,
        ),
        // The shape of the message.
        (r#"{"state":7,"value":"zero"}"#.into(), MALFORMED),
        (r#"{"state":"level"}"#.into(), MALFORMED),
        (
            r#"{"state":"level","value":"zero","clear":true}"#.into(),
            MALFORMED,
        ),
        (r#"{"state":"level","clear":false}"#.into(), MALFORMED),
        (r#"{"state":"level","clear":"yes"}"#.into(), MALFORMED),
        (
            r#"{"type":"t.evt","state":"level","value":"zero"}"#.into(),
            MALFORMED,
        ),
        (big, TOO_LARGE),
    ];
    for (message, want) in cases {
        let (hub, r, p) = run(&message);
        let got = code_of(r);
        let short = &message[..message.len().min(90)];
        assert_eq!(got, want, "{short}");
        if want != ACCEPTED {
            // Nothing was stored, and the author is told why in the plugin's log.
            assert_eq!(state_of(&hub, "level"), (Status::Unknown, None), "{short}");
            assert!(
                p.log()
                    .entries()
                    .iter()
                    .any(|l| l.message.contains("rejected")),
                "{short} should leave a diagnostic"
            );
        }
    }
}

#[tokio::test]
async fn one_call_cannot_write_more_than_the_cap_of_distinct_instances() {
    // Instances "00" to "99" in a loop; the first 64 are accepted, the rest refused.
    let msg = r#"{"state":"chan","args":{"n":"00"},"value":"up"}"#;
    let tens = msg.find("\"00\"").unwrap() as u32 + 1 + EVENT_AT;
    let body = format!(
        "(global.set $calls (i32.const 0))
         (loop $l
           (i32.store8 (i32.const {tens}) (i32.add (i32.const 48) (i32.div_u (global.get $calls) (i32.const 10))))
           (i32.store8 (i32.const {ones}) (i32.add (i32.const 48) (i32.rem_u (global.get $calls) (i32.const 10))))
           {publish}
           (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
           (br_if $l (i32.lt_u (global.get $calls) (i32.const 100))))
         (call $respond (i32.const 0) (i32.const {ok})) (i32.const 0)",
        ones = tens + 1,
        publish = Wat::publish_code(msg),
        ok = OK.len(),
    );
    let wasm = Wat::new()
        .data(EVENT_AT, msg)
        .data(0, OK)
        .body(&body)
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    p.invoke(&ctx(ID, &EventBus::new()), "a", &json!({}))
        .unwrap();
    let known = hub
        .known()
        .into_iter()
        .filter(|(k, _)| k.provider() == ID)
        .count();
    assert_eq!(known, 64);
    assert!(p
        .log()
        .entries()
        .iter()
        .any(|l| l.message.contains("more than 64 states")));
}

#[tokio::test]
async fn the_hub_still_bounds_what_a_plugin_holds_over_many_calls() {
    // The gate bounds a call; the hub bounds the provider. Writes that get past a
    // call's cap (here, handed to the plugin directly) stop at the hub's limit.
    let hub = StateHub::new();
    let p = load_with_hub(
        Wat::new().reply(OK).build(),
        manifest(),
        Limits::default(),
        &hub,
    );
    let writes: Vec<Write> = (0..1100)
        .map(|n| Write {
            address: StateAddress::new(ID, "chan").with("n", format!("{n}").as_str()),
            value: Some(Scalar::text("up")),
        })
        .collect();
    p.apply_states(writes);
    let held = hub
        .known()
        .into_iter()
        .filter(|(k, _)| k.provider() == ID)
        .count();
    assert_eq!(held, crate::state::hub::MAX_INSTANCES_PER_PROVIDER);
    assert!(p
        .log()
        .entries()
        .iter()
        .any(|l| l.message.contains("too many instances")));
}

#[tokio::test]
async fn a_plugin_cannot_write_to_another_provider_even_if_it_tries_past_the_gate() {
    let hub = StateHub::new();
    hub.ensure_provider("agent-control", crate::agents::observables::definitions())
        .unwrap();
    let p = load_with_hub(
        Wat::new().reply(OK).build(),
        manifest(),
        Limits::default(),
        &hub,
    );
    let theirs = StateAddress::new("agent-control", "slot.status").with("slot", 1i64);
    p.apply_states(vec![Write {
        address: theirs.clone(),
        value: Some(Scalar::text("failed")),
    }]);
    assert_eq!(hub.read(&theirs.key()).status, Status::Unknown);
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn states_follow_the_plugin_through_stop_and_restart() {
    let hub = StateHub::new();
    let wasm = publishing(r#"{"state":"level","value":"high"}"#);
    let p = load_with_hub(wasm.clone(), manifest(), Limits::default(), &hub);
    let bus = EventBus::new();
    // Starts: every declared state is unknown.
    for id in ["level", "vol", "mute"] {
        assert_eq!(state_of(&hub, id), (Status::Unknown, None), "{id}");
    }
    assert_eq!(scoped(&hub, "a"), (Status::Unknown, None));
    // Publishes: known.
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(state_of(&hub, "level").1, Some(Scalar::text("high")));
    // Disabled: unavailable, with nothing kept.
    p.deactivate();
    assert_eq!(state_of(&hub, "level"), (Status::Unavailable, None));
    assert_eq!(scoped(&hub, "a"), (Status::Unavailable, None));
    // Returns: unknown, never the old value.
    p.activate(&wasm, &serde_json::Map::new());
    assert_eq!(p.status().0, PluginStatus::Enabled);
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    // Reports again.
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(state_of(&hub, "level").1, Some(Scalar::text("high")));
}

#[tokio::test]
async fn a_failure_makes_the_states_unavailable_and_a_reload_makes_them_unknown() {
    let hub = StateHub::new();
    let m = r#"{"state":"level","value":"low"}"#;
    // Publishes on the first call, traps on the second.
    let wasm = Wat::new()
        .data(EVENT_AT, m)
        .data(0, OK)
        .body(&format!(
            "(if (i32.eq (global.get $calls) (i32.const 1)) (then unreachable))
             (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
             {} (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(m),
            OK.len()
        ))
        .build();
    let p = load_with_hub(wasm.clone(), manifest(), Limits::default(), &hub);
    let bus = EventBus::new();
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    assert_eq!(state_of(&hub, "level").0, Status::Known);
    assert!(p.invoke(&ctx(ID, &bus), "a", &json!({})).is_err());
    assert_eq!(p.status().0, PluginStatus::Error);
    assert_eq!(state_of(&hub, "level"), (Status::Unavailable, None));
    // Reload (a new copy of the same module): unknown, not low.
    p.retire();
    let again = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    drop(again);
}

#[tokio::test]
async fn nothing_from_a_call_that_traps_is_stored() {
    let m = r#"{"state":"level","value":"high"}"#;
    let wasm = Wat::new()
        .data(EVENT_AT, m)
        .body(&format!("{} unreachable", Wat::publish_code(m)))
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    assert!(p
        .invoke(&ctx(ID, &EventBus::new()), "a", &json!({}))
        .is_err());
    assert_ne!(state_of(&hub, "level").0, Status::Known);
}

#[tokio::test]
async fn a_state_set_before_a_reported_failure_is_still_true() {
    // "At the maximum" is a fact about the world even when the action failed.
    let m = r#"{"state":"level","value":"high"}"#;
    let failed = r#"{"status":"failed","error":"at the maximum"}"#;
    let wasm = Wat::new()
        .data(EVENT_AT, m)
        .data(0, failed)
        .body(&format!(
            "{} (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(m),
            failed.len()
        ))
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    assert!(p
        .invoke(&ctx(ID, &EventBus::new()), "a", &json!({}))
        .is_err());
    assert_eq!(state_of(&hub, "level").1, Some(Scalar::text("high")));
}

#[tokio::test]
async fn a_retired_copy_cannot_write_into_the_next_one() {
    // The old copy had a call in flight when it was replaced. What it says when
    // that call ends is dropped: the new copy starts from unknown.
    let hub = StateHub::new();
    let old = load_with_hub(
        Wat::new().reply(OK).build(),
        manifest(),
        Limits::default(),
        &hub,
    );
    old.retire();
    let new = load_with_hub(
        Wat::new().reply(OK).build(),
        manifest(),
        Limits::default(),
        &hub,
    );
    old.apply_states(vec![Write {
        address: StateAddress::new(ID, "level"),
        value: Some(Scalar::text("high")),
    }]);
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    assert!(old
        .log()
        .entries()
        .iter()
        .any(|l| l.message.contains("dropped")));
    drop(new);
}

#[tokio::test]
async fn states_set_during_init_are_dropped_with_a_warning() {
    let m = r#"{"state":"level","value":"high"}"#;
    let wasm = Wat::new()
        .data(EVENT_AT, m)
        .init(&format!(
            "{} (call $respond (i32.const 1000) (i32.const {})) (return (i32.const 0))",
            Wat::publish_code(m),
            INIT_OK.len()
        ))
        .reply(OK)
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    assert_eq!(p.status().0, PluginStatus::Enabled);
    assert_eq!(state_of(&hub, "level"), (Status::Unknown, None));
    assert!(p
        .log()
        .entries()
        .iter()
        .any(|l| l.message.contains("during init")));
}

#[tokio::test]
async fn events_stay_events_and_follow_the_state_they_are_about() {
    // A plugin may publish both. The state is in the hub by the time anyone hears the event.
    let state = r#"{"state":"level","value":"high"}"#;
    let event = r#"{"type":"t.evt","payload":{"level":"high"}}"#;
    let wasm = Wat::new()
        .data(EVENT_AT, state)
        .data(2500, event)
        .data(0, OK)
        .body(&format!(
            "{}
             (drop (call $publish (i32.const 2500) (i32.const {})))
             (call $respond (i32.const 0) (i32.const {})) (i32.const 0)",
            Wat::publish_code(state),
            event.len(),
            OK.len()
        ))
        .build();
    let hub = StateHub::new();
    let p = load_with_hub(wasm, manifest(), Limits::default(), &hub);
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    p.invoke(&ctx(ID, &bus), "a", &json!({})).unwrap();
    let events = drain(&mut rx);
    assert_eq!(
        events.len(),
        1,
        "the event is an event; the state is not an event"
    );
    assert_eq!(events[0].kind, "t.evt");
    assert_eq!(state_of(&hub, "level").1, Some(Scalar::text("high")));
}

#[tokio::test]
async fn a_plugin_without_states_cannot_write_any() {
    // Schema 1 and 2 plugins declare none, so the same message is simply not allowed.
    let hub = StateHub::new();
    let m = r#"{"state":"level","value":"high"}"#;
    let p = load_with_hub(
        publishing(m),
        manifest_json(ID, &["a"], &[]),
        Limits::default(),
        &hub,
    );
    let r = p.invoke(&ctx(ID, &EventBus::new()), "a", &json!({"s": "x"}));
    assert_eq!(code_of(r), super::sandbox::publish_code::NOT_ALLOWED);
}
