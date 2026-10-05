//! The Accessibility starter: fewer, larger targets in colour-coded groups.

use serde_json::json;

use super::build::{ctl, g, http, open_page, plugin, shortcut, url};
use super::navigation::surface_switchers;
use super::palette::{BLUE, GRAY, GREEN, ORANGE, PURPLE, RED, YELLOW};
use crate::core::model::{Assignment, Control, ControlPos, Page, Rgb, Span, Surface};

fn big(
    pos: ControlPos,
    label: &str,
    icon: &str,
    color: Rgb,
    desc: &str,
    action: Assignment,
) -> Control {
    let mut c = ctl(pos, label, icon, color, action);
    c.span = Span { w: 2, h: 2 };
    c.description = desc.into();
    c
}

pub(super) fn accessibility() -> Surface {
    // Four columns x three rows of 2x2 targets, grouped by colour:
    // blue navigation, green editing, yellow view, orange audio, red help.
    let mut home = vec![
        big(
            g(0, 0),
            "HOME",
            "⌂",
            BLUE,
            "Go to the start of the page (Alt+Home).",
            shortcut(&["alt", "home"]),
        ),
        big(
            g(2, 0),
            "BACK",
            "←",
            BLUE,
            "Go back (Alt+Left).",
            shortcut(&["alt", "left"]),
        ),
        big(
            g(4, 0),
            "COPY",
            "⧉",
            GREEN,
            "Copy (Cmd/Ctrl+C).",
            shortcut(&["mod", "c"]),
        ),
        big(
            g(6, 0),
            "PASTE",
            "⎘",
            GREEN,
            "Paste (Cmd/Ctrl+V).",
            shortcut(&["mod", "v"]),
        ),
        big(
            g(0, 2),
            "ZOOM +",
            "＋",
            YELLOW,
            "Zoom in (Cmd/Ctrl+Plus).",
            shortcut(&["mod", "="]),
        ),
        big(
            g(2, 2),
            "ZOOM −",
            "－",
            YELLOW,
            "Zoom out (Cmd/Ctrl+Minus).",
            shortcut(&["mod", "-"]),
        ),
        big(
            g(4, 2),
            "SCROLL ↑",
            "▲",
            YELLOW,
            "Scroll up (Page Up).",
            shortcut(&["pageup"]),
        ),
        big(
            g(6, 2),
            "SCROLL ↓",
            "▼",
            YELLOW,
            "Scroll down (Page Down).",
            shortcut(&["pagedown"]),
        ),
        big(
            g(0, 4),
            "VOLUME +",
            "🔊",
            ORANGE,
            "Volume up.",
            plugin("system", "volumeUp", json!(null)),
        ),
        big(
            g(2, 4),
            "VOLUME −",
            "🔉",
            ORANGE,
            "Volume down.",
            plugin("system", "volumeDown", json!(null)),
        ),
        big(
            g(4, 4),
            "MUTE",
            "🔇",
            ORANGE,
            "Mute / unmute.",
            plugin("system", "toggleMute", json!(null)),
        ),
        big(
            g(6, 4),
            "HELP",
            "?",
            RED,
            "Open the help page: call, message, lights, music.",
            open_page("help"),
        ),
    ];
    home.extend(surface_switchers("accessibility"));

    let help = vec![
        big(
            g(0, 0),
            "CALL",
            "☎",
            GREEN,
            "Start a call with your phone app (tel:). Edit the number in the inspector.",
            url("tel:"),
        ),
        big(
            g(2, 0),
            "MESSAGE",
            "✉",
            BLUE,
            "Start a text message (sms:). Edit the recipient in the inspector.",
            url("sms:"),
        ),
        big(
            g(4, 0),
            "LIGHTS",
            "☀",
            YELLOW,
            "Template: edit the webhook URL in the inspector.",
            http(
                "POST",
                "http://homeassistant.local:8123/api/webhook/launchdeck-lights",
            ),
        ),
        big(
            g(6, 0),
            "MUSIC",
            "♪",
            PURPLE,
            "Play / pause media.",
            plugin("system", "playPause", json!(null)),
        ),
        big(
            g(0, 6),
            "BACK",
            "←",
            GRAY,
            "Return to the main accessibility page.",
            open_page("home"),
        ),
    ];

    Surface {
        id: "accessibility".into(),
        name: "Accessibility".into(),
        description: "Fewer, larger targets in colour-coded groups. A predictable, low-effort way to drive the computer.".into(),
        icon: Some("◎".into()),
        category: "accessibility".into(),
        pages: vec![
            Page { id: "home".into(), name: "Home".into(), controls: home },
            Page { id: "help".into(), name: "Help".into(), controls: help },
        ],
        context: None,
    }
}
