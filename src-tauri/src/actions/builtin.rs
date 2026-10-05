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
use super::params::{hidden_char, ParamDef, RefTarget, MAX_TEXT_VALUE};
use crate::core::model::Rgb;
use crate::plugins::external::capabilities::{Origin, Reach};
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
/// Characters of a redirect's target an error repeats.
const MAX_LOCATION_SHOWN: usize = 200;

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
                let command = shell_command(args).to_string();
                ctx.block_on(run_shell(&command))
            }
            other => Err(ActionError::UnknownAction {
                plugin: SHELL.into(),
                action: other.into(),
            }),
        }
    }
}

/// The command `shell.execute` runs: `command`, trimmed. An artifact's
/// review shows the same text, read here.
pub(crate) fn shell_command(args: &Value) -> &str {
    args.get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
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
            request_destination(url).map_err(|e| Issue::param("url", e.message()))?;
        }
        Ok(())
    }

    fn invoke(&self, _: &PluginContext, action: &str, args: &Value) -> Result<Value, ActionError> {
        match action {
            HTTP_REQUEST => http_request(http_request_of(args)),
            other => Err(ActionError::UnknownAction {
                plugin: HTTP.into(),
                action: other.into(),
            }),
        }
    }
}

/// A request as `http.request` sends it. No header is added beyond the
/// HTTP client's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpRequest {
    /// In capitals; `GET` when none is given.
    pub method: String,
    pub url: String,
    /// Absent when blank: a blank body is not sent.
    pub body: Option<String>,
}

/// How `http.request` reads its arguments. An artifact's review shows the
/// same request, read here.
pub(crate) fn http_request_of(args: &Value) -> HttpRequest {
    let text = |name: &str| {
        args.get(name)
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
    };
    HttpRequest {
        method: text("method").unwrap_or("GET").to_ascii_uppercase(),
        url: text("url").unwrap_or_default().to_string(),
        body: text("body").map(str::to_string),
    }
}

/// Where a request goes, as the HTTP client that sends it reads the
/// address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Destination {
    /// `http` or `https`.
    pub scheme: String,
    /// Lowercase; an IPv6 address in brackets.
    pub host: String,
    pub port: u16,
    /// As the plugin network broker classifies the host.
    reach: Reach,
}

impl Destination {
    /// Where a destination that is not on the internet is, in words.
    pub(crate) fn nearby(&self) -> Option<&'static str> {
        match self.reach {
            Reach::Loopback => Some("on this computer"),
            Reach::Local => Some("on your local network"),
            Reach::Internet => None,
        }
    }
}

/// The one reading of an `http.request` address (security finding SF-4),
/// by the runtime before it sends and by an artifact's review: the
/// destination as the HTTP client (`http::Uri`) reads it. Refused: an
/// address that is not http or https; that carries a user name or a
/// password; that holds a backslash, a blank or a hidden character; or
/// whose scheme, host or port a browser's parser (WHATWG, the `url` crate)
/// reads differently. What a person is shown is then where it goes.
pub(crate) fn request_destination(url: &str) -> Result<Destination, ActionError> {
    let refused = |why: &str| Err(ActionError::InvalidArgs(why.to_string()));
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return refused("URL must start with http:// or https://");
    }
    if url.contains('\\') {
        return refused("The address contains a backslash, which address readers disagree about.");
    }
    if url.chars().any(char::is_whitespace) {
        return refused("The address contains a blank, such as a space or a tab.");
    }
    if url.chars().any(hidden_char) {
        return refused("The address contains a character a person cannot see.");
    }
    let differ = "The address is read differently by the HTTP client and by a browser.";
    let (Ok(client), Ok(browser)) = (url.parse::<ureq::http::Uri>(), url::Url::parse(url)) else {
        return refused(differ);
    };
    let with_user = client.authority().is_some_and(|a| a.as_str().contains('@'));
    if with_user || !browser.username().is_empty() || browser.password().is_some() {
        return refused(
            "The address carries a user name or password, which LaunchDeck does not send.",
        );
    }
    let scheme = client.scheme_str().unwrap_or_default().to_ascii_lowercase();
    let host = client.host().unwrap_or_default().to_ascii_lowercase();
    let port = client
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let same = browser.scheme() == scheme
        && browser.host_str() == Some(host.as_str())
        && browser.port_or_known_default() == Some(port);
    if !same || host.is_empty() {
        return refused(differ);
    }
    // The broker's classification of the host (SF-5), which also refuses
    // what nothing may connect to (unspecified, broadcast, multicast).
    let Ok(origin) = Origin::of_url(&browser) else {
        return refused("The address names a host LaunchDeck sends no request to.");
    };
    Ok(Destination {
        scheme,
        host,
        port,
        reach: origin.reach(),
    })
}

