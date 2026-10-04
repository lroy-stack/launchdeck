//! Starter surfaces. These ship with the app and explain the product at a glance.
//!
//! Safe by default: shell commands only `echo`/sleep, URLs are public pages,
//! and HTTP actions point at obvious placeholders the user must edit.

use serde_json::json;

use crate::core::model::*;

pub mod agent_control;

// Palette used by the starter surfaces (kept readable on RGB LEDs).
const BLUE: Rgb = Rgb::new(40, 110, 255);
const GREEN: Rgb = Rgb::new(30, 220, 90);
const YELLOW: Rgb = Rgb::new(255, 200, 30);
const PURPLE: Rgb = Rgb::new(170, 80, 255);
const WHITE: Rgb = Rgb::new(255, 255, 255);
const RED: Rgb = Rgb::new(255, 40, 50);
const ORANGE: Rgb = Rgb::new(255, 120, 20);
const CYAN: Rgb = Rgb::new(30, 210, 230);
const PINK: Rgb = Rgb::new(255, 80, 170);
const GRAY: Rgb = Rgb::new(70, 74, 90);

fn g(col: u8, row: u8) -> ControlPos {
    ControlPos::Grid { col, row }
}

fn ctl(pos: ControlPos, label: &str, icon: &str, color: Rgb, press: Assignment) -> Control {
    let mut c = Control::new(pos, label, color);
    c.icon = Some(icon.to_string());
    c.press = Some(press);
    c
}

fn url(u: &str) -> Assignment {
    plugin("system", "openUrl", json!({ "url": u }))
}

fn shortcut(keys: &[&str]) -> Assignment {
    plugin(
        "system",
        "keyboardShortcut",
        json!({ "keys": keys.join("+") }),
    )
}

