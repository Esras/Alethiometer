//! OpenShell (NVIDIA/OpenShell): another place a local ACP agent can run. The
//! harness's adapter runs inside an OpenShell sandbox, on a local or a remote
//! gateway, and its stdio comes back through `openshell sandbox exec`, which
//! the agent host runs like any adapter. Approvals, the transcript, Stop and
//! resuming after a restart are the local session's (`acp.rs`).
//!
//! The OpenShell CLI owns the gateways, their sign-in and the providers that
//! hold credentials; Alethiometer only runs it (`agents.openshell.command`).
//! Built from OpenShell's source at `67adcf1` (2026-10-08) and its docs; not
//! run against a gateway here.
//!
//! The sandbox is the boundary, so the agent's own permission prompts are
//! turned off inside it: Claude Code runs in its `auto` mode (it decides what
//! to allow, and anything it still asks comes to you); any other agent is
//! switched to a mode that skips its checks if it offers one, and its
//! permission requests are answered with its own allow option.

use super::model::{PermOption, Sandbox};
use crate::error::{msg, Result};
use crate::settings::Harness;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Listing gateways or sandboxes, starting a stopped one, asking its working directory.
const QUICK: Duration = Duration::from_secs(120);
/// Creating a sandbox: the gateway may pull the image first, then the directory is uploaded.
const CREATE: Duration = Duration::from_secs(20 * 60);

/// Where the CLI is, on the login shell's PATH.
pub fn cli(command: &str) -> Option<std::path::PathBuf> {
    super::acp::which(command, &super::acp::login_path())
}

/// `-g <gateway>`, unless it's the CLI's active one.
fn gateway_flag(gateway: &str) -> Vec<String> {
    if gateway.is_empty() {
        vec![]
    } else {
        vec!["-g".into(), gateway.into()]
    }
}

/// A new sandbox, from the session's harness settings and the form.
#[derive(Debug, Clone, Default)]
pub struct NewSandbox {
    pub gateway: String,
    pub name: String,
    pub from: String,
    pub providers: Vec<String>,
    pub policy: String,
    /// The local directory uploaded into its working directory (`.gitignore` applies).
    pub upload: String,
}

/// `sandbox create`, kept running with no command of its own (a login shell)
/// so the adapter can be started in it with `exec`. `--upload` can't be
/// combined with a command yet, which suits this. Missing providers are an
/// error, never created from local credentials.
pub fn create_args(n: &NewSandbox) -> Vec<String> {
    let mut a = gateway_flag(&n.gateway);
    a.extend(["sandbox", "create", "--name", &n.name].map(String::from));
    if !n.from.is_empty() {
        a.extend(["--from".into(), n.from.clone()]);
    }
    for p in n.providers.iter().filter(|p| !p.trim().is_empty()) {
        a.extend(["--provider".into(), p.trim().to_owned()]);
    }
    if !n.policy.is_empty() {
        a.extend(["--policy".into(), super::acp::expand_home(&n.policy)]);
    }
    a.extend(["--upload".into(), n.upload.clone()]);
    a.extend(["--detach", "--no-auto-providers", "--no-tty", "-o", "json"].map(String::from));
    a
}

/// The adapter inside the sandbox: the harness's `openshell` command, else its own.
pub fn adapter(h: &Harness) -> (String, Vec<String>, std::collections::BTreeMap<String, String>) {
    match &h.openshell {
        Some(o) if !o.command.is_empty() => (o.command.clone(), o.args.clone(), o.env.clone()),
        Some(o) => (h.command.clone(), h.args.clone(), o.env.clone()),
        None => (h.command.clone(), h.args.clone(), Default::default()),
    }
}

/// `sandbox exec` running the adapter in `workdir`, its stdio piped (no TTY:
/// ACP is one JSON message per line). It runs through the sandbox user's
/// login shell, so an image's profile can put the adapter on PATH. No
/// timeout: the exec lasts as long as the agent.
pub fn exec_args(sb: &Sandbox, workdir: &str, h: &Harness) -> Vec<String> {
    let (cmd, args, env) = adapter(h);
    let mut a = gateway_flag(&sb.gateway);
    a.extend(["sandbox", "exec", "-n", &sb.name, "--no-tty", "--timeout", "0"].map(String::from));
    if !workdir.is_empty() {
        a.extend(["--workdir".into(), workdir.to_owned()]);
    }
    for (k, v) in env {
        a.extend(["--env".into(), format!("{k}={v}")]);
    }
    a.push("--".into());
    a.push(cmd);
    a.extend(args);
    a
}

