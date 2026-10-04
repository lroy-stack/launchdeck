//! Built-in providers that are not the System plugin: navigation, shell and HTTP.
//!
//! They register exactly like any other provider, which is what lets the
//! Action Library list them. What keeps `shell` and `http` safe is unchanged:
//! only a person configuring a pad (or the inspector's Test button) can cause
//! them to run. Installed plugins cannot register in these provider ids (their
//! ids need two dotted parts), and no plugin API invokes another provider.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::definition::{Issue, ProviderKind};
use super::params::{ParamDef, RefTarget, MAX_TEXT_VALUE};
use crate::core::model::Rgb;
use crate::plugins::{ActionDef, ActionError, Plugin, PluginContext, PluginManifest};

pub const NAVIGATION: &str = "navigation";
pub const OPEN_SURFACE: &str = "openSurface";
pub const OPEN_PAGE: &str = "openPage";
pub const SHELL: &str = "shell";
pub const SHELL_EXECUTE: &str = "execute";
pub const HTTP: &str = "http";
pub const HTTP_REQUEST: &str = "request";

const INDIGO: Rgb = Rgb::new(99, 102, 241);
const TEAL: Rgb = Rgb::new(20, 184, 166);
const SLATE: Rgb = Rgb::new(100, 116, 139);
const SHELL_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_COMMAND_LEN: usize = 4096;
const OUTPUT_LIMIT: usize = 4096;
const HTTP_METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

