//! The Development starter: build, test and ship from the pads.

use super::build::{ctl, demo_cmd, g, launch, open_page, shell, url, with_state_visuals};
use super::navigation::{back_control, surface_switchers};
use super::palette::{BLUE, CYAN, GREEN, ORANGE, PINK, PURPLE, RED, YELLOW};
use crate::core::model::{Assignment, Control, ControlPos, Page, Rgb, Surface};
use crate::platform::Platform;

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

pub(super) fn development(platform: Platform) -> Surface {
    let mut home = vec![
        dev_pad(
            g(0, 0),
            "Terminal",
            "›_",
            CYAN,
            "Launch a terminal (placeholder command).",
            launch(
                match platform {
                    Platform::Linux => "x-terminal-emulator",
                    Platform::MacOs => "Terminal",
                    Platform::Windows => "wt",
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
                match platform {
                    Platform::MacOs => "Visual Studio Code",
                    Platform::Linux | Platform::Windows => "code",
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
            demo_cmd(platform, 1, "docker: demo only"),
        ),
        dev_pad(
            g(0, 1),
            "Tests",
            "✓",
            YELLOW,
            "Safe demo: sleeps 2s then succeeds. Pad pulses yellow, then turns green.",
            demo_cmd(platform, 2, "tests passed"),
        ),
        dev_pad(
            g(1, 1),
            "Build",
            "⚒",
            ORANGE,
            "Safe demo: sleeps 3s then succeeds.",
            demo_cmd(platform, 3, "build ok"),
        ),
        dev_pad(
            g(2, 1),
            "Dev Server",
            "⚡",
            GREEN,
            "Safe demo: sleeps 1s then succeeds.",
            demo_cmd(platform, 1, "dev server: demo only"),
        ),
        dev_pad(
            g(3, 1),
            "Deploy",
            "⇪",
            RED,
            "Safe demo: sleeps 2s then succeeds. Replace with your deploy script.",
            demo_cmd(platform, 2, "deploy: demo only"),
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
            demo_cmd(platform, 1, "git status: demo only"),
        ),
        dev_pad(
            g(1, 0),
            "Commit",
            "●",
            GREEN,
            "Safe demo. Replace with your commit script.",
            demo_cmd(platform, 1, "git commit: demo only"),
        ),
        dev_pad(
            g(2, 0),
            "Pull",
            "↓",
            BLUE,
            "Safe demo. Replace with `git pull`.",
            demo_cmd(platform, 1, "git pull: demo only"),
        ),
        dev_pad(
            g(3, 0),
            "Push",
            "↑",
            ORANGE,
            "Safe demo. Replace with `git push`.",
            demo_cmd(platform, 1, "git push: demo only"),
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
        context: None,
        imported_from: None,
    }
}