/// A shell in the sandbox at `workdir`, for a terminal.
pub fn terminal_command(command: &str, sb: &Sandbox, workdir: &str) -> String {
    let mut a = vec![command.to_owned()];
    a.extend(gateway_flag(&sb.gateway));
    a.extend(["sandbox", "exec", "-n", &sb.name, "--tty"].map(String::from));
    if !workdir.is_empty() {
        a.extend(["--workdir".into(), workdir.to_owned()]);
    }
    a.extend(["--", "bash", "-l"].map(String::from));
    shlex::try_join(a.iter().map(String::as_str)).unwrap_or_else(|_| a.join(" "))
}

/// Run the CLI and return its stdout; a failure carries the end of its stderr.
fn run(command: &str, args: &[String], timeout: Duration) -> Result<String> {
    let path = super::acp::login_path();
    let exe = super::acp::which(command, &path).ok_or_else(|| msg(format!("{command} isn't on your PATH (agents.openshell.command)")))?;
    let mut c = Command::new(exe);
    c.args(args).env("PATH", path).env("NO_COLOR", "1").stdin(Stdio::null());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(c.output());
    });
    let out = rx
        .recv_timeout(timeout)
        .map_err(|_| msg(format!("openshell {} didn't finish in {} s", args.iter().find(|a| !a.starts_with('-')).map(String::as_str).unwrap_or(""), timeout.as_secs())))?
        .map_err(|e| msg(e.to_string()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = err.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(3)..].join(" · ");
    Err(msg(if tail.is_empty() { format!("openshell exited with {}", out.status) } else { tail }))
}

/// The gateways the CLI knows (`gateway list -o json`): name, endpoint, local or remote, and which is active.
pub fn gateways(command: &str) -> Result<Value> {
    let out = run(command, &["gateway", "list", "-o", "json"].map(String::from), QUICK)?;
    Ok(json!(parse_gateways(&out)))
}

