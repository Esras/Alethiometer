//! AWS credentials for Bedrock: a small subset of the standard provider chain.
//!
//! Order (first hit wins):
//! 1. env: `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ `AWS_SESSION_TOKEN`);
//! 2. Alethiometer's secret store (Keychain on the Mac): `aws_access_key_id`,
//!    `aws_secret_access_key`, optional `aws_session_token`;
//! 3. the profile (`settings.ai.aws_profile`, else `AWS_PROFILE`, else "default"):
//!    static keys in `~/.aws/credentials`, then static keys or `credential_process`
//!    in `~/.aws/config`.
//!
//! SSO profiles aren't read directly (that needs the SSO portal API). Point a
//! profile's `credential_process` at `aws configure export-credentials --profile
//! <sso-profile> --format process` instead; see docs/ai.md.
//!
//! Nothing here logs or formats a secret. Error messages name where we looked,
//! never what we found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct AwsCreds {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// Where they came from, for status messages ("env", "keychain", "profile work").
    pub source: String,
}

impl std::fmt::Debug for AwsCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsCreds").field("source", &self.source).finish_non_exhaustive()
    }
}

/// Everything the chain reads, injectable for tests.
pub struct Lookup<'a> {
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub secret: &'a dyn Fn(&str) -> Option<String>,
    pub home: Option<PathBuf>,
    /// `settings.ai.aws_profile` (may be empty).
    pub profile: &'a str,
}

pub fn resolve(l: &Lookup) -> anyhow::Result<AwsCreds> {
    let env = |k: &str| (l.env)(k).filter(|v| !v.is_empty());
    if let (Some(id), Some(key)) = (env("AWS_ACCESS_KEY_ID"), env("AWS_SECRET_ACCESS_KEY")) {
        return Ok(AwsCreds { access_key_id: id, secret_access_key: key, session_token: env("AWS_SESSION_TOKEN"), source: "env".into() });
    }
    let sec = |k: &str| (l.secret)(k).filter(|v| !v.is_empty());
    if let (Some(id), Some(key)) = (sec("aws_access_key_id"), sec("aws_secret_access_key")) {
        return Ok(AwsCreds { access_key_id: id, secret_access_key: key, session_token: sec("aws_session_token"), source: "keychain".into() });
    }

    let profile = if !l.profile.is_empty() { l.profile.to_string() } else { env("AWS_PROFILE").unwrap_or_else(|| "default".into()) };
    let aws_dir = l.home.as_ref().map(|h| h.join(".aws"));
    let creds_file = env("AWS_SHARED_CREDENTIALS_FILE").map(PathBuf::from).or_else(|| aws_dir.as_ref().map(|d| d.join("credentials")));
    let config_file = env("AWS_CONFIG_FILE").map(PathBuf::from).or_else(|| aws_dir.as_ref().map(|d| d.join("config")));

    // ~/.aws/credentials uses bare section names: [default], [work].
    if let Some(sect) = creds_file.as_deref().and_then(|p| section(p, &profile)) {
        if let Some(c) = static_keys(&sect, &profile) {
            return Ok(c);
        }
    }
    // ~/.aws/config uses [default] and [profile work].
    let name = if profile == "default" { "default".to_string() } else { format!("profile {profile}") };
    if let Some(sect) = config_file.as_deref().and_then(|p| section(p, &name)) {
        if let Some(c) = static_keys(&sect, &profile) {
            return Ok(c);
        }
        if let Some(cmd) = sect.get("credential_process") {
            return credential_process(cmd, &profile);
        }
        if sect.contains_key("sso_session") || sect.contains_key("sso_start_url") {
            anyhow::bail!(
                "AWS profile '{profile}' uses SSO, which Alethiometer can't read directly. Add \
                 credential_process = aws configure export-credentials --profile {profile} --format process \
                 to a profile and point ai.aws_profile at it (see docs/ai.md)"
            );
        }
    }
    anyhow::bail!(
        "No AWS credentials found for Bedrock (looked in env vars, Alethiometer secrets aws_access_key_id/aws_secret_access_key, \
         and profile '{profile}' in ~/.aws). See docs/ai.md"
    )
}

fn static_keys(sect: &BTreeMap<String, String>, profile: &str) -> Option<AwsCreds> {
    let id = sect.get("aws_access_key_id").filter(|v| !v.is_empty())?;
    let key = sect.get("aws_secret_access_key").filter(|v| !v.is_empty())?;
    Some(AwsCreds {
        access_key_id: id.clone(),
        secret_access_key: key.clone(),
        session_token: sect.get("aws_session_token").filter(|v| !v.is_empty()).cloned(),
        source: format!("profile {profile}"),
    })
}

/// Keys of one `[name]` section of an INI file. None if the file or section is missing.
fn section(path: &Path, name: &str) -> Option<BTreeMap<String, String>> {
    parse_section(&std::fs::read_to_string(path).ok()?, name)
}

fn parse_section(text: &str, name: &str) -> Option<BTreeMap<String, String>> {
    let mut found = None;
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(h) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let h = h.split_whitespace().collect::<Vec<_>>().join(" ");
            current = Some(h.clone());
            if h == name {
                found.get_or_insert_with(BTreeMap::new);
            }
            continue;
        }
        if current.as_deref() == Some(name) {
            if let Some((k, v)) = line.split_once('=') {
                found.get_or_insert_with(BTreeMap::new).insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
    }
    found
}