fn launch(app: &str, args: &[&str]) -> Assignment {
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

fn shell(command: &str) -> Assignment {
    plugin("shell", "execute", json!({ "command": command }))
}

fn http(method: &str, url: &str) -> Assignment {
    plugin("http", "request", json!({ "method": method, "url": url }))
}

fn plugin(provider: &str, action: &str, args: serde_json::Value) -> Assignment {
    Assignment::new(provider, action, args)
}

fn open_surface(id: &str) -> Assignment {
    plugin("navigation", "openSurface", json!({ "surfaceId": id }))
}

fn open_page(id: &str) -> Assignment {
    plugin("navigation", "openPage", json!({ "pageId": id }))
}

/// Cross-platform harmless command that takes `secs` seconds then prints `msg`.
fn demo_cmd(secs: u32, msg: &str) -> Assignment {
    let command = if cfg!(windows) {
        format!("ping -n {} 127.0.0.1 >nul & echo {msg}", secs + 1)
    } else {
        format!("sleep {secs} && echo '{msg}'")
    };
    shell(&command)
}

/// Top-row buttons switch between surfaces on every surface.
fn surface_switchers(active: &str) -> Vec<Control> {
    let list = [
        ("agent-control", "Agents", "◉", PURPLE),
        ("development", "Dev", "⌘", CYAN),
        ("productivity", "Work", "☑", GREEN),
        ("smart-home", "Home", "⌂", ORANGE),
        ("accessibility", "Access", "◎", YELLOW),
    ];
    list.iter()
        .enumerate()
        .map(|(i, (id, label, icon, color))| {
            let mut c = ctl(
                ControlPos::Top { index: i as u8 },
                label,
                icon,
                *color,
                open_surface(id),
            );
            c.description = format!("Switch to the {label} surface.");
            if *id == active {
                c.visual = Visual::solid(WHITE);
            } else {
                c.visual = Visual::solid(color.scaled(0.35));
            }
            c
        })
        .collect()
}

fn with_state_visuals(mut c: Control) -> Control {
    // The state vocabulary the runtime also applies automatically to actions
    // on controls without feedback rules: running / success / error.
    c.states.insert("running".into(), Visual::pulse(YELLOW));
    c.states.insert("success".into(), Visual::solid(GREEN));
    c.states.insert("error".into(), Visual::flash(RED));
    c
}

fn back_control(pos: ControlPos, page: &str) -> Control {
    let mut c = ctl(pos, "Back", "←", GRAY, open_page(page));
    c.description = "Return to the previous page.".into();
    c
}

fn tool_pad(
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

// ---------------------------------------------------------------------------
// Development
// ---------------------------------------------------------------------------

fn dev_pad(
    pos: ControlPos,
    label: &str,
    icon: &str,
    color: Rgb,
    desc: &str,
    action: Assignment,
) -> Control {
    let mut c = with_state_visuals(ctl(pos, label, icon, color, action));
    c.description = desc.into();
    c
}

fn development() -> Surface {
    let mut home = vec![
        dev_pad(
            g(0, 0),
            "Terminal",
            "›_",
            CYAN,
            "Launch a terminal (placeholder command).",
            launch(
                if cfg!(target_os = "macos") {
                    "Terminal"
                } else if cfg!(windows) {
                    "wt"
                } else {
                    "x-terminal-emulator"
                },
                &[],
            ),
        ),
        dev_pad(
            g(1, 0),
            "VS Code",
            "⌘",
            BLUE,
            "Launch VS Code (`code` must be on your PATH, or the app name on macOS).",
            launch(
                if cfg!(target_os = "macos") {
                    "Visual Studio Code"
                } else {
                    "code"
                },
                &[],
            ),
        ),
        dev_pad(
            g(2, 0),
            "GitHub",
            "⎇",
            PURPLE,
            "Open GitHub.",
            url("https://github.com"),
        ),
        dev_pad(
            g(3, 0),
            "Docker",
            "▣",
            CYAN,
            "Placeholder: harmless echo. Replace with `docker compose up`.",
            demo_cmd(1, "docker: demo only"),
        ),
        dev_pad(
            g(0, 1),
            "Tests",
            "✓",
            YELLOW,
            "Safe demo: sleeps 2s then succeeds. Pad pulses yellow, then turns green.",
            demo_cmd(2, "tests passed"),
        ),
        dev_pad(
            g(1, 1),
            "Build",
            "⚒",
            ORANGE,
            "Safe demo: sleeps 3s then succeeds.",
            demo_cmd(3, "build ok"),
        ),
        dev_pad(
            g(2, 1),
            "Dev Server",
            "⚡",
            GREEN,
            "Safe demo: sleeps 1s then succeeds.",
            demo_cmd(1, "dev server: demo only"),
        ),
        dev_pad(
            g(3, 1),
            "Deploy",
            "⇪",
            RED,
            "Safe demo: sleeps 2s then succeeds. Replace with your deploy script.",
            demo_cmd(2, "deploy: demo only"),
        ),
        {
            let mut c = ctl(g(0, 2), "Git", "⑂", PINK, open_page("git"));
            c.description = "Open the Git page.".into();
            c
        },
        dev_pad(
            g(1, 2),
            "Failing Task",
            "✕",
            RED.scaled(0.5),
            "Demonstrates the error state: exits with code 1, pad flashes red.",
            shell("echo 'simulated failure' >&2; exit 1"),
        ),
    ];
    home.extend(surface_switchers("development"));

    let mut git = vec![
        dev_pad(
            g(0, 0),
            "Git Status",
            "≡",
            CYAN,
            "Safe demo: prints a message.",
            demo_cmd(1, "git status: demo only"),
        ),
        dev_pad(
            g(1, 0),
            "Commit",
            "●",
            GREEN,
            "Safe demo. Replace with your commit script.",
            demo_cmd(1, "git commit: demo only"),
        ),
        dev_pad(
            g(2, 0),
            "Pull",
            "↓",
            BLUE,
            "Safe demo. Replace with `git pull`.",
            demo_cmd(1, "git pull: demo only"),
        ),
        dev_pad(
            g(3, 0),
            "Push",
            "↑",
            ORANGE,
            "Safe demo. Replace with `git push`.",
            demo_cmd(1, "git push: demo only"),
        ),
        back_control(g(0, 7), "home"),
    ];
    git.extend(surface_switchers("development"));

    Surface {
        id: "development".into(),
        name: "Development".into(),
        description:
            "Build, test and ship. Pads pulse while a command runs, then turn green or flash red."
                .into(),
        icon: Some("⌘".into()),
        category: "development".into(),
        pages: vec![
            Page {
                id: "home".into(),
                name: "Home".into(),
                controls: home,
            },
            Page {
                id: "git".into(),
                name: "Git".into(),
                controls: git,
            },
        ],
        context: Some(ContextHint {
            foreground_apps: vec!["com.microsoft.VSCode".into(), "Code".into()],
        }),
    }
}

// ---------------------------------------------------------------------------
// Productivity
// ---------------------------------------------------------------------------

fn productivity() -> Surface {
    let files_app = if cfg!(target_os = "macos") {
        launch("Finder", &[])
    } else if cfg!(windows) {
        launch("explorer", &[])
    } else {
        launch("xdg-open", &["."])
    };
    let screenshot = if cfg!(target_os = "macos") {
        shortcut(&["cmd", "shift", "4"])
    } else if cfg!(windows) {
        shortcut(&["win", "shift", "s"])
    } else {
        shell("gnome-screenshot --interactive")
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
            demo_cmd(1, "dnd: demo only"),
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
    }
}

// ---------------------------------------------------------------------------
// Smart Home (template: needs the user's own webhooks)
// ---------------------------------------------------------------------------

fn smart_home() -> Surface {
    let hook = |name: &str| {
        http(
            "POST",
            &format!("http://homeassistant.local:8123/api/webhook/launchdeck-{name}"),
        )
    };
    let note = "Template: edit the webhook URL in the inspector. Until you do, pressing this shows an error state.";
    let mut controls = Vec::new();
    for (i, (label, icon, color, name)) in [
        ("Lights", "☀", YELLOW, "lights"),
        ("Movie", "▶", PURPLE, "movie"),
        ("Work", "⌨", BLUE, "work"),
        ("Warmer", "▲", ORANGE, "warmer"),
        ("Cooler", "▼", CYAN, "cooler"),
        ("Lock", "🔒", RED, "lock"),
    ]
    .iter()
    .enumerate()
    {
        let mut c = with_state_visuals(ctl(
            g((i % 3) as u8 * 2, (i / 3) as u8 * 2),
            label,
            icon,
            *color,
            hook(name),
        ));
        c.span = Span { w: 2, h: 2 };
        c.description = note.into();
        controls.push(c);
    }
    controls.extend(surface_switchers("smart-home"));
    Surface {
        id: "smart-home".into(),
        name: "Smart Home".into(),
        description: "Template for Home Assistant style webhooks. Edit the URLs to make it yours."
            .into(),
        icon: Some("⌂".into()),
        category: "smart-home".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
    }
}

// ---------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------

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

fn accessibility() -> Surface {
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

// ---------------------------------------------------------------------------

pub fn starter_surfaces() -> Vec<Surface> {
    vec![
        agent_control::agent_control(),
        development(),
        productivity(),
        smart_home(),
        accessibility(),
    ]
}

pub fn default_workspace() -> Workspace {
    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: starter_surfaces(),
        last_active_surface: Some(agent_control::SURFACE_ID.into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

/// Upgrade a v1 workspace in place: add the Agent Control surface (first, it
/// is the flagship) and a button to it in the free top-row slot of every
/// existing surface. Nothing the user made is removed or changed.
pub fn add_agent_control(ws: &mut Workspace) {
    ws.schema_version = SCHEMA_VERSION;
    if ws.surface(agent_control::SURFACE_ID).is_some() {
        return;
    }
    ws.surfaces.insert(0, agent_control::agent_control());
    for surface in ws.surfaces.iter_mut().skip(1) {
        for page in &mut surface.pages {
            let taken = page
                .controls
                .iter()
                .any(|c| c.pos == ControlPos::Top { index: 5 });
            let has_switchers = page
                .controls
                .iter()
                .any(|c| matches!(c.pos, ControlPos::Top { .. }));
            if taken || !has_switchers {
                continue;
            }
            let mut c = ctl(
                ControlPos::Top { index: 5 },
                "Agents",
                "◉",
                PURPLE,
                open_surface(agent_control::SURFACE_ID),
            );
            c.description = "Switch to the Agent Control surface.".into();
            c.visual = Visual::solid(PURPLE.scaled(0.35));
            page.controls.push(c);
        }
    }
}

/// An empty surface for "Build your own".
pub fn blank_surface(id: &str, name: &str) -> Surface {
    Surface {
        id: id.into(),
        name: name.into(),
        description: "A blank surface. Select a pad and give it an action.".into(),
        icon: Some("＋".into()),
        category: "custom".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls: surface_switchers(""),
        }],
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn starter_workspace_has_the_documented_surfaces() {
        let ws = default_workspace();
        let ids: Vec<&str> = ws.surfaces.iter().map(|s| s.id.as_str()).collect();
        for expected in [
            "agent-control",
            "development",
            "productivity",
            "smart-home",
            "accessibility",
        ] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
        assert_eq!(ws.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn every_page_is_well_formed() {
        for s in starter_surfaces() {
            assert!(!s.pages.is_empty(), "{} has no pages", s.id);
            for p in &s.pages {
                let mut seen_cells = HashSet::new();
                let mut seen_ids = HashSet::new();
                for c in &p.controls {
                    assert!(
                        seen_ids.insert(c.id.clone()),
                        "{}/{}: duplicate control id {}",
                        s.id,
                        p.id,
                        c.id
                    );
                    if let ControlPos::Grid { .. } = c.pos {
                        for cell in c.cells() {
                            assert!(
                                cell.0 < 8 && cell.1 < 8,
                                "{}/{}: {} outside 8x8",
                                s.id,
                                p.id,
                                c.label
                            );
                            assert!(
                                seen_cells.insert(cell),
                                "{}/{}: overlapping cell {:?}",
                                s.id,
                                p.id,
                                cell
                            );
                        }
                    }
                    assert!(
                        c.press.is_some(),
                        "{}/{}: {} has no action",
                        s.id,
                        p.id,
                        c.label
                    );
                }
            }
        }
    }

    #[test]
    fn navigation_targets_exist() {
        let ws = default_workspace();
        for s in &ws.surfaces {
            for p in &s.pages {
                for c in &p.controls {
                    let a = c.press.as_ref().unwrap();
                    if a.provider != "navigation" {
                        continue;
                    }
                    let arg = |k: &str| a.args.get(k).and_then(|v| v.as_str());
                    match a.action.as_str() {
                        "openSurface" => {
                            let surface_id = arg("surfaceId").unwrap();
                            let target = ws
                                .surface(surface_id)
                                .unwrap_or_else(|| panic!("{surface_id} missing"));
                            if let Some(pid) = arg("pageId") {
                                assert!(target.page(pid).is_some());
                            }
                        }
                        "openPage" => {
                            let page_id = arg("pageId").unwrap();
                            assert!(
                                s.page(page_id).is_some(),
                                "{}: page {page_id} missing",
                                s.id
                            );
                        }
                        other => panic!("unknown navigation action {other}"),
                    }
                }
            }
        }
    }

    #[test]
    fn accessibility_surface_uses_large_targets_and_few_controls() {
        let s = default_workspace()
            .surfaces
            .into_iter()
            .find(|s| s.id == "accessibility")
            .unwrap();
        let home = s.page("home").unwrap();
        let grid: Vec<_> = home
            .controls
            .iter()
            .filter(|c| matches!(c.pos, ControlPos::Grid { .. }))
            .collect();
        assert_eq!(grid.len(), 12);
        assert!(grid.iter().all(|c| c.span == Span { w: 2, h: 2 }));
    }

    #[test]
    fn blank_surface_is_valid() {
        let s = blank_surface("mine", "Mine");
        assert_eq!(s.pages.len(), 1);
        assert_eq!(s.category, "custom");
    }

    /// `examples/default-workspace.json` is the contract fixture the TypeScript
    /// schemas are tested against. Refresh it with:
    /// `cargo run --example dump_workspace > ../examples/default-workspace.json`
    #[test]
    fn contract_fixture_matches_the_default_workspace() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/default-workspace.json")).unwrap();
        assert_eq!(
            fixture,
            serde_json::to_value(default_workspace()).unwrap(),
            "examples/default-workspace.json is stale; regenerate it with the dump_workspace example"
        );
    }
}
