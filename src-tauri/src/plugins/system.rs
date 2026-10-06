//! The System plugin: semantic OS capabilities.
//!
//! Deliberately NOT here: arbitrary shell execution. `shell.execute` is a
//! core action configured by the user, never something a plugin can call.

use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};

use super::keyboard::{parse_combo, Combo, KeyKind, Keyboard, Named};
use super::{str_arg, ActionDef, ActionError, Plugin, PluginContext, PluginManifest};
use crate::actions::definition::{Issue, ProviderKind};
use crate::actions::params::ParamDef;
use crate::core::model::Rgb;
use crate::platform::Platform;

pub const ID: &str = "system";
/// The action that starts an application by name or path.
pub const LAUNCH_APPLICATION: &str = "launchApplication";

/// Schemes `system.openUrl` will hand to the OS. No `file:`, no custom
/// handlers: opening arbitrary schemes is how links turn into code execution.
const ALLOWED_SCHEMES: &[&str] = &["http", "https", "mailto", "tel", "sms", "facetime"];

pub fn validate_url(url: &str) -> Result<(), ActionError> {
    let scheme = url
        .split_once(':')
        .map(|(s, _)| s.to_ascii_lowercase())
        .ok_or_else(|| ActionError::InvalidArgs("URL must include a scheme".into()))?;
    if ALLOWED_SCHEMES.contains(&scheme.as_str()) && url.len() <= 2048 {
        Ok(())
    } else {
        Err(ActionError::InvalidArgs(format!(
            "scheme `{scheme}` is not allowed; allowed: {}",
            ALLOWED_SCHEMES.join(", ")
        )))
    }
}

/// Program and arguments used to launch an application on `platform`. No
/// shell is involved except `cmd /C start` on Windows (needed to resolve
/// registered apps), which is why [`launch_spec`] refuses what it would read.
pub fn launch_command(app: &str, args: &[String], platform: Platform) -> (String, Vec<String>) {
    match platform {
        Platform::MacOs => {
            let mut a = vec!["-a".to_string(), app.to_string()];
            if !args.is_empty() {
                a.push("--args".into());
                a.extend(args.iter().cloned());
            }
            ("open".into(), a)
        }
        Platform::Windows => {
            let mut a = vec![
                "/C".to_string(),
                "start".into(),
                String::new(),
                app.to_string(),
            ];
            a.extend(args.iter().cloned());
            ("cmd".into(), a)
        }
        Platform::Linux => (app.to_string(), args.to_vec()),
    }
}

pub struct SystemPlugin {
    keyboard: Arc<dyn Keyboard>,
}

impl SystemPlugin {
    pub fn new(keyboard: Arc<dyn Keyboard>) -> Self {
        Self { keyboard }
    }

    fn press_named(&self, key: Named) -> Result<Value, ActionError> {
        self.keyboard.press(&Combo {
            modifiers: vec![],
            key: KeyKind::Named(key),
        })?;
        Ok(Value::Null)
    }
}

/// Keys as people write them (`mod+shift+p`). A list is accepted too, which is
/// how this action was called before it had a settings form.
fn keys_of(args: &Value) -> Result<Vec<String>, ActionError> {
    match args.get("keys") {
        Some(Value::String(text)) => Ok(text
            .split('+')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(String::from)
            .collect()),
        Some(Value::Array(list)) => Ok(list
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()),
        _ => Err(ActionError::InvalidArgs("`keys` (text) is required".into())),
    }
}

/// An application to start and the arguments to give it, exactly as
/// `launchApplication` passes them on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub app: String,
    pub args: Vec<String>,
}

/// How `launchApplication` reads its arguments: `app`, a text that is not
/// blank, and `args`, either a text with one argument per line (each line
/// trimmed, empty lines left out) or a list of texts passed as they are,
/// empty ones included. Any other `args` is refused, and so is a NUL or a
/// line break anywhere, an application name a launcher would read as an
/// option, and on Windows any character its command interpreter reads
/// (`& | < > ^ % "`). The runtime starts what this
/// returns, and an artifact's review shows it, so the two can never read
/// one assignment differently.
pub fn launch_spec(args: &Value, platform: Platform) -> Result<LaunchSpec, ActionError> {
    let app = str_arg(args, "app")?.to_string();
    let refused = || ActionError::InvalidArgs("`args` is text, one argument per line".into());
    let args = match args.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|v| v.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(refused)?,
        Some(_) => return Err(refused()),
    };
    passable(&app, "This application name", platform)?;
    not_an_option(&app, platform)?;
    for arg in &args {
        passable(arg, "This argument", platform)?;
    }
    Ok(LaunchSpec { app, args })
}