fn http_request(request: HttpRequest) -> Result<Value, ActionError> {
    let HttpRequest { method, url, body } = request;
    let url = url.as_str();
    if !HTTP_METHODS.contains(&method.as_str()) {
        return Err(ActionError::InvalidArgs(format!(
            "unsupported HTTP method `{method}`"
        )));
    }
    request_destination(url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        // A redirect would send the request where no one reviewed (SF-5).
        .max_redirects(0)
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
    if (300..400).contains(&status) {
        let location: String = resp
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .unwrap_or("nowhere")
            .chars()
            .filter(|c| !hidden_char(*c))
            .take(MAX_LOCATION_SHOWN)
            .collect();
        return Err(ActionError::Failed(format!(
            "HTTP {status}: redirected to {location}, which is not followed"
        )));
    }
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
        let request =
            |method: &str, url: &str| http_request_of(&json!({ "method": method, "url": url }));
        assert!(http_request(request("TRACE", "http://127.0.0.1")).is_err());
        assert!(http_request(request("GET", "file:///etc/passwd")).is_err());
    }

    /// SF-4: one reading of an address, the HTTP client's, which a browser's
    /// parser must agree with on scheme, host and port.
    #[test]
    fn a_request_goes_only_where_both_parsers_agree() {
        let to = |url: &str| {
            let d = request_destination(url).unwrap_or_else(|e| panic!("{url}: {e}"));
            (d.scheme, d.host, d.port)
        };
        assert_eq!(
            to("https://Example.com/a?b=1"),
            ("https".into(), "example.com".into(), 443)
        );
        assert_eq!(
            to("http://127.0.0.1:8123/api"),
            ("http".into(), "127.0.0.1".into(), 8123)
        );
        assert_eq!(to("http://[::1]/x"), ("http".into(), "[::1]".into(), 80));
        for (url, why) in [
            ("ftp://example.com", "http:// or https://"),
            ("HTTPS://example.com", "http:// or https://"),
            ("https://user:pw@example.com/", "user name or password"),
            (
                "https://trusted.example@evil.example/",
                "user name or password",
            ),
            (
                "https://trusted.example:443@evil.example/",
                "user name or password",
            ),
            ("https://trusted.example\\@evil.example/", "backslash"),
            ("https://evil.example\\trusted.example/", "backslash"),
            ("https://example.com/a b", "blank"),
            ("https://example.com/\t", "blank"),
            ("https://example.com/\u{200B}", "cannot see"),
            ("https://example.com/\u{0}", "cannot see"),
            ("https://\u{430}pple.com/", "read differently"),
            ("http://0x7f.1/", "read differently"),
            ("http://2130706433/", "read differently"),
            ("https://", "read differently"),
        ] {
            let refused = request_destination(url).unwrap_err().message();
            assert!(refused.contains(why), "{url:?}: {refused}");
        }
        // The runtime refuses what the review would not show truthfully,
        // before any network use; so does the editor's check.
        let request = |url: &str| http_request_of(&json!({ "url": url }));
        assert!(http_request(request("https://trusted.example@127.0.0.1:9/")).is_err());
        let issue = HttpProvider
            .check(HTTP_REQUEST, &json!({ "url": "https://a\\@b/" }))
            .unwrap_err();
        assert_eq!(issue.param.as_deref(), Some("url"));
    }

    /// SF-5: the built-in request follows no redirect. A 3xx is a failure
    /// that names where it pointed, and nothing is sent there.
    #[test]
    fn a_redirect_is_a_failure_that_names_where_it_pointed() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = format!(
            "http://127.0.0.1:{}/admin",
            elsewhere.local_addr().unwrap().port()
        );
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let answer = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let served = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream.write_all(answer.as_bytes()).unwrap();
        });
        let refused = http_request(http_request_of(
            &json!({ "method": "POST", "url": format!("http://127.0.0.1:{port}/hook") }),
        ))
        .unwrap_err()
        .message();
        served.join().unwrap();
        assert!(
            refused.contains("HTTP 302") && refused.contains(&target),
            "{refused}"
        );
        assert!(refused.contains("not followed"), "{refused}");
        elsewhere.set_nonblocking(true).unwrap();
        assert!(elsewhere.accept().is_err(), "the redirect was followed");
    }

    /// Where a destination is, as the plugin network broker classifies a
    /// host; a host it lets nothing connect to is refused.
    #[test]
    fn a_destination_off_the_internet_says_where_it_is() {
        let nearby = |url: &str| request_destination(url).unwrap().nearby();
        assert_eq!(
            nearby("http://localhost:8123/api"),
            Some("on this computer")
        );
        assert_eq!(nearby("http://127.0.0.1/"), Some("on this computer"));
        assert_eq!(nearby("http://[::1]/"), Some("on this computer"));
        assert_eq!(
            nearby("http://192.168.1.20/"),
            Some("on your local network")
        );
        assert_eq!(
            nearby("http://homeassistant.local:8123/"),
            Some("on your local network")
        );
        assert_eq!(nearby("http://printer/"), Some("on your local network"));
        assert_eq!(nearby("https://example.com/"), None);
        for unroutable in [
            "http://0.0.0.0:8080/",
            "http://255.255.255.255/",
            "http://224.0.0.1/",
        ] {
            let refused = request_destination(unroutable).unwrap_err().message();
            assert!(refused.contains("no request"), "{unroutable}: {refused}");
        }
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
