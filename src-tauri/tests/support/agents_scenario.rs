//! A real Agent Control state, captured from the real runtime, shared by the
//! `dump_agents` example (which writes `examples/agents-snapshot.json`) and
//! the contract test (which checks that file is still current).

use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::Value;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// Runs the demo agents until one waits for approval while another has failed,
/// then returns the agents snapshot exactly as the desktop UI receives it.
pub async fn capture() -> Value {
    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![],
        secrets: None,
    })
    .await;
    rt.demo_adapter().run_demo().expect("demo starts");
    for _ in 0..2000 {
        let snap = serde_json::to_value(rt.snapshot().agents).expect("serialise");
        let sessions = snap["sessions"].as_array().cloned().unwrap_or_default();
        let waiting = sessions.iter().any(|s| !s["pendingApproval"].is_null());
        let failed = sessions.iter().any(|s| s["status"] == "failed");
        if waiting && failed {
            return snap;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the demo never reached an approval and a failure at the same time");
}