/// Run a profile's `credential_process` and parse its JSON (Version 1 format).
fn credential_process(cmd: &str, profile: &str) -> anyhow::Result<AwsCreds> {
    #[cfg(unix)]
    let out = std::process::Command::new("sh").arg("-c").arg(cmd).output();
    #[cfg(not(unix))]
    let out = std::process::Command::new("cmd").arg("/C").arg(cmd).output();
    let out = out.map_err(|e| anyhow::anyhow!("credential_process for AWS profile '{profile}' could not start: {e}"))?;
    if !out.status.success() {
        let err: String = String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("").chars().take(200).collect();
        anyhow::bail!("credential_process for AWS profile '{profile}' failed (expired SSO login? run `aws sso login`): {err}");
    }
    parse_process_output(&String::from_utf8_lossy(&out.stdout), profile)
}

fn parse_process_output(stdout: &str, profile: &str) -> anyhow::Result<AwsCreds> {
    let bad = || anyhow::anyhow!("credential_process for AWS profile '{profile}' returned unexpected output");
    let v: serde_json::Value = serde_json::from_str(stdout).map_err(|_| bad())?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_owned);
    Ok(AwsCreds {
        access_key_id: s("AccessKeyId").ok_or_else(bad)?,
        secret_access_key: s("SecretAccessKey").ok_or_else(bad)?,
        session_token: s("SessionToken"),
        source: format!("profile {profile} (credential_process)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    fn home_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".aws")).unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(".aws").join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn env_wins() {
        let env = |k: &str| match k {
            "AWS_ACCESS_KEY_ID" => Some("AKIDENV".to_string()),
            "AWS_SECRET_ACCESS_KEY" => Some("s".to_string()),
            "AWS_SESSION_TOKEN" => Some("t".to_string()),
            _ => None,
        };
        let c = resolve(&Lookup { env: &env, secret: &none, home: None, profile: "" }).unwrap();
        assert_eq!((c.access_key_id.as_str(), c.session_token.as_deref(), c.source.as_str()), ("AKIDENV", Some("t"), "env"));
    }

    #[test]
    fn keychain_before_files() {
        let sec = |k: &str| match k {
            "aws_access_key_id" => Some("AKIDKC".to_string()),
            "aws_secret_access_key" => Some("s".to_string()),
            _ => None,
        };
        let home = home_with(&[("credentials", "[default]\naws_access_key_id=AKIDFILE\naws_secret_access_key=x\n")]);
        let c = resolve(&Lookup { env: &none, secret: &sec, home: Some(home.path().into()), profile: "" }).unwrap();
        assert_eq!(c.access_key_id, "AKIDKC");
    }

    #[test]
    fn named_profile_from_credentials_file() {
        let home = home_with(&[(
            "credentials",
            "[default]\naws_access_key_id = AKIDDEF\naws_secret_access_key = a\n\n# work\n[work]\naws_access_key_id = AKIDWORK\naws_secret_access_key = b\naws_session_token = tok\n",
        )]);
        let env = |k: &str| (k == "AWS_PROFILE").then(|| "work".to_string());
        let c = resolve(&Lookup { env: &env, secret: &none, home: Some(home.path().into()), profile: "" }).unwrap();
        assert_eq!((c.access_key_id.as_str(), c.session_token.as_deref()), ("AKIDWORK", Some("tok")));
        // The setting beats AWS_PROFILE.
        let c = resolve(&Lookup { env: &env, secret: &none, home: Some(home.path().into()), profile: "default" }).unwrap();
        assert_eq!(c.access_key_id, "AKIDDEF");
    }

    #[test]
    fn config_file_uses_profile_prefix_and_credential_process() {
        let home = home_with(&[(
            "config",
            "[profile alethiometer]\nregion = us-east-1\ncredential_process = echo '{\"Version\":1,\"AccessKeyId\":\"AKIDPROC\",\"SecretAccessKey\":\"s\",\"SessionToken\":\"t\"}'\n",
        )]);
        let c = resolve(&Lookup { env: &none, secret: &none, home: Some(home.path().into()), profile: "alethiometer" }).unwrap();
        assert_eq!((c.access_key_id.as_str(), c.session_token.as_deref()), ("AKIDPROC", Some("t")));
    }

    #[test]
    fn sso_profile_explains_what_to_do() {
        let home = home_with(&[("config", "[profile sso]\nsso_session = corp\nsso_account_id = 1\n")]);
        let err = resolve(&Lookup { env: &none, secret: &none, home: Some(home.path().into()), profile: "sso" }).unwrap_err().to_string();
        assert!(err.contains("export-credentials"), "{err}");
    }

    #[test]
    fn missing_everywhere_is_a_clear_error() {
        let home = home_with(&[]);
        let err = resolve(&Lookup { env: &none, secret: &none, home: Some(home.path().into()), profile: "" }).unwrap_err().to_string();
        assert!(err.contains("No AWS credentials"), "{err}");
    }

    #[test]
    fn debug_never_shows_keys() {
        let c = AwsCreds { access_key_id: "AKIDX".into(), secret_access_key: "SECRETX".into(), session_token: Some("TOKX".into()), source: "env".into() };
        let d = format!("{c:?}");
        assert!(!d.contains("AKIDX") && !d.contains("SECRETX") && !d.contains("TOKX"), "{d}");
    }
}
