//! Context: the surface follows the application in front. The engine
//! decides; what it decides is shown through `activate`, like a person's choice.

use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use super::{ui, By, Inner, Runtime};
use crate::context::engine::World;
use crate::context::observer::Report;
use crate::context::rules::checked as checked_context;
use crate::context::status::status as context_status;
use crate::context::{ContextSettings, ContextStatus, Engine, Target};
use crate::core::observables as native;
use crate::state::Scalar;

impl Runtime {
    /// Give the engine what it needs to know about the rest of the runtime.
    pub(super) fn with_context<R>(
        inner: &mut Inner,
        f: impl FnOnce(&mut Engine, &World) -> R,
    ) -> R {
        let Inner {
            context,
            workspace,
            active_surface,
            active_page,
            ..
        } = inner;
        let world = World {
            settings: &workspace.context,
            workspace,
            surface: active_surface,
            page: active_page,
        };
        f(context, &world)
    }

    /// Activate what the engine decided, through the same path as a person's
    /// choice: persistence, events, the hub, the renderer and the devices all
    /// follow from there.
    pub(super) fn show_for_context(&self, target: Option<Target>) {
        let Some(target) = target else { return };
        if let Err(e) = self.activate(&target.surface_id, target.page_id.as_deref(), By::Context) {
            log::warn!("context: {e}");
        }
    }

    /// The observer reported something.
    pub(super) fn on_foreground(self: &Arc<Self>, report: Report) {
        match report {
            Report::Status(status) => self.lock().context.set_observer(status),
            Report::Seen(seen) => {
                let wait = self.lock().context.observe(seen, Instant::now());
                if let Some(deadline) = wait {
                    // It counts only if it is still in front when this fires.
                    let rt = self.clone();
                    self.handle.spawn(async move {
                        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                        rt.settle_context();
                    });
                }
            }
        }
        self.emit_context();
    }

    fn settle_context(&self) {
        let target = {
            let mut inner = self.lock();
            let now = Instant::now();
            Self::with_context(&mut inner, |engine, world| engine.settle(now, world))
        };
        self.show_for_context(target);
        self.emit_context();
    }

    /// Auto Context as it is now: settings, observer, application, decision.
    pub fn context(&self) -> ContextStatus {
        let inner = self.lock();
        context_status(&inner.context, &inner.workspace.context, &inner.workspace)
    }

    /// Replace the context settings (Auto on or off, the rules, the fallback).
    /// New rules are checked; the error says which one and why.
    pub fn update_context(&self, settings: ContextSettings) -> Result<ContextStatus, String> {
        let target = {
            let mut inner = self.lock();
            let settings = checked_context(&settings, &inner.workspace.context, &inner.workspace)
                .map_err(|e| e.to_string())?;
            let was_auto = inner.workspace.context.auto;
            inner.workspace.context = settings;
            self.persist(&inner);
            Self::with_context(&mut inner, |engine, world| {
                engine.settings_changed(was_auto, world)
            })
        };
        self.emit_workspace();
        self.show_for_context(target);
        self.emit_context();
        Ok(self.context())
    }

    /// Resume Auto: end a manual override and apply the current application's rule.
    pub fn resume_context(&self) -> ContextStatus {
        let target = {
            let mut inner = self.lock();
            Self::with_context(&mut inner, |engine, world| engine.resume(world))
        };
        self.show_for_context(target);
        self.emit_context();
        self.context()
    }

    pub(super) fn publish_context_mode(&self, mode: crate::context::Mode) {
        if let Err(e) = self
            .hub
            .set(&native::context_mode(), Scalar::text(mode.as_str()))
        {
            log::warn!("could not publish the context mode: {e}");
        }
    }

    /// Tell the UI and the hub, if anything about context changed.
    pub(super) fn emit_context(&self) {
        let status = self.context();
        {
            let mut sent = self.context_sent.lock().unwrap_or_else(|e| e.into_inner());
            if sent.as_ref() == Some(&status) {
                return;
            }
            *sent = Some(status.clone());
        }
        self.publish_context_mode(status.mode);
        self.emitter.emit(
            ui::CONTEXT,
            serde_json::to_value(status).unwrap_or(Value::Null),
        );
    }
}