/// An application name a launcher would read as one of its own options: a
/// leading `-` anywhere, and on Windows a leading `/`, which `start` reads
/// as `/D`, `/B`, `/MIN`... and then starts the next word instead.
fn not_an_option(app: &str, platform: Platform) -> Result<(), ActionError> {
    let app = app.trim_start();
    if app.starts_with('-') {
        return Err(ActionError::InvalidArgs(
            "This application name starts with a hyphen, which a launcher reads as an option."
                .into(),
        ));
    }
    if platform == Platform::Windows && app.starts_with('/') {
        return Err(ActionError::InvalidArgs(
            "This application name starts with a character the Windows start command reads as \
             an option."
                .into(),
        ));
    }
    Ok(())
}

/// Characters `cmd.exe` reads as more than text: they chain, redirect,
/// escape, expand or quote.
const WINDOWS_COMMAND_CHARS: [char; 7] = ['&', '|', '<', '>', '^', '%', '"'];

/// A NUL ends a C string, and a line break starts a new command or line
/// wherever a program reads lines; neither is ever passed on.
fn is_break(c: char) -> bool {
    matches!(
        c,
        '\0' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

fn passable(text: &str, what: &str, platform: Platform) -> Result<(), ActionError> {
    if text.chars().any(is_break) {
        return Err(ActionError::InvalidArgs(format!(
            "{what} contains a line break or a NUL character, which LaunchDeck will not pass to a program."
        )));
    }
    if platform == Platform::Windows && text.contains(WINDOWS_COMMAND_CHARS) {
        return Err(ActionError::InvalidArgs(format!(
            "{what} contains characters LaunchDeck will not pass to the Windows command interpreter."
        )));
    }
    Ok(())
}

const BLUE: Rgb = Rgb::new(47, 129, 247);
const GREEN: Rgb = Rgb::new(30, 180, 90);
const PURPLE: Rgb = Rgb::new(150, 90, 240);
const SLATE: Rgb = Rgb::new(100, 116, 139);

impl Plugin for SystemPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: ID.into(),
            name: "System".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Open links, launch apps, press shortcuts and control media volume."
                .into(),
            kind: ProviderKind::Builtin,
            actions: vec![
                ActionDef::new(
                    "openUrl",
                    "Open URL",
                    "Open a web page, mail, phone or message link in the default app.",
                )
                .category("Apps and links")
                .keywords(&[
                    "web", "browser", "link", "website", "page", "mail", "bookmark",
                ])
                .param(
                    ParamDef::string("url", "Link")
                        .required()
                        .max_length(2048)
                        .placeholder("https://example.com")
                        .help("http, https, mailto, tel, sms and facetime links only."),
                )
                .look("Open link", BLUE, "🔗"),
                ActionDef::new(
                    LAUNCH_APPLICATION,
                    "Launch application",
                    "Start an application by name or path.",
                )
                .category("Apps and links")
                .keywords(&["app", "program", "open", "start", "run", "executable"])
                .param(
                    ParamDef::string("app", "Application")
                        .required()
                        .max_length(512)
                        .placeholder("Visual Studio Code"),
                )
                .param(
                    ParamDef::string("args", "Arguments")
                        .multiline()
                        .monospace()
                        .max_length(2048)
                        .help("Optional. One argument per line."),
                )
                .look("Launch", PURPLE, "🚀"),
                ActionDef::new(
                    "keyboardShortcut",
                    "Keyboard shortcut",
                    "Press a key combination as if you typed it.",
                )
                .category("Keyboard")
                .keywords(&[
                    "keys", "hotkey", "press", "combo", "type", "copy", "paste", "macro",
                ])
                .param(
                    ParamDef::string("keys", "Keys")
                        .required()
                        .max_length(120)
                        .placeholder("mod+shift+p")
                        .help(
                            "Join keys with +. mod is Cmd on macOS and Ctrl elsewhere. Also: \
                             ctrl, alt, shift, enter, esc, tab, space, arrows, f1 to f12.",
                        ),
                )
                .look("Shortcut", GREEN, "⌨"),
                ActionDef::new("volumeUp", "Volume up", "Press the volume-up media key.")
                    .category("Media")
                    .keywords(&["louder", "sound", "audio"])
                    .look("Volume up", SLATE, "🔊"),
                ActionDef::new(
                    "volumeDown",
                    "Volume down",
                    "Press the volume-down media key.",
                )
                .category("Media")
                .keywords(&["quieter", "sound", "audio"])
                .look("Volume down", SLATE, "🔉"),
                ActionDef::new("toggleMute", "Toggle mute", "Press the mute media key.")
                    .category("Media")
                    .keywords(&["silence", "sound", "audio", "mute"])
                    .look("Mute", SLATE, "🔇"),
                ActionDef::new(
                    "playPause",
                    "Play / pause",
                    "Press the media play/pause key.",
                )
                .category("Media")
                .keywords(&["music", "song", "player", "resume", "stop"])
                .look("Play", SLATE, "⏯"),
            ],
            states: vec![],
            permissions: vec!["automation".into(), "accessibility".into()],
            external: None,
        }
    }

    fn check(&self, action: &str, args: &Value) -> Result<(), Issue> {
        match action {
            "openUrl" => {
                let url = args.get("url").and_then(Value::as_str).unwrap_or_default();
                validate_url(url).map_err(|e| Issue::param("url", e.message()))
            }
            "keyboardShortcut" => keys_of(args)
                .and_then(|keys| parse_combo(&keys, Platform::current() == Platform::MacOs))
                .map(|_| ())
                .map_err(|e| Issue::param("keys", e.message())),
            _ => Ok(()),
        }
    }

    fn invoke(
        &self,
        _ctx: &PluginContext,
        action: &str,
        args: &Value,
    ) -> Result<Value, ActionError> {
        match action {
            "openUrl" => {
                let url = str_arg(args, "url")?;
                validate_url(url)?;
                open::that_detached(url)
                    .map_err(|e| ActionError::Failed(format!("could not open URL: {e}")))?;
                Ok(Value::Null)
            }
            LAUNCH_APPLICATION => {
                let LaunchSpec { app, args: extra } = launch_spec(args, Platform::current())?;
                let (program, argv) = launch_command(&app, &extra, Platform::current());
                let mut child = Command::new(&program)
                    .args(&argv)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| ActionError::Failed(format!("could not launch `{app}`: {e}")))?;
                // Reap the child without blocking the runtime.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                Ok(json!({ "launched": app }))
            }
            "keyboardShortcut" => {
                let keys = keys_of(args)?;
                let combo = parse_combo(&keys, Platform::current() == Platform::MacOs)?;
                self.keyboard.press(&combo)?;
                Ok(Value::Null)
            }
            "volumeUp" => self.press_named(Named::VolumeUp),
            "volumeDown" => self.press_named(Named::VolumeDown),
            "toggleMute" => self.press_named(Named::VolumeMute),
            "playPause" => self.press_named(Named::MediaPlayPause),
            other => Err(ActionError::UnknownAction {
                plugin: ID.into(),
                action: other.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Combo>>);
    impl Keyboard for Recorder {
        fn press(&self, combo: &Combo) -> Result<(), ActionError> {
            self.0.lock().unwrap().push(combo.clone());
            Ok(())
        }
    }

    fn ctx() -> PluginContext {
        PluginContext::new(
            ID,
            EventBus::new(),
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .handle()
                .clone(),
        )
    }

    #[test]
    fn url_policy_blocks_dangerous_schemes() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("mailto:me@example.com").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("ms-msdt:/id").is_err());
        assert!(validate_url("no-scheme").is_err());
    }

    #[test]
    fn launch_commands_per_os() {
        assert_eq!(
            launch_command("code", &["."].map(String::from), Platform::Linux),
            ("code".to_string(), vec![".".to_string()])
        );
        let (p, a) = launch_command("Safari", &[], Platform::MacOs);
        assert_eq!(
            (p.as_str(), a),
            ("open", vec!["-a".to_string(), "Safari".to_string()])
        );
        assert_eq!(launch_command("code", &[], Platform::Windows).0, "cmd");
    }

    #[test]
    fn launch_arguments_are_lines_or_a_list_and_nothing_else() {
        let spec = |args: Value| launch_spec(&args, Platform::Linux);
        let ok = |app: &str, args: &[&str]| {
            Ok(LaunchSpec {
                app: app.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
            })
        };
        assert_eq!(spec(json!({ "app": "code" })), ok("code", &[]));
        assert_eq!(
            spec(json!({ "app": "code", "args": null })),
            ok("code", &[])
        );
        assert_eq!(
            spec(json!({ "app": "code", "args": " --wait \n\n . \r\n" })),
            ok("code", &["--wait", "."])
        );
        // A list is passed as it is, empty entries included.
        assert_eq!(
            spec(json!({ "app": "code", "args": ["", " ", "a b"] })),
            ok("code", &["", " ", "a b"])
        );
        for refused in [
            json!({ "app": "code", "args": 1 }),
            json!({ "app": "code", "args": true }),
            json!({ "app": "code", "args": { "a": "b" } }),
            json!({ "app": "code", "args": ["--x", 1] }),
            json!({ "app": " " }),
            json!({ "app": 7 }),
            json!({}),
        ] {
            assert!(
                matches!(spec(refused.clone()), Err(ActionError::InvalidArgs(_))),
                "{refused}"
            );
        }
        // The runtime refuses it before it starts anything.
        let plugin = SystemPlugin::new(Arc::new(Recorder::default()));
        assert!(matches!(
            plugin.invoke(
                &ctx(),
                LAUNCH_APPLICATION,
                &json!({ "app": "code", "args": [1] })
            ),
            Err(ActionError::InvalidArgs(_))
        ));
    }

    #[test]
    fn a_launch_never_passes_a_nul_or_a_line_break() {
        let breaks = [
            "\0", "\n", "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
        ];
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            for b in breaks {
                for args in [
                    json!({ "app": format!("calc{b}x") }),
                    json!({ "app": "calc", "args": [format!("a{b}b")] }),
                ] {
                    assert!(
                        matches!(
                            launch_spec(&args, platform),
                            Err(ActionError::InvalidArgs(_))
                        ),
                        "{platform:?} {args}"
                    );
                }
            }
            // In the text form a line break separates arguments; a lone
            // carriage return inside a line does not.
            assert_eq!(
                launch_spec(&json!({ "app": "calc", "args": "a\nb" }), platform)
                    .unwrap()
                    .args,
                ["a", "b"]
            );
            assert!(launch_spec(&json!({ "app": "calc", "args": "a\rb" }), platform).is_err());
        }
    }

    /// Windows starts an application through `cmd /C start`, so a character
    /// the command interpreter reads would chain commands there. Not run on
    /// Windows: the decision is tested here by passing the platform.
    #[test]
    fn windows_refuses_what_its_command_interpreter_reads() {
        for meta in ['&', '|', '<', '>', '^', '%', '"'] {
            let app = json!({ "app": format!("calc {meta} whoami") });
            let arg = json!({ "app": "calc", "args": [format!("a{meta}b")] });
            let lines = json!({ "app": "calc", "args": format!("--x\na{meta}b") });
            for args in [&app, &arg, &lines] {
                let refused = launch_spec(args, Platform::Windows).unwrap_err();
                assert!(
                    refused
                        .message()
                        .ends_with("LaunchDeck will not pass to the Windows command interpreter."),
                    "{args}: {refused}"
                );
                for other in [Platform::Linux, Platform::MacOs] {
                    assert!(launch_spec(args, other).is_ok(), "{other:?} {args}");
                }
            }
            assert_eq!(
                launch_spec(&app, Platform::Windows).unwrap_err().message(),
                "This application name contains characters LaunchDeck will not pass to the \
                 Windows command interpreter."
            );
        }
        // Without such a character, the same launch is fine on Windows.
        assert!(launch_spec(
            &json!({ "app": "Visual Studio Code", "args": "--new-window\nC:\\src" }),
            Platform::Windows
        )
        .is_ok());
        // The runtime refuses it before it starts anything.
        let plugin = SystemPlugin::new(Arc::new(Recorder::default()));
        assert!(matches!(
            plugin.invoke(&ctx(), LAUNCH_APPLICATION, &json!({ "app": "calc\0x" })),
            Err(ActionError::InvalidArgs(_))
        ));
    }

    /// `cmd /C start "" /D C:\x evil.exe` reads /D as start's own option and
    /// starts evil.exe, so on Windows an application never starts with `/`;
    /// on no system does one start with `-`, which a launcher would read as
    /// an option. Not run on Windows: tested by passing the platform.
    #[test]
    fn an_application_is_never_read_as_an_option() {
        let start_option = json!({ "app": "/D", "args": ["C:\\x", "evil.exe"] });
        let refused = launch_spec(&start_option, Platform::Windows).unwrap_err();
        assert_eq!(
            refused.message(),
            "This application name starts with a character the Windows start command reads \
             as an option."
        );
        for other in [Platform::Linux, Platform::MacOs] {
            assert!(launch_spec(&start_option, other).is_ok(), "{other:?}");
        }
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            let refused = launch_spec(&json!({ "app": "-x" }), platform).unwrap_err();
            assert_eq!(
                refused.message(),
                "This application name starts with a hyphen, which a launcher reads as an \
                 option.",
                "{platform:?}"
            );
            // Further in, or in an argument, both are text.
            assert!(launch_spec(&json!({ "app": "a-b", "args": ["-x", "/y"] }), platform).is_ok());
        }
    }

    #[test]
    fn shortcut_and_media_keys_go_through_the_keyboard_backend() {
        let rec = Arc::new(Recorder::default());
        let plugin = SystemPlugin::new(rec.clone());
        plugin
            .invoke(
                &ctx(),
                "keyboardShortcut",
                &json!({ "keys": ["ctrl", "c"] }),
            )
            .unwrap();
        plugin.invoke(&ctx(), "volumeUp", &Value::Null).unwrap();
        plugin.invoke(&ctx(), "toggleMute", &Value::Null).unwrap();
        let calls = rec.0.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].key, KeyKind::Char('c'));
        assert_eq!(calls[1].key, KeyKind::Named(Named::VolumeUp));
        assert_eq!(calls[2].key, KeyKind::Named(Named::VolumeMute));
    }

    #[test]
    fn bad_arguments_and_unknown_actions_are_errors() {
        let plugin = SystemPlugin::new(Arc::new(Recorder::default()));
        assert!(matches!(
            plugin.invoke(&ctx(), "openUrl", &json!({})),
            Err(ActionError::InvalidArgs(_))
        ));
        assert!(matches!(
            plugin.invoke(&ctx(), "keyboardShortcut", &json!({})),
            Err(ActionError::InvalidArgs(_))
        ));
        assert!(matches!(
            plugin.invoke(&ctx(), "nope", &Value::Null),
            Err(ActionError::UnknownAction { .. })
        ));
    }

    #[test]
    fn checks_speak_to_a_person_and_name_their_setting() {
        let plugin = SystemPlugin::new(Arc::new(Recorder::default()));
        let issue = plugin
            .check("openUrl", &json!({ "url": "ftp://nope" }))
            .unwrap_err();
        assert_eq!(issue.param.as_deref(), Some("url"));
        assert!(
            issue.message.starts_with("scheme `ftp` is not allowed"),
            "{}",
            issue.message
        );
        let issue = plugin
            .check("keyboardShortcut", &json!({ "keys": "mod+notakey" }))
            .unwrap_err();
        assert_eq!(issue.param.as_deref(), Some("keys"));
        assert!(
            !issue.message.starts_with("invalid arguments"),
            "{}",
            issue.message
        );
    }

    #[test]
    fn manifest_declares_permissions_and_no_shell_capability() {
        let m = SystemPlugin::new(Arc::new(Recorder::default())).manifest();
        assert!(m.actions.iter().all(|a| !a.id.contains("shell")));
        assert!(m.permissions.contains(&"automation".to_string()));
    }
}
