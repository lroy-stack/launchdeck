//! Input: what a pad press or release does. Live runs the pad's action (a long
//! press wins over the tap when the pad has one); Edit only reports the press.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::{control_key, Held, Mode, Runtime};
use crate::core::model::{Assignment, Control};
use crate::devices::{DeviceInput, InputKind};
use crate::events::types;

pub(super) const LONG_PRESS: Duration = Duration::from_millis(500);

impl Runtime {
    pub fn handle_input(self: &Arc<Self>, input: DeviceInput) {
        let mut inner = self.lock();
        let (surface_id, page_id) = (inner.active_surface.clone(), inner.active_page.clone());
        let Some(control) = inner
            .workspace
            .surface(&surface_id)
            .and_then(|s| s.page(&page_id))
            .and_then(|p| p.control_at(input.pos))
            .cloned()
        else {
            return;
        };
        let key = control_key(&surface_id, &page_id, &control.id);

        match input.kind {
            InputKind::Pressed { velocity } => {
                self.publish(
                    "device",
                    types::CONTROL_PRESSED,
                    json!({
                        "deviceId": input.device_id,
                        "surfaceId": surface_id,
                        "pageId": page_id,
                        "controlId": control.id,
                        "label": control.label,
                        "velocity": velocity,
                    }),
                );
                if inner.mode == Mode::Edit {
                    return;
                }
                if control.long_press.is_some() {
                    // The tap action is deferred to release so a long press can win.
                    let token = self.next_token();
                    inner.held.insert(
                        key.clone(),
                        Held {
                            token,
                            long_fired: false,
                        },
                    );
                    drop(inner);
                    let rt = self.clone();
                    self.handle.spawn(async move {
                        tokio::time::sleep(LONG_PRESS).await;
                        rt.fire_long_press(key, token, surface_id, page_id, control);
                    });
                } else if let Some(action) = control.press.clone() {
                    drop(inner);
                    self.trigger(surface_id, page_id, Some(control), action);
                }
            }
            InputKind::Released => {
                let Some(held) = inner.held.remove(&key) else {
                    return;
                };
                if held.long_fired || inner.mode == Mode::Edit {
                    return;
                }
                if let Some(action) = control.press.clone() {
                    drop(inner);
                    self.trigger(surface_id, page_id, Some(control), action);
                }
            }
        }
    }

    fn fire_long_press(
        self: &Arc<Self>,
        key: String,
        token: u64,
        surface_id: String,
        page_id: String,
        control: Control,
    ) {
        {
            let mut inner = self.lock();
            match inner.held.get_mut(&key) {
                Some(h) if h.token == token && !h.long_fired => h.long_fired = true,
                _ => return,
            }
        }
        if let Some(action) = control.long_press.clone() {
            self.trigger(surface_id, page_id, Some(control), action);
        }
    }

    fn trigger(
        self: &Arc<Self>,
        surface_id: String,
        page_id: String,
        control: Option<Control>,
        assignment: Assignment,
    ) {
        let rt = self.clone();
        self.handle.spawn(async move {
            let _ = rt
                .run_action_async(surface_id, page_id, control, assignment)
                .await;
        });
    }
}
