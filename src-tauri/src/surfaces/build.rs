//! Builders the starter surfaces are written with: positions, controls and
//! the actions their pads run.

use serde_json::json;

use super::palette::{GREEN, RED, YELLOW};
use crate::core::model::{Assignment, Control, ControlPos, Rgb, Visual};
use crate::platform::Platform;

pub(super) fn g(col: u8, row: u8) -> ControlPos {
    ControlPos::Grid { col, row }
}

pub(super) fn ctl(
    pos: ControlPos,
    label: &str,
    icon: &str,
    color: Rgb,
    press: Assignment,
) -> Control {
    let mut c = Control::new(pos, label, color);
    c.icon = Some(icon.to_string());
    c.press = Some(press);
    c
}

pub(super) fn url(u: &str) -> Assignment {
    plugin("system", "openUrl", json!({ "url": u }))
}

pub(super) fn shortcut(keys: &[&str]) -> Assignment {
    plugin(
        "system",
        "keyboardShortcut",
        json!({ "keys": keys.join("+") }),
    )
}

pub(super) fn launch(app: &str, args: &[&str]) -> Assignment {
    if args.is_empty() {
        plugin("system", "launchApplication", json!({ "app": app }))
    } else {
        plugin(
            "system",
            "launchApplication",
            json!({ "app": app, "args": args.join("\n") }),
        )
    }
}

pub(super) fn shell(command: &str) -> Assignment {
    plugin("shell", "execute", json!({ "command": command }))
}

pub(super) fn http(method: &str, url: &str) -> Assignment {
    plugin("http", "request", json!({ "method": method, "url": url }))
}

pub(super) fn plugin(provider: &str, action: &str, args: serde_json::Value) -> Assignment {
    Assignment::new(provider, action, args)
}

pub(super) fn open_surface(id: &str) -> Assignment {
    plugin("navigation", "openSurface", json!({ "surfaceId": id }))
}

pub(super) fn open_page(id: &str) -> Assignment {
    plugin("navigation", "openPage", json!({ "pageId": id }))
}

/// Harmless command that takes `secs` seconds then prints `msg`.
pub(super) fn demo_cmd(platform: Platform, secs: u32, msg: &str) -> Assignment {
    let command = match platform {
        Platform::Windows => format!("ping -n {} 127.0.0.1 >nul & echo {msg}", secs + 1),
        Platform::Linux | Platform::MacOs => format!("sleep {secs} && echo '{msg}'"),
    };
    shell(&command)
}

pub(super) fn with_state_visuals(mut c: Control) -> Control {
    // The state vocabulary the runtime also applies automatically to actions
    // on controls without feedback rules: running / success / error.
    c.states.insert("running".into(), Visual::pulse(YELLOW));
    c.states.insert("success".into(), Visual::solid(GREEN));
    c.states.insert("error".into(), Visual::flash(RED));
    c
}

pub(super) fn tool_pad(
    pos: ControlPos,
    label: &str,
    icon: &str,
    color: Rgb,
    desc: &str,
    action: Assignment,
) -> Control {
    let mut c = ctl(pos, label, icon, color.scaled(0.6), action);
    c.description = desc.into();
    c
}