pub fn parse_gateways(out: &str) -> Vec<Value> {
    let list: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    list.iter()
        .filter(|g| !s(g, "name").is_empty())
        .map(|g| {
            json!({
                "name": s(g, "name"),
                "endpoint": s(g, "endpoint"),
                "remote": g.get("is_remote").and_then(Value::as_bool).unwrap_or(false) || s(g, "type") == "remote",
                "active": g.get("active").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// A gateway's sandboxes (`sandbox list -o json`): name and phase.
pub fn sandboxes(command: &str, gateway: &str) -> Result<Value> {
    let mut a = gateway_flag(gateway);
    a.extend(["sandbox", "list", "-o", "json"].map(String::from));
    Ok(json!(parse_sandboxes(&run(command, &a, QUICK)?)))
}

pub fn parse_sandboxes(out: &str) -> Vec<Value> {
    let list: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    list.iter().filter(|b| !s(b, "name").is_empty()).map(|b| json!({ "name": s(b, "name"), "phase": s(b, "phase") })).collect()
}

/// Create the sandbox and upload the directory; returns where it landed
/// inside: the sandbox's working directory, then the directory's own name
/// (OpenShell keeps it, as `cp -r` does).
pub fn create(command: &str, n: &NewSandbox) -> Result<String> {
    run(command, &create_args(n), CREATE)?;
    let home = working_dir(command, &n.gateway, &n.name)?;
    let base = std::path::Path::new(n.upload.trim_end_matches('/')).file_name().map(|b| b.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(if base.is_empty() { home } else { format!("{}/{base}", home.trim_end_matches('/')) })
}

/// The sandbox's working directory, where uploads land.
fn working_dir(command: &str, gateway: &str, name: &str) -> Result<String> {
    let mut a = gateway_flag(gateway);
    a.extend(["sandbox", "exec", "-n", name, "--no-tty", "--no-login-shell", "--", "pwd"].map(String::from));
    let out = run(command, &a, QUICK)?;
    out.lines().map(str::trim).rfind(|l| l.starts_with('/')).map(str::to_owned).ok_or_else(|| msg("couldn't tell the sandbox's working directory"))
}

/// Start the sandbox if it was stopped (a no-op when it's running), before the adapter is started in it again.
pub fn ensure_started(command: &str, sb: &Sandbox) -> Result<()> {
    let mut a = gateway_flag(&sb.gateway);
    a.extend(["sandbox", "start", &sb.name].map(String::from));
    run(command, &a, QUICK).map(|_| ())
}

/// A sandbox name from the session id: lowercase letters, digits and hyphens, at most 19 (OpenShell's routable limit).
pub fn sandbox_name(session_id: &str) -> String {
    let hex: String = session_id.chars().filter(char::is_ascii_hexdigit).collect::<String>().to_lowercase();
    format!("aleth-{}", &hex[hex.len().saturating_sub(8)..])
}

/// Claude Code's adapter (claude-agent-acp), from `initialize`'s `agentInfo`.
pub fn is_claude(init: &Value) -> bool {
    init.pointer("/agentInfo/name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase().contains("claude"))
}

/// The mode a sandboxed session switches to, if it isn't in it already:
/// Claude Code's `auto` (claude-agent-acp falls back to accept-edits, with a
/// notice, for a model without it); for any other agent, one that skips
/// permission checks (`bypassPermissions`, codex-acp's `full-access`, `yolo`).
pub fn mode(init: &Value, opened: &Value) -> Option<String> {
    let modes: Vec<&str> = opened.pointer("/modes/availableModes")?.as_array()?.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).collect();
    let current = opened.pointer("/modes/currentModeId").and_then(Value::as_str);
    let want: &[&str] = if is_claude(init) { &["auto"] } else { &["bypassPermissions", "full-access", "full_access", "yolo"] };
    let m = want.iter().find(|w| modes.contains(w))?;
    (current != Some(*m)).then(|| (*m).to_owned())
}

/// The option that allows a permission request, for agents whose checks are
/// off in the sandbox. By kind when the kinds tell allow from deny (allow
/// once before always, so nothing is saved as a rule); otherwise by name:
/// pi-acp sends every choice of an extension's `select` (a permission
/// extension's "Allow" / "Deny") as `allow_once`. None when nothing reads as
/// an allow: the request is left to you.
pub fn allow_option(options: &[PermOption]) -> Option<&PermOption> {
    if options.iter().any(|o| o.kind.starts_with("reject")) {
        return options.iter().find(|o| o.kind == "allow_once").or_else(|| options.iter().find(|o| o.kind == "allow_always"));
    }
    let yes = regex::Regex::new(r"^(yes|allow|approve|ok|okay|proceed|continue|run)\b").ok()?;
    let no = regex::Regex::new(r"\b(no|not|never|deny|reject|block|cancel|abort|don't|dont)\b").ok()?;
    options.iter().find(|o| {
        let n = o.name.trim().to_lowercase();
        yes.is_match(&n) && !no.is_match(&n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::HarnessSandbox;

    fn opt(id: &str, name: &str, kind: &str) -> PermOption {
        PermOption { id: id.into(), name: name.into(), kind: kind.into(), key: String::new() }
    }

    fn harness(openshell: Option<HarnessSandbox>) -> Harness {
        Harness { key: "claude".into(), label: "Claude Code".into(), command: "npx".into(), args: vec!["-y".into(), "@agentclientprotocol/claude-agent-acp".into()], env: Default::default(), cache_ttl_min: None, openshell }
    }

    #[test]
    fn claude_runs_in_auto_mode() {
        let init = json!({ "agentInfo": { "name": "@agentclientprotocol/claude-agent-acp" } });
        let opened = json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "acceptEdits" }, { "id": "plan" }, { "id": "auto" }, { "id": "bypassPermissions" }] } });
        assert_eq!(mode(&init, &opened).as_deref(), Some("auto"), "auto, not bypass, even when bypass is offered");
        let already = json!({ "modes": { "currentModeId": "auto", "availableModes": [{ "id": "auto" }] } });
        assert_eq!(mode(&init, &already), None);
        assert!(is_claude(&init));
    }

    #[test]
    fn other_agents_skip_their_checks() {
        let codex = json!({ "agentInfo": { "name": "codex-acp" } });
        let opened = json!({ "modes": { "currentModeId": "auto", "availableModes": [{ "id": "read-only" }, { "id": "auto" }, { "id": "full-access" }] } });
        assert_eq!(mode(&codex, &opened).as_deref(), Some("full-access"), "codex's auto still asks");
        let pi = json!({ "agentInfo": { "name": "pi-acp" } });
        let levels = json!({ "modes": { "currentModeId": "medium", "availableModes": [{ "id": "low" }, { "id": "medium" }] } });
        assert_eq!(mode(&pi, &levels), None, "pi's modes are thinking levels");
        assert!(!is_claude(&pi));
        let dont_ask = json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "dontAsk" }] } });
        assert_eq!(mode(&pi, &dont_ask), None, "dontAsk denies what isn't allowed already");
    }

    #[test]
    fn the_allow_option_by_kind() {
        let claude = [opt("always", "Yes, and don't ask again for git", "allow_always"), opt("once", "Yes", "allow_once"), opt("no", "No", "reject_once")];
        assert_eq!(allow_option(&claude).map(|o| o.id.as_str()), Some("once"), "once, so no rule is saved");
        let only_always = [opt("a", "Always", "allow_always"), opt("n", "No", "reject_once")];
        assert_eq!(allow_option(&only_always).map(|o| o.id.as_str()), Some("a"));
        let pi_confirm = [opt("yes", "Yes", "allow_once"), opt("no", "No", "reject_once")];
        assert_eq!(allow_option(&pi_confirm).map(|o| o.id.as_str()), Some("yes"));
    }

    #[test]
    fn the_allow_option_by_name_when_kinds_dont_tell() {
        // pi-acp: an extension's select, every choice allow_once.
        let gate = [opt("choice-0", "Deny", "allow_once"), opt("choice-1", "Allow once", "allow_once"), opt("choice-2", "Allow always", "allow_once")];
        assert_eq!(allow_option(&gate).map(|o| o.id.as_str()), Some("choice-1"));
        let not = [opt("choice-0", "Don't allow", "allow_once"), opt("choice-1", "No, provide reason", "allow_once")];
        assert_eq!(allow_option(&not), None);
        let question = [opt("choice-0", "main", "allow_once"), opt("choice-1", "dev", "allow_once")];
        assert_eq!(allow_option(&question), None, "a select that isn't a gate is left to you");
    }

    #[test]
    fn exec_runs_the_sandbox_adapter() {
        let sb = Sandbox { gateway: "remote-k8s".into(), name: "aleth-1a2b3c4d".into(), ..Default::default() };
        let own = exec_args(&sb, "/sandbox/alethiometer", &harness(None));
        assert_eq!(
            own,
            ["-g", "remote-k8s", "sandbox", "exec", "-n", "aleth-1a2b3c4d", "--no-tty", "--timeout", "0", "--workdir", "/sandbox/alethiometer", "--", "npx", "-y", "@agentclientprotocol/claude-agent-acp"].map(String::from).to_vec()
        );
        let image = HarnessSandbox { command: "claude-agent-acp".into(), env: [("DISABLE_AUTOUPDATER".to_string(), "1".to_string())].into(), ..Default::default() };
        let active = Sandbox { name: "s".into(), ..Default::default() };
        assert_eq!(
            exec_args(&active, "", &harness(Some(image))),
            ["sandbox", "exec", "-n", "s", "--no-tty", "--timeout", "0", "--env", "DISABLE_AUTOUPDATER=1", "--", "claude-agent-acp"].map(String::from).to_vec(),
            "the active gateway takes no -g"
        );
    }

    #[test]
    fn create_uploads_and_detaches() {
        let n = NewSandbox { gateway: "local".into(), name: "aleth-00ff".into(), from: "ghcr.io/me/claude-acp:1".into(), providers: vec!["anthropic".into(), " ".into()], upload: "/Users/me/src/app".into(), ..Default::default() };
        assert_eq!(
            create_args(&n),
            ["-g", "local", "sandbox", "create", "--name", "aleth-00ff", "--from", "ghcr.io/me/claude-acp:1", "--provider", "anthropic", "--upload", "/Users/me/src/app", "--detach", "--no-auto-providers", "--no-tty", "-o", "json"].map(String::from).to_vec()
        );
    }

    #[test]
    fn a_terminal_line_is_quoted() {
        let sb = Sandbox { gateway: "my gw".into(), name: "s".into(), ..Default::default() };
        assert_eq!(terminal_command("openshell", &sb, "/sandbox/a b"), "openshell -g 'my gw' sandbox exec -n s --tty --workdir '/sandbox/a b' -- bash -l");
    }

    #[test]
    fn names_fit_openshell() {
        let n = sandbox_name("acp-1A2B3C4D");
        assert_eq!(n, "aleth-1a2b3c4d");
        assert!(n.len() <= 19 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn cli_listings() {
        // Shapes from openshell-cli's gateway_to_json and sandbox_to_json.
        let g = parse_gateways(r#"[{"name":"local","endpoint":"http://127.0.0.1:17670","type":"local","source":"system","auth":"plaintext","active":true,"is_remote":false,"remote_host":null,"resolved_host":null},{"name":"k8s","endpoint":"https://gw.example.com","type":"remote","source":"user","auth":"edge","active":false,"is_remote":true}]"#);
        assert_eq!(g, vec![json!({ "name": "local", "endpoint": "http://127.0.0.1:17670", "remote": false, "active": true }), json!({ "name": "k8s", "endpoint": "https://gw.example.com", "remote": true, "active": false })]);
        let s = parse_sandboxes(r#"[{"id":"x","name":"aleth-1","phase":"Ready","labels":{}},{"id":"y","name":"old","phase":"Stopped"}]"#);
        assert_eq!(s, vec![json!({ "name": "aleth-1", "phase": "Ready" }), json!({ "name": "old", "phase": "Stopped" })]);
        assert!(parse_sandboxes("No sandboxes found.").is_empty());
    }
}
