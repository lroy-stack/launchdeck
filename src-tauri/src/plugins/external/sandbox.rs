//! The WebAssembly sandbox: bytes in, bytes out.
//!
//! This is the only file that touches Wasmtime. It knows nothing about
//! manifests, actions or the event bus, so another engine (for example an
//! interpreter on a platform that forbids JIT code) could replace it without
//! touching anything else.
//!
//! What a module can do is exactly what is linked here and nothing else:
//! `launchdeck.log`, `launchdeck.publish` and `launchdeck.respond`. `publish`
//! carries two kinds of message, an event (`{"type": ...}`) and a state
//! (`{"state": ...}`). A plugin that was given a network gate (its manifest
//! asks for the network) may also import `launchdeck.http_request` and
//! `launchdeck.http_response`, which hand a request to the host and read the
//! answer; the host decides whether anything is sent. The protocol is still
//! ABI 1: a module that imports neither is unaffected. No WASI, so no files,
//! clock, random, environment, sockets or processes exist inside. A module
//! that imports anything else does not load.
//!
//! Limits per call: a fuel budget (deterministic CPU), an epoch deadline (wall
//! clock), a memory cap, bounded stack, bounded request, response, event and
//! log sizes. Any host side failure is returned as a value; nothing panics.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use wasmtime::{
    AsContextMut, Caller, Config, Engine, Extern, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, Trap, TypedFunc,
};

use super::log::{clean_line, Level, Origin, PluginLog, MAX_LINE_BYTES};
use super::network::{
    code as net_code, refusal, NetGate, MAX_REQUESTS_PER_CALL, MAX_REQUEST_MESSAGE,
    NETWORK_BUDGET_PER_CALL,
};
use crate::events::external::valid_ident;
use crate::state::definition::Write;

/// Namespace of the host functions a module may import.
pub const HOST_MODULE: &str = "launchdeck";
const HOST_FUNCTIONS: &[&str] = &["log", "publish", "respond"];
/// Linked only for a plugin that was given a network gate.
const NETWORK_FUNCTIONS: &[&str] = &["http_request", "http_response"];

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
pub const MAX_EVENTS_PER_CALL: usize = 32;
/// A state message is a name, a few arguments and one scalar.
pub const MAX_STATE_BYTES: usize = 2 * 1024;
/// Distinct state instances one call may write. The hub bounds what a plugin
/// holds; this bounds what one call can ask for.
pub const MAX_STATES_PER_CALL: usize = 64;
pub const MAX_LOG_LINES_PER_CALL: u32 = 50;

/// Codes `launchdeck.publish` returns to the module.
pub mod publish_code {
    pub const ACCEPTED: i32 = 0;
    pub const MALFORMED: i32 = 1;
    pub const TOO_LARGE: i32 = 2;
    pub const TOO_MANY: i32 = 3;
    /// An event type or a state the manifest does not declare.
    pub const NOT_ALLOWED: i32 = 4;
    /// A declared state, but the arguments or the value do not fit its
    /// declaration (wrong type, an enum value it does not list, a scope it
    /// does not have).
    pub const INVALID: i32 = 5;
}

/// Why a state write was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The manifest declares no such state.
    NotDeclared,
    /// It does, and the write does not fit it.
    Invalid(String),
}

/// Decides whether a state write is allowed. The sandbox does not know what a
/// manifest is; whoever loads the module hands it the one thing it needs, which
/// is a way to ask "may this module write this state, and what is its
/// canonical address and value?".
pub trait StateGate: Send + Sync {
    fn check(&self, id: &str, args: &Value, value: Option<&Value>) -> Result<Write, Refusal>;
}

/// A gate that declares nothing: every state write is refused.
pub struct NoStates;

impl StateGate for NoStates {
    fn check(&self, _: &str, _: &Value, _: Option<&Value>) -> Result<Write, Refusal> {
        Err(Refusal::NotDeclared)
    }
}

/// How often the epoch advances. Wall-clock deadlines have this resolution.
const TICK: Duration = Duration::from_millis(25);
const MAX_WASM_STACK: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct Limits {
    /// Fuel per call. Roughly one unit per executed instruction.
    pub fuel: u64,
    /// Wall-clock budget per call.
    pub timeout: Duration,
    /// Largest linear memory the module may grow to.
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            fuel: 100_000_000,
            timeout: Duration::from_secs(2),
            memory_bytes: 16 * 1024 * 1024,
        }
    }
}

