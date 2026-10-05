//! Actions: running an assignment for a pad or for the inspector's Test button,
//! the pending list in the runtime state, and the temporary looks (overlays) an
//! action or an event puts on a control.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{control_key, Origin, PendingAction, Runtime, StateEntry};
use crate::actions::builtin::NAVIGATION;
use crate::core::model::{Assignment, Control};
use crate::events::types;

const SUCCESS_RESET: Duration = Duration::from_millis(2500);
const ERROR_RESET: Duration = Duration::from_millis(4000);
/// How long the `failed` look stays on a control whose action failed.
const FAILED_RESET: Duration = Duration::from_millis(1500);
/// Looks a control may define for its own action: shown while it is in flight
/// and briefly after it failed. They sit over the state rules and end by themselves.
const PENDING_LOOK: &str = "pending";
const FAILED_LOOK: &str = "failed";

impl Runtime {
    /// Run an assignment as if a pad had triggered it (the inspector's Test
    /// button) and report how it went. The same events, the same checks.
    pub async fn run_action(self: &Arc<Self>, assignment: Assignment) -> Result<Value, String> {
        let (s, p) = {
            let i = self.lock();
            (i.active_surface.clone(), i.active_page.clone())
        };
        self.clone().run_action_async(s, p, None, assignment).await
    }

    pub(super) async fn run_action_async(
        self: Arc<Self>,
        surface_id: String,
        page_id: String,
        control: Option<Control>,
        action: Assignment,
    ) -> Result<Value, String> {
        let semantic = action.definition_id();
        let control_id = control.as_ref().map(|c| c.id.clone());
        let ctx = json!({ "action": semantic, "controlId": control_id, "surfaceId": surface_id, "pageId": page_id });
        let with = |extra: Value| {
            let mut v = ctx.clone();
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };

        if action.provider == NAVIGATION {
            let outcome = match self.engine.registry().prepare(&action) {
                Ok(prepared) => self.navigate(&prepared),
                Err(e) => Err(e.to_string()),
            };
            return match outcome {
                Ok(()) => {
                    self.publish("runtime", types::ACTION_COMPLETED, ctx);
                    Ok(Value::Null)
                }
                Err(e) => {
                    self.publish(
                        "runtime",
                        types::ACTION_FAILED,
                        with(json!({ "error": e.clone() })),
                    );
                    Err(e)
                }
            };
        }

        // A control with feedback rules gets its stable look from them. Its own
        // action only ever adds a temporary one: `pending` while it is in flight
        // and `failed` briefly after it failed, when the control defines them.
        // A control without rules keeps the automatic running/success/error looks.
        let auto = control.as_ref().filter(|c| c.feedback.is_empty());
        let key = control
            .as_ref()
            .map(|c| control_key(&surface_id, &page_id, &c.id));
        let has = |state: &str| auto.is_some_and(|c| c.states.contains_key(state));
        let defines = |state: &str| {
            control
                .as_ref()
                .is_some_and(|c| c.states.contains_key(state))
        };
        let pending_look = if has("running") {
            Some("running")
        } else if defines(PENDING_LOOK) {
            Some(PENDING_LOOK)
        } else {
            None
        };

        self.publish("runtime", types::ACTION_STARTED, ctx.clone());
        // From here until the provider answers, the action is pending, and the
        // runtime says so: in its state, and on the pad if it has a look for it.
        let run = self.begin_action(key.clone(), &semantic);
        let shown = match (&key, pending_look) {
            (Some(key), Some(look)) => {
                Some(self.set_overlay(key, Some(look), None, Origin::ACTION))
            }
            _ => None,
        };
        let outcome = self.engine.execute(&action).await;
        self.end_action(run);
        match outcome {
            Ok(result) => {
                self.publish(
                    "runtime",
                    types::ACTION_COMPLETED,
                    with(json!({ "result": result })),
                );
                if let Some(key) = &key {
                    if has("success") {
                        self.set_overlay(key, Some("success"), Some(SUCCESS_RESET), Origin::ACTION);
                    } else if auto.is_some() {
                        self.set_overlay(key, None, None, Origin::ACTION);
                    } else if let Some(token) = shown {
                        // Success leaves nothing behind: the state rules decide what the pad shows.
                        self.clear_overlay(key, token);
                    }
                }
                Ok(result)
            }
            Err(e) => {
                self.publish(
                    "runtime",
                    types::ACTION_FAILED,
                    with(json!({ "error": e.to_string() })),
                );
                if let Some(key) = &key {
                    if has("error") {
                        self.set_overlay(key, Some("error"), Some(ERROR_RESET), Origin::ACTION);
                    } else if defines(FAILED_LOOK) {
                        self.set_overlay(
                            key,
                            Some(FAILED_LOOK),
                            Some(FAILED_RESET),
                            Origin::ACTION,
                        );
                    } else if auto.is_some() {
                        self.set_overlay(key, None, None, Origin::ACTION);
                    } else if let Some(token) = shown {
                        self.clear_overlay(key, token);
                    }
                }
                Err(e.to_string())
            }
        }
    }

    /// Record that an action is in flight. The token ends it.
    fn begin_action(&self, control: Option<String>, action: &str) -> u64 {
        let token = self.next_token();
        self.lock().pending.insert(
            token,
            PendingAction {
                control,
                action: action.to_string(),
                since: crate::events::now_ms(),
            },
        );
        self.emit_state();
        token
    }

    fn end_action(&self, token: u64) {
        if self.lock().pending.remove(&token).is_some() {
            self.emit_state();
        }
    }

    /// Put a temporary look on a control, or take it off with `None`.
    /// Optionally it ends by itself after `reset_after`, unless something else
    /// replaced it in the meantime (tracked with a token).
    pub(super) fn set_overlay(
        self: &Arc<Self>,
        key: &str,
        look: Option<&str>,
        reset_after: Option<Duration>,
        origin: Origin,
    ) -> u64 {
        let token = self.next_token();
        {
            let mut inner = self.lock();
            match look {
                Some(name) => {
                    inner.overlay.insert(
                        key.to_string(),
                        StateEntry {
                            name: name.to_string(),
                            token,
                            origin,
                            until: reset_after.map(|after| Instant::now() + after),
                        },
                    );
                }
                None => {
                    inner.overlay.remove(key);
                }
            }
        }
        self.emit_state();
        if let (Some(after), Some(_)) = (reset_after, look) {
            let rt = self.clone();
            let key = key.to_string();
            self.handle.spawn(async move {
                tokio::time::sleep(after).await;
                let removed = {
                    let mut i = rt.lock();
                    if i.overlay.get(&key).is_some_and(|e| e.token == token) {
                        i.overlay.remove(&key);
                        true
                    } else {
                        false
                    }
                };
                if removed {
                    rt.emit_state();
                }
            });
        }
        token
    }

    /// Take a temporary look off, if it is still the one `token` put there.
    /// Something that replaced it in the meantime (an event's look) stays.
    fn clear_overlay(&self, key: &str, token: u64) {
        let removed = {
            let mut i = self.lock();
            if i.overlay.get(key).is_some_and(|e| e.token == token) {
                i.overlay.remove(key);
                true
            } else {
                false
            }
        };
        if removed {
            self.emit_state();
        }
    }
}