fn manifest(
    id: &str,
    name: &str,
    description: &str,
    actions: Vec<ActionDef>,
    permissions: &[&str],
) -> PluginManifest {
    PluginManifest {
        id: id.into(),
        name: name.into(),
        version: "0.1.0".into(),
        description: description.into(),
        kind: ProviderKind::Builtin,
        actions,
        states: vec![],
        permissions: permissions.iter().map(|p| (*p).to_string()).collect(),
        external: None,
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Declares the navigation actions. The runtime performs them, because moving
/// between surfaces needs runtime state; `invoke` is never the way they run.
pub struct NavigationProvider;

impl Plugin for NavigationProvider {
    fn manifest(&self) -> PluginManifest {
        manifest(
            NAVIGATION,
            "Navigation",
            "Move between surfaces and pages.",
            vec![
                ActionDef::new(
                    OPEN_SURFACE,
                    "Open surface",
                    "Switch to another surface, optionally on a given page.",
                )
                .category("Navigation")
                .keywords(&["switch", "go to", "profile", "layout", "change", "launch"])
                .param(ParamDef::reference("surfaceId", "Surface", RefTarget::Surface).required())
                .param(
                    ParamDef::reference("pageId", "Page", RefTarget::Page)
                        .scope("surfaceId")
                        .help("Leave empty to open its first page."),
                )
                .look("Go to", INDIGO, "↪"),
                ActionDef::new(
                    OPEN_PAGE,
                    "Open page",
                    "Go to another page of the surface you are on.",
                )
                .category("Navigation")
                .keywords(&["next", "previous", "tab", "switch", "folder"])
                .param(ParamDef::reference("pageId", "Page", RefTarget::Page).required())
                .look("Page", INDIGO, "▤"),
            ],
            &[],
        )
    }

    fn invoke(&self, _: &PluginContext, _: &str, _: &Value) -> Result<Value, ActionError> {
        Err(ActionError::Failed(
            "navigation is handled by the runtime".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

pub struct ShellProvider;

impl Plugin for ShellProvider {
    fn manifest(&self) -> PluginManifest {
        manifest(
            SHELL,
            "Shell command",
            "Run a command on this computer.",
            vec![ActionDef::new(
                SHELL_EXECUTE,
                "Run shell command",
                "Runs a command on your computer with your permissions, exactly as written, \
                 each time you press the pad.",
            )
            .category("Advanced")
            .keywords(&[
                "terminal",
                "script",
                "bash",
                "cli",
                "command",
                "run",
                "powershell",
            ])
            .privileged()
            .param(
                ParamDef::string("command", "Command")
                    .required()
                    .multiline()
                    .monospace()
                    .max_length(MAX_COMMAND_LEN)
                    .placeholder("git pull"),
            )
            .look("Shell", SLATE, "›_")],
            &["shell"],
        )
    }

    fn invoke(
        &self,
        ctx: &PluginContext,
        action: &str,
        args: &Value,
    ) -> Result<Value, ActionError> {
        match action {
            SHELL_EXECUTE => {
                let command = args
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                ctx.block_on(run_shell(&command))
            }
            other => Err(ActionError::UnknownAction {
                plugin: SHELL.into(),
                action: other.into(),
            }),
        }
    }
}

fn truncate(mut s: String) -> String {
    if s.len() > OUTPUT_LIMIT {
        let mut end = OUTPUT_LIMIT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

async fn run_shell(command: &str) -> Result<Value, ActionError> {
    let command = command.trim();
    if command.is_empty() {
        return Err(ActionError::InvalidArgs("command is empty".into()));
    }
    if command.len() > MAX_COMMAND_LEN {
        return Err(ActionError::InvalidArgs("command is too long".into()));
    }
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| ActionError::Failed(format!("could not start shell: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");

    let run = async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (_, _, status) = tokio::join!(
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err),
            child.wait()
        );
        (out, err, status)
    };
    let (out, err, status) = tokio::time::timeout(SHELL_TIMEOUT, run)
        .await
        .map_err(|_| {
            ActionError::Failed(format!(
                "command timed out after {}s",
                SHELL_TIMEOUT.as_secs()
            ))
        })?;
    let status = status.map_err(|e| ActionError::Failed(e.to_string()))?;
    let stdout = truncate(String::from_utf8_lossy(&out).trim().to_string());
    let stderr = truncate(String::from_utf8_lossy(&err).trim().to_string());
    if status.success() {
        Ok(json!({ "exitCode": 0, "stdout": stdout, "stderr": stderr }))
    } else {
        let code = status
            .code()
            .map_or("signal".to_string(), |c| c.to_string());
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(ActionError::Failed(format!("exit {code}: {detail}")))
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

pub struct HttpProvider;

impl Plugin for HttpProvider {
    fn manifest(&self) -> PluginManifest {
        manifest(
            HTTP,
            "HTTP request",
            "Send a web request from this computer.",
            vec![ActionDef::new(
                HTTP_REQUEST,
                "HTTP request",
                "Sends a request from this computer each time you press the pad. Only use \
                 addresses you trust.",
            )
            .category("Advanced")
            .keywords(&[
                "webhook", "api", "rest", "post", "get", "call", "deploy", "trigger",
            ])
            .privileged()
            .param(
                ParamDef::choice(
                    "method",
                    "Method",
                    &[
                        ("GET", "GET"),
                        ("POST", "POST"),
                        ("PUT", "PUT"),
                        ("PATCH", "PATCH"),
                        ("DELETE", "DELETE"),
                    ],
                )
                .default_value(json!("GET")),
            )
            .param(
                ParamDef::string("url", "URL")
                    .required()
                    .max_length(2048)
                    .placeholder("https://example.com/hook"),
            )
            .param(
                ParamDef::string("body", "Body")
                    .multiline()
                    .monospace()
                    .max_length(MAX_TEXT_VALUE)
                    .help("Optional. Sent exactly as written."),
            )
            .look("Request", TEAL, "↗")],
            &["network"],
        )
    }

    fn check(&self, action: &str, args: &Value) -> Result<(), Issue> {
        if action == HTTP_REQUEST {
            let url = args.get("url").and_then(Value::as_str).unwrap_or_default();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Issue::param(
                    "url",
                    "URL must start with http:// or https://",
                ));
            }
        }
        Ok(())
    }

    fn invoke(&self, _: &PluginContext, action: &str, args: &Value) -> Result<Value, ActionError> {
        match action {
            HTTP_REQUEST => {
                let method = args.get("method").and_then(Value::as_str).unwrap_or("GET");
                let url = args.get("url").and_then(Value::as_str).unwrap_or_default();
                let body = args.get("body").and_then(Value::as_str).map(str::to_string);
                http_request(method, url, body)
            }
            other => Err(ActionError::UnknownAction {
                plugin: HTTP.into(),
                action: other.into(),
            }),
        }
    }
}

fn http_request(method: &str, url: &str, body: Option<String>) -> Result<Value, ActionError> {
    let method = method.to_ascii_uppercase();
    if !HTTP_METHODS.contains(&method.as_str()) {
        return Err(ActionError::InvalidArgs(format!(
            "unsupported HTTP method `{method}`"
        )));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ActionError::InvalidArgs(
            "URL must start with http:// or https://".into(),
        ));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let req = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(url);
    let resp = match body {
        Some(b) => agent.run(
            req.body(b)
                .map_err(|e| ActionError::Failed(e.to_string()))?,
        ),
        None => agent.run(
            req.body(())
                .map_err(|e| ActionError::Failed(e.to_string()))?,
        ),
    }
    .map_err(|e| ActionError::Failed(format!("request failed: {e}")))?;
    let status = resp.status().as_u16();
    if status >= 400 {
        Err(ActionError::Failed(format!("HTTP {status}")))
    } else {
        Ok(json!({ "status": status }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;

    fn ctx() -> PluginContext {
        PluginContext::new("t", EventBus::new(), tokio::runtime::Handle::current())
    }

    /// Plugins run on a blocking thread, as the engine does.
    async fn call(
        p: impl Plugin + 'static,
        action: &'static str,
        args: Value,
    ) -> Result<Value, ActionError> {
        let ctx = ctx();
        tokio::task::spawn_blocking(move || p.invoke(&ctx, action, &args))
            .await
            .unwrap()
    }

    #[test]
    fn manifests_declare_privilege_and_parameters() {
        let shell = ShellProvider.manifest();
        assert!(shell.actions[0].privileged);
        assert!(shell.actions[0].params[0].required);
        let http = HttpProvider.manifest();
        assert!(http.actions[0].privileged);
        assert_eq!(http.actions[0].params.len(), 3);
        let nav = NavigationProvider.manifest();
        assert!(nav.actions.iter().all(|a| !a.privileged));
        for m in [shell, http, nav] {
            for a in &m.actions {
                crate::actions::params::validate_defs(&a.params).unwrap();
            }
        }
    }

    #[test]
    fn http_checks_the_address_before_any_network_use() {
        let bad = json!({ "method": "GET", "url": "file:///etc/passwd" });
        assert!(HttpProvider.check(HTTP_REQUEST, &bad).is_err());
        let ok = json!({ "method": "GET", "url": "http://127.0.0.1" });
        assert!(HttpProvider.check(HTTP_REQUEST, &ok).is_ok());
        assert!(http_request("TRACE", "http://127.0.0.1", None).is_err());
        assert!(http_request("GET", "file:///etc/passwd", None).is_err());
    }

    #[tokio::test]
    async fn navigation_is_not_executable_through_invoke() {
        let err = call(NavigationProvider, OPEN_PAGE, json!({ "pageId": "x" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("runtime"));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn shell_reports_output_and_failure() {
        let ok = call(
            ShellProvider,
            SHELL_EXECUTE,
            json!({ "command": "echo launchdeck" }),
        )
        .await
        .unwrap();
        assert_eq!(ok["stdout"], "launchdeck");
        let bad = call(
            ShellProvider,
            SHELL_EXECUTE,
            json!({ "command": "echo nope >&2; exit 3" }),
        )
        .await
        .unwrap_err();
        assert_eq!(bad, ActionError::Failed("exit 3: nope".into()));
        assert!(
            call(ShellProvider, SHELL_EXECUTE, json!({ "command": "  " }))
                .await
                .is_err()
        );
    }
}
