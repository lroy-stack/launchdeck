//! The Productivity starter: everyday shortcuts, apps and media.

use serde_json::json;

use super::build::{demo_cmd, g, launch, plugin, shell, shortcut, tool_pad, url};
use super::navigation::surface_switchers;
use super::palette::{BLUE, CYAN, GRAY, GREEN, ORANGE, PINK, PURPLE, WHITE, YELLOW};
use crate::core::model::{Page, Surface};
use crate::platform::Platform;

pub(super) fn productivity(platform: Platform) -> Surface {
    let files_app = match platform {
        Platform::Linux => launch("xdg-open", &["."]),
        Platform::MacOs => launch("Finder", &[]),
        Platform::Windows => launch("explorer", &[]),
    };
    let screenshot = match platform {
        Platform::Linux => shell("gnome-screenshot --interactive"),
        Platform::MacOs => shortcut(&["cmd", "shift", "4"]),
        Platform::Windows => shortcut(&["win", "shift", "s"]),
    };
    let mut controls = vec![
        tool_pad(
            g(0, 0),
            "Browser",
            "◉",
            BLUE,
            "Open a web page.",
            url("https://www.google.com"),
        ),
        tool_pad(
            g(1, 0),
            "Mail",
            "✉",
            CYAN,
            "Open your mail app (mailto:).",
            url("mailto:"),
        ),
        tool_pad(
            g(2, 0),
            "Calendar",
            "▦",
            GREEN,
            "Open a web calendar.",
            url("https://calendar.google.com"),
        ),
        tool_pad(
            g(3, 0),
            "Files",
            "▤",
            ORANGE,
            "Open the file manager.",
            files_app,
        ),
        tool_pad(
            g(0, 1),
            "Copy",
            "⧉",
            WHITE,
            "Copy (Cmd/Ctrl+C).",
            shortcut(&["mod", "c"]),
        ),
        tool_pad(
            g(1, 1),
            "Paste",
            "⎘",
            WHITE,
            "Paste (Cmd/Ctrl+V).",
            shortcut(&["mod", "v"]),
        ),
        tool_pad(
            g(2, 1),
            "Undo",
            "↶",
            YELLOW,
            "Undo (Cmd/Ctrl+Z).",
            shortcut(&["mod", "z"]),
        ),
        tool_pad(
            g(3, 1),
            "Redo",
            "↷",
            YELLOW,
            "Redo (Cmd/Ctrl+Shift+Z).",
            shortcut(&["mod", "shift", "z"]),
        ),
        tool_pad(
            g(0, 2),
            "Screenshot",
            "▣",
            PINK,
            "Take a screenshot with your OS shortcut.",
            screenshot,
        ),
        tool_pad(
            g(1, 2),
            "Search",
            "⌕",
            PINK,
            "Open system search (Cmd/Ctrl+Space).",
            shortcut(&["mod", "space"]),
        ),
        tool_pad(
            g(2, 2),
            "Music",
            "♪",
            PURPLE,
            "Play / pause media.",
            plugin("system", "playPause", json!(null)),
        ),
        tool_pad(
            g(3, 2),
            "Do Not Disturb",
            "☾",
            GRAY,
            "Placeholder: wire this to your OS focus mode with a shell command.",
            demo_cmd(platform, 1, "dnd: demo only"),
        ),
    ];
    controls.extend(surface_switchers("productivity"));
    Surface {
        id: "productivity".into(),
        name: "Productivity".into(),
        description: "Everyday shortcuts, apps and media on physical pads.".into(),
        icon: Some("☑".into()),
        category: "productivity".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
        imported_from: None,
    }
}