/// An event the module asked to publish during a call. Delivered by the caller
/// only if the call ends well.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEvent {
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug)]
pub struct CallOutput {
    pub response: Vec<u8>,
    pub events: Vec<PendingEvent>,
    /// States the module asked to write, checked against the manifest and
    /// merged so each instance appears once with its last value. Applied by the
    /// caller only if the call ends well.
    pub states: Vec<Write>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The instance can no longer be trusted (trap, limit, bad pointer, broken
    /// exports). The caller must drop it.
    Fatal(String),
    /// This call failed but the instance is intact (no response, oversized
    /// response, plugin reported an internal error).
    Rejected(String),
}

#[derive(Default)]
struct CallState {
    events: Vec<PendingEvent>,
    states: Vec<Write>,
    response: Option<Vec<u8>>,
    violation: Option<String>,
    log_lines: u32,
    log_overflow: bool,
    /// This call may use the network (every call but `init`).
    network: bool,
    http_requests: u32,
    /// Time this call has spent waiting on the network.
    http_spent: Duration,
    /// The answer to the last request, until the module reads it.
    http_answer: Option<Vec<u8>>,
}

struct HostState {
    limits: StoreLimits,
    log: Arc<PluginLog>,
    declared_events: Arc<HashSet<String>>,
    gate: Arc<dyn StateGate>,
    net: Option<Arc<dyn NetGate>>,
    /// Ticks one call may run for, to give back after a network wait.
    deadline_ticks: u64,
    call: CallState,
}

pub struct Sandbox {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    handle: TypedFunc<(i32, i32), i32>,
    limits: Limits,
}

// ---------------------------------------------------------------------------
// Engine and epoch ticker (shared by every plugin)
// ---------------------------------------------------------------------------

static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
/// Sandboxes alive. The ticker thread runs only while this is above zero.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static TICKER: AtomicBool = AtomicBool::new(false);

fn engine() -> Result<&'static Engine, String> {
    ENGINE
        .get_or_init(|| {
            let mut c = Config::new();
            c.consume_fuel(true)
                .epoch_interruption(true)
                .max_wasm_stack(MAX_WASM_STACK)
                // Threads, function references and exceptions are not compiled in
                // (no `threads` or `gc` feature), so they cannot be switched on.
                .wasm_gc(false)
                .wasm_tail_call(false)
                .wasm_relaxed_simd(false)
                .wasm_multi_memory(false)
                .wasm_memory64(false)
                .wasm_stack_switching(false)
                .wasm_custom_page_sizes(false);
            Engine::new(&c).map_err(|e| format!("could not start the WebAssembly engine: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn ensure_ticker(engine: &Engine) {
    if TICKER
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let engine = engine.clone();
    let spawned = std::thread::Builder::new()
        .name("plugin-epoch".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            engine.increment_epoch();
            if LIVE.load(Ordering::SeqCst) == 0 {
                TICKER.store(false, Ordering::SeqCst);
                // A sandbox may have appeared between the check and the store.
                if LIVE.load(Ordering::SeqCst) == 0
                    || TICKER
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                {
                    return;
                }
            }
        });
    if let Err(e) = spawned {
        // Fuel still bounds every call; only wall-clock deadlines are lost.
        TICKER.store(false, Ordering::SeqCst);
        log::error!("plugin watchdog thread could not start: {e}");
    }
}

/// True while the epoch ticker thread is running (for tests and diagnostics).
pub fn ticker_running() -> bool {
    TICKER.load(Ordering::SeqCst)
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Guest memory helpers
// ---------------------------------------------------------------------------

fn guest_memory(caller: &mut Caller<'_, HostState>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::format_err!("module exports no memory"))
}

/// Copy `len` bytes at `ptr` out of guest memory. Out of range is a trap: the
/// module handed the host a pointer it does not own.
fn read_guest(
    caller: &mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
    what: &str,
) -> wasmtime::Result<Vec<u8>> {
    let memory = guest_memory(caller)?;
    let data = memory.data(&*caller);
    let (start, len) = (ptr as u32 as usize, len as u32 as usize);
    let end = start
        .checked_add(len)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| wasmtime::format_err!("{what}: pointer out of bounds"))?;
    Ok(data[start..end].to_vec())
}

// ---------------------------------------------------------------------------
// Host functions
// ---------------------------------------------------------------------------

fn host_log(
    mut caller: Caller<'_, HostState>,
    level: i32,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<()> {
    // Look at no more than a line's worth, whatever length the module claims.
    let capped = (len as u32 as usize).min(MAX_LINE_BYTES * 2) as i32;
    let bytes = read_guest(&mut caller, ptr, capped, "log")?;
    let state = caller.data_mut();
    if state.call.log_lines >= MAX_LOG_LINES_PER_CALL {
        if !state.call.log_overflow {
            state.call.log_overflow = true;
            state.log.host(
                Level::Warn,
                &format!("more than {MAX_LOG_LINES_PER_CALL} log lines in one call; the rest are dropped"),
            );
        }
        return Ok(());
    }
    state.call.log_lines += 1;
    state.log.push(
        Level::from_guest(level),
        Origin::Plugin,
        &clean_line(&bytes),
    );
    Ok(())
}

fn reject_publish(state: &HostState, code: i32, why: &str) -> wasmtime::Result<i32> {
    state
        .log
        .host(Level::Warn, &format!("event rejected: {why}"));
    Ok(code)
}

fn host_publish(mut caller: Caller<'_, HostState>, ptr: i32, len: i32) -> wasmtime::Result<i32> {
    use publish_code::*;
    // Both kinds of message are small; the larger bound applies to events, the
    // smaller one to a state, once we know which one this is.
    if len as u32 as usize > MAX_EVENT_BYTES {
        return reject_publish(
            caller.data(),
            TOO_LARGE,
            &format!("larger than {MAX_EVENT_BYTES} bytes"),
        );
    }
    let bytes = read_guest(&mut caller, ptr, len, "publish")?;
    let state = caller.data_mut();
    let Ok(Value::Object(mut obj)) = serde_json::from_slice::<Value>(&bytes) else {
        return reject_publish(state, MALFORMED, "not a JSON object");
    };
    match (obj.contains_key("type"), obj.contains_key("state")) {
        (true, true) => reject_publish(
            state,
            MALFORMED,
            "a message is an event (`type`) or a state (`state`), not both",
        ),
        (false, true) => publish_state(state, obj, bytes.len()),
        _ => publish_event(state, &mut obj),
    }
}

fn publish_event(
    state: &mut HostState,
    obj: &mut serde_json::Map<String, Value>,
) -> wasmtime::Result<i32> {
    use publish_code::*;
    if state.call.events.len() >= MAX_EVENTS_PER_CALL {
        return reject_publish(
            state,
            TOO_MANY,
            &format!("more than {MAX_EVENTS_PER_CALL} events in one call"),
        );
    }
    // Anything the module says about the source is ignored: the host decides it.
    let Some(Value::String(kind)) = obj.remove("type") else {
        return reject_publish(state, MALFORMED, "missing string `type`");
    };
    if !valid_ident(&kind) || !state.declared_events.contains(&kind) {
        return reject_publish(
            state,
            NOT_ALLOWED,
            &format!(
                "event type `{}` is not declared in the manifest",
                clean_line(kind.as_bytes())
            ),
        );
    }
    let payload = obj.remove("payload").unwrap_or(Value::Null);
    state.call.events.push(PendingEvent { kind, payload });
    Ok(ACCEPTED)
}

/// `{"state": "level", "args": {...}, "value": "high"}` sets one state;
/// `{"state": "level", "args": {...}, "clear": true}` says it has no value now.
/// The provider is never in the message: it is the plugin, always.
fn publish_state(
    state: &mut HostState,
    mut obj: serde_json::Map<String, Value>,
    size: usize,
) -> wasmtime::Result<i32> {
    use publish_code::*;
    if size > MAX_STATE_BYTES {
        return reject_state(
            state,
            TOO_LARGE,
            &format!("a state message is larger than {MAX_STATE_BYTES} bytes"),
        );
    }
    let Some(Value::String(id)) = obj.remove("state") else {
        return reject_state(state, MALFORMED, "`state` must be a string");
    };
    let args = obj.remove("args").unwrap_or(Value::Null);
    let value = obj.remove("value");
    let clear = obj.remove("clear");
    if let Some(extra) = obj.keys().next() {
        return reject_state(
            state,
            MALFORMED,
            &format!(
                "unknown field `{}` (a state message has `state`, `args`, and `value` or `clear`)",
                clean_line(extra.as_bytes())
            ),
        );
    }
    let clearing = match (&value, &clear) {
        (Some(_), None) => false,
        (None, Some(Value::Bool(true))) => true,
        _ => {
            return reject_state(
                state,
                MALFORMED,
                "give either a `value` or `\"clear\": true`",
            )
        }
    };
    let write = match state
        .gate
        .check(&id, &args, value.as_ref().filter(|_| !clearing))
    {
        Ok(w) => w,
        Err(Refusal::NotDeclared) => {
            return reject_state(
                state,
                NOT_ALLOWED,
                &format!(
                    "state `{}` is not declared in the manifest",
                    clean_line(id.as_bytes())
                ),
            )
        }
        Err(Refusal::Invalid(why)) => {
            return reject_state(state, INVALID, &clean_line(why.as_bytes()))
        }
    };
    // One instance is one entry: the last write of this call wins.
    let key = write.address.key();
    let slot = state
        .call
        .states
        .iter()
        .position(|w| w.address.key() == key);
    match slot {
        Some(at) => state.call.states[at] = write,
        None if state.call.states.len() >= MAX_STATES_PER_CALL => {
            return reject_state(
                state,
                TOO_MANY,
                &format!("more than {MAX_STATES_PER_CALL} states in one call"),
            )
        }
        None => state.call.states.push(write),
    }
    Ok(ACCEPTED)
}

fn reject_state(state: &HostState, code: i32, why: &str) -> wasmtime::Result<i32> {
    state
        .log
        .host(Level::Warn, &format!("state rejected: {why}"));
    Ok(code)
}

fn host_respond(mut caller: Caller<'_, HostState>, ptr: i32, len: i32) -> wasmtime::Result<()> {
    if caller.data().call.response.is_some() || caller.data().call.violation.is_some() {
        caller.data_mut().call.violation = Some("responded more than once".into());
        return Ok(());
    }
    if len as u32 as usize > MAX_RESPONSE_BYTES {
        caller.data_mut().call.violation = Some(format!(
            "response is larger than {MAX_RESPONSE_BYTES} bytes"
        ));
        return Ok(());
    }
    let bytes = read_guest(&mut caller, ptr, len, "respond")?;
    caller.data_mut().call.response = Some(bytes);
    Ok(())
}

/// `http_request(ptr, len) -> i32`: hand one request to the host. The result
/// is the size of the answer, which is always there to read with
/// `http_response`: a refusal is an answer too. The host performs the request;
/// the module waits, and that wait is not counted against its own time.
fn host_http_request(
    mut caller: Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<i32> {
    let answer = if len as u32 as usize > MAX_REQUEST_MESSAGE {
        refusal(net_code::REQUEST_TOO_LARGE, "the request is too large")
    } else {
        let message = read_guest(&mut caller, ptr, len, "http_request")?;
        let state = caller.data_mut();
        let left = NETWORK_BUDGET_PER_CALL.saturating_sub(state.call.http_spent);
        match state.net.clone() {
            // Not reachable: the functions are only linked when there is a gate.
            None => refusal(net_code::PERMISSION_DENIED, "this plugin has no network access"),
            Some(_) if !state.call.network => refusal(
                net_code::INVALID_REQUEST,
                "the network is not available while a plugin starts; make requests from an action or from refresh",
            ),
            Some(_) if state.call.http_requests >= MAX_REQUESTS_PER_CALL => refusal(
                net_code::RATE_LIMITED,
                &format!("more than {MAX_REQUESTS_PER_CALL} requests in one call"),
            ),
            Some(_) if left.is_zero() => refusal(net_code::TIMEOUT, "this call has used up its time on the network"),
            Some(net) => {
                state.call.http_requests += 1;
                let started = Instant::now();
                let answer = net.request(&message, left);
                let state = caller.data_mut();
                state.call.http_spent += started.elapsed();
                // The wait was the server's, not the module's: its own clock starts again.
                let ticks = state.deadline_ticks;
                caller.as_context_mut().set_epoch_deadline(ticks);
                answer
            }
        }
    };
    let size = answer.len() as i32;
    caller.data_mut().call.http_answer = Some(answer);
    Ok(size)
}

/// `http_response(ptr, cap) -> i32`: copy the last answer into the module's
/// buffer. Returns its size, or -1 when there is none or it does not fit.
fn host_http_response(
    mut caller: Caller<'_, HostState>,
    ptr: i32,
    cap: i32,
) -> wasmtime::Result<i32> {
    let Some(answer) = caller.data_mut().call.http_answer.take() else {
        return Ok(-1);
    };
    if answer.len() > cap as u32 as usize {
        caller.data_mut().call.http_answer = Some(answer);
        return Ok(-1);
    }
    let memory = guest_memory(&mut caller)?;
    memory
        .write(&mut caller, ptr as u32 as usize, &answer)
        .map_err(|_| wasmtime::format_err!("http_response: pointer out of bounds"))?;
    Ok(answer.len() as i32)
}

// ---------------------------------------------------------------------------
// Loading and calling
// ---------------------------------------------------------------------------

fn describe(err: &wasmtime::Error, limits: &Limits) -> String {
    match err.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "used up its CPU budget for one call".to_string(),
        Some(Trap::Interrupt) => format!("did not finish within {} ms", limits.timeout.as_millis()),
        Some(Trap::UnreachableCodeReached) => "aborted (unreachable instruction)".to_string(),
        Some(Trap::StackOverflow) => "overflowed its stack".to_string(),
        Some(t) => format!("trapped: {t}"),
        None => format!("failed: {}", clean_line(err.to_string().as_bytes())),
    }
}

fn ticks_of(limits: &Limits) -> u64 {
    (limits.timeout.as_millis() / TICK.as_millis() + 1) as u64
}

fn arm(store: &mut Store<HostState>, limits: &Limits, network: bool) {
    let _ = store.set_fuel(limits.fuel);
    store.set_epoch_deadline(ticks_of(limits));
    store.data_mut().call = CallState {
        network,
        ..CallState::default()
    };
}

impl Sandbox {
    /// Compile, link and start a module, and check that it speaks `abi`.
    /// `net` is given only for a plugin whose manifest asks for the network;
    /// without it the network functions do not exist for the module.
    pub fn load(
        wasm: &[u8],
        abi: u32,
        declared_events: Arc<HashSet<String>>,
        gate: Arc<dyn StateGate>,
        net: Option<Arc<dyn NetGate>>,
        log: Arc<PluginLog>,
        limits: &Limits,
    ) -> Result<Sandbox, String> {
        let engine = engine()?;
        let module = Module::new(engine, wasm).map_err(|e| {
            format!(
                "not a valid WebAssembly module: {}",
                clean_line(e.to_string().as_bytes())
            )
        })?;
        // Deny by default, with a readable reason: anything that is not a
        // `launchdeck` function would be a request for system access.
        for import in module.imports() {
            let known = HOST_FUNCTIONS.contains(&import.name())
                || (net.is_some() && NETWORK_FUNCTIONS.contains(&import.name()));
            let allowed = import.module() == HOST_MODULE && known && import.ty().func().is_some();
            if !allowed
                && import.module() == HOST_MODULE
                && NETWORK_FUNCTIONS.contains(&import.name())
            {
                return Err(format!(
                    "the module imports `{}::{}`, but its manifest does not ask for the network (`capabilities.network`)",
                    import.module(),
                    import.name()
                ));
            }
            if !allowed {
                return Err(format!(
                    "the module imports `{}::{}`, which LaunchDeck does not provide (plugins have no system access)",
                    import.module(),
                    import.name()
                ));
            }
        }
        let mut linker: Linker<HostState> = Linker::new(engine);
        linker
            .func_wrap(HOST_MODULE, "log", host_log)
            .and_then(|l| l.func_wrap(HOST_MODULE, "publish", host_publish))
            .and_then(|l| l.func_wrap(HOST_MODULE, "respond", host_respond))
            .map_err(|e| e.to_string())?;
        if net.is_some() {
            linker
                .func_wrap(HOST_MODULE, "http_request", host_http_request)
                .and_then(|l| l.func_wrap(HOST_MODULE, "http_response", host_http_response))
                .map_err(|e| e.to_string())?;
        }

        let state = HostState {
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .table_elements(10_000)
                .instances(1)
                .memories(1)
                .tables(4)
                .build(),
            log,
            declared_events,
            gate,
            net,
            deadline_ticks: ticks_of(limits),
            call: CallState::default(),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        arm(&mut store, limits, false);

        // Count the sandbox before the first instruction can run.
        LIVE.fetch_add(1, Ordering::SeqCst);
        ensure_ticker(engine);
        let mut guard = LiveGuard(true);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("could not start the module: {}", describe(&e, limits)))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or("the module does not export `memory`")?;
        let abi_fn = instance
            .get_typed_func::<(), i32>(&mut store, "ld_abi")
            .map_err(|_| "the module does not export `ld_abi() -> i32`".to_string())?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "ld_alloc")
            .map_err(|_| "the module does not export `ld_alloc(len) -> ptr`".to_string())?;
        let handle = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "ld_handle")
            .map_err(|_| "the module does not export `ld_handle(ptr, len) -> i32`".to_string())?;
        let found = abi_fn
            .call(&mut store, ())
            .map_err(|e| format!("`ld_abi` {}", describe(&e, limits)))?;
        if found as u32 != abi {
            return Err(format!(
                "the manifest says protocol {abi} but the module reports protocol {found}"
            ));
        }
        // Loading worked: the Sandbox now owns the live count.
        guard.0 = false;
        Ok(Sandbox {
            store,
            memory,
            alloc,
            handle,
            limits: limits.clone(),
        })
    }

    /// Send one request and collect the response and the events it published.
    /// The module may use the network during it, if it has a gate.
    pub fn call(&mut self, request: &[u8]) -> Result<CallOutput, CallError> {
        self.call_with(request, true)
    }

    /// Like [`call`](Sandbox::call), with the network closed: for `init`,
    /// which runs while the plugin host is busy and must be quick.
    pub fn call_offline(&mut self, request: &[u8]) -> Result<CallOutput, CallError> {
        self.call_with(request, false)
    }

    fn call_with(&mut self, request: &[u8], network: bool) -> Result<CallOutput, CallError> {
        if request.is_empty() || request.len() > MAX_REQUEST_BYTES {
            return Err(CallError::Rejected(format!(
                "request is larger than {MAX_REQUEST_BYTES} bytes"
            )));
        }
        arm(&mut self.store, &self.limits, network);
        let len = request.len() as i32;
        let ptr = self
            .alloc
            .call(&mut self.store, len)
            .map_err(|e| CallError::Fatal(describe(&e, &self.limits)))?;
        let start = ptr as u32 as usize;
        let data = self.memory.data_mut(&mut self.store);
        let Some(target) = start
            .checked_add(request.len())
            .and_then(|end| data.get_mut(start..end))
        else {
            return Err(CallError::Fatal(
                "returned an allocation outside its own memory".into(),
            ));
        };
        target.copy_from_slice(request);
        let code = self
            .handle
            .call(&mut self.store, (ptr, len))
            .map_err(|e| CallError::Fatal(describe(&e, &self.limits)))?;
        let call = std::mem::take(&mut self.store.data_mut().call);
        if let Some(v) = call.violation {
            return Err(CallError::Rejected(v));
        }
        if code != 0 {
            return Err(CallError::Rejected(format!(
                "reported an internal error (code {code})"
            )));
        }
        match call.response {
            Some(response) => Ok(CallOutput {
                response,
                events: call.events,
                states: call.states,
            }),
            None => Err(CallError::Rejected("returned without a response".into())),
        }
    }
}

/// Undoes the live count if loading fails before the Sandbox exists.
struct LiveGuard(bool);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        if self.0 {
            LIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
