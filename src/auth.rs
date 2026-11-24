//! Claude Code's own login, through `claude auth`.
//!
//! cam never sees a credential. It runs `claude auth login`, `status --json`
//! and `logout` with the account's config dir, and reads only identity
//! fields: the email, org, plan and expiry from `status`, and the email from
//! `.claude.json`, which can also hold MCP server secrets.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use serde::Deserialize;
use toml::value::Date;

use crate::date;
use crate::error::Error;
use crate::platform::IgnoreInterrupts;
pub use crate::platform::describe_exit;

/// The `authMethod` of a `/login`.
pub const CLAUDE_AI: &str = "claude.ai";

/// Days before expiry from which Claude Code warns, and so does cam.
pub const WARN_DAYS: i64 = 3;

/// What `claude auth status --json` reports. Other fields are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub logged_in: bool,
    pub auth_method: Option<String>,
    pub api_key_source: Option<String>,
    pub email: Option<String>,
    pub org_name: Option<String>,
    pub subscription_type: Option<String>,
    pub login_expires_at: Option<String>,
}

impl Status {
    /// Logged in through `/login`, as someone.
    pub fn is_login(&self) -> bool {
        self.logged_in && self.auth_method.as_deref() == Some(CLAUDE_AI)
    }

    /// The day the login expires, from `loginExpiresAt`.
    pub fn expires(&self) -> Option<Date> {
        self.login_expires_at.as_deref().and_then(date::parse_day)
    }

    /// "you@company.com (Acme, team)", or as much of it as is known.
    pub fn identity(&self) -> String {
        let email = self.email.as_deref().unwrap_or("an unknown email");
        match self.org_plan() {
            Some(org_plan) => format!("{email} ({org_plan})"),
            None => email.to_owned(),
        }
    }

    /// "Acme, team", "Acme" or "team".
    pub fn org_plan(&self) -> Option<String> {
        match (self.org_name.as_deref(), self.subscription_type.as_deref()) {
            (Some(org), Some(plan)) => Some(format!("{org}, {plan}")),
            (Some(org), None) => Some(org.to_owned()),
            (None, Some(plan)) => Some(plan.to_owned()),
            (None, None) => None,
        }
    }

    /// What Claude Code would use instead of a login, when it isn't one:
    /// `ANTHROPIC_API_KEY`, `apiKeyHelper` and the like, or the method.
    pub fn other_method(&self) -> Option<String> {
        if !self.logged_in || self.is_login() {
            return None;
        }
        self.api_key_source
            .clone()
            .or_else(|| self.auth_method.clone())
            .or_else(|| Some("something other than a login".to_owned()))
    }
}

/// How a login's expiry stands on `today`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    Unknown,
    Good(Date),
    Soon(Date),
    Expired(Date),
}

pub fn expiry(status: &Status, today: Date) -> Expiry {
    let Some(expires) = status.expires() else {
        return Expiry::Unknown;
    };
    match date::to_days(expires) - date::to_days(today) {
        days if days < 0 => Expiry::Expired(expires),
        days if days <= WARN_DAYS => Expiry::Soon(expires),
        _ => Expiry::Good(expires),
    }
}

impl Expiry {
    /// "expires 2026-12-13 (in 28 days)", "expired 2026-11-10 (5 days ago)".
    pub fn describe(self, today: Date) -> Option<String> {
        match self {
            Self::Unknown => None,
            Self::Good(day) | Self::Soon(day) => {
                Some(format!("expires {day} ({})", date::until(day, today)))
            }
            Self::Expired(day) => Some(format!("expired {day} ({})", date::age(day, today))),
        }
    }
}

pub fn parse_status(stdout: &[u8]) -> Result<Status, serde_json::Error> {
    serde_json::from_slice(stdout)
}

/// Runs `claude auth status --json` (`command`) and parses what it says.
/// It exits 1 when not logged in, with the same JSON, so the exit code
/// alone means nothing. The output is never shown: only parsed fields are.
pub fn status(mut command: Command, program: &Path) -> Result<Status, Error> {
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|source| Error::Exec {
            path: program.to_owned(),
            source,
        })?;
    parse_status(&output.stdout).map_err(|_| Error::AuthStatus {
        status: describe_exit(output.status),
    })
}

/// Runs `claude auth login` (`command`) on the terminal and waits for it.
/// While it runs, cam ignores Ctrl-C and Ctrl-\, as `system(3)` does, so
/// that they reach only `claude` and cam can clean up after a cancel.
pub fn login(mut command: Command, program: &Path) -> Result<ExitStatus, Error> {
    let mut child = command.spawn().map_err(|source| Error::Exec {
        path: program.to_owned(),
        source,
    })?;
    let _ignored = IgnoreInterrupts::new();
    child.wait().map_err(|source| Error::Exec {
        path: program.to_owned(),
        source,
    })
}

/// Runs `claude auth logout` (`command`). Its output is shown only when it
/// fails: what it printed on stderr goes into the error, which is redacted
/// like everything cam prints.
pub fn logout(mut command: Command, program: &Path) -> Result<(), Error> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| Error::Exec {
            path: program.to_owned(),
            source,
        })?;
    if output.status.success() {
        return Ok(());
    }
    let printed = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::Logout {
        status: describe_exit(output.status),
        output: (!printed.is_empty()).then_some(printed),
    })
}

/// The `.claude.json` of a config dir: inside it for an account (or an
/// exported `CLAUDE_CONFIG_DIR`), else `~/.claude.json`.
pub fn claude_json(config_dir: Option<&Path>, home: &Path) -> PathBuf {
    match config_dir {
        Some(dir) => dir.join(".claude.json"),
        None => home.join(".claude.json"),
    }
}

/// The email Claude Code last recorded in a `.claude.json`
/// (`oauthAccount.emailAddress`). Nothing else is read out of it.
pub fn recorded_email(path: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct File {
        #[serde(rename = "oauthAccount")]
        oauth_account: Option<Account>,
    }
    #[derive(Deserialize)]
    struct Account {
        #[serde(rename = "emailAddress")]
        email_address: Option<String>,
    }
    let text = fs::read(path).ok()?;
    serde_json::from_slice::<File>(&text)
        .ok()?
        .oauth_account?
        .email_address
        .filter(|email| !email.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    const TODAY: Date = Date {
        year: 2026,
        month: 11,
        day: 15,
    };

    fn logged_in() -> Status {
        parse_status(
            br#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
                 "configDirectory": "/x", "loginExpiresAt": "2026-12-13T15:59:25.959Z",
                 "email": "you@company.com", "orgId": "id", "orgName": "Acme",
                 "subscriptionType": "team", "planTier": "t", "storedClaudeAiLogin": true}"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_a_login() {
        let status = logged_in();
        assert!(status.is_login());
        assert_eq!(status.identity(), "you@company.com (Acme, team)");
        assert_eq!(status.other_method(), None);
        assert_eq!(
            status.expires(),
            Some(Date {
                year: 2026,
                month: 12,
                day: 13
            })
        );
    }

    #[test]
    fn parses_logged_out_and_api_key() {
        let out =
            parse_status(br#"{"loggedIn": false, "authMethod": "none", "loginExpiresAt": null}"#)
                .unwrap();
        assert!(!out.is_login());
        assert_eq!(out.other_method(), None);
        assert_eq!(expiry(&out, TODAY), Expiry::Unknown);

        let key = parse_status(
            br#"{"loggedIn": true, "authMethod": "api_key", "apiKeySource": "ANTHROPIC_API_KEY"}"#,
        )
        .unwrap();
        assert!(!key.is_login());
        assert_eq!(key.other_method().as_deref(), Some("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn rejects_what_isnt_status_json() {
        assert!(parse_status(b"").is_err());
        assert!(parse_status(b"Not logged in").is_err());
    }

    #[test]
    fn identity_with_missing_parts() {
        let status = Status {
            email: Some("me@home.org".into()),
            subscription_type: Some("max".into()),
            ..Status::default()
        };
        assert_eq!(status.identity(), "me@home.org (max)");
        assert_eq!(Status::default().identity(), "an unknown email");
    }

    #[test]
    fn expiry_warns_within_three_days() {
        let at = |day: &str| Status {
            login_expires_at: Some(format!("{day}T10:00:00.000Z")),
            ..logged_in()
        };
        let day = |d| Date {
            year: 2026,
            month: 11,
            day: d,
        };
        assert_eq!(expiry(&at("2026-11-19"), TODAY), Expiry::Good(day(19)));
        assert_eq!(expiry(&at("2026-11-18"), TODAY), Expiry::Soon(day(18)));
        assert_eq!(expiry(&at("2026-11-15"), TODAY), Expiry::Soon(day(15)));
        assert_eq!(expiry(&at("2026-11-14"), TODAY), Expiry::Expired(day(14)));
        assert_eq!(
            expiry(&logged_in(), TODAY).describe(TODAY).as_deref(),
            Some("expires 2026-12-13 (in 28 days)")
        );
        assert_eq!(
            Expiry::Expired(day(10)).describe(TODAY).as_deref(),
            Some("expired 2026-11-10 (5 days ago)")
        );
    }

    #[test]
    fn claude_json_location() {
        let home = Path::new("/home/me");
        assert_eq!(
            claude_json(None, home),
            PathBuf::from("/home/me/.claude.json")
        );
        assert_eq!(
            claude_json(Some(Path::new("/acct")), home),
            PathBuf::from("/acct/.claude.json")
        );
    }

    #[test]
    fn reads_only_the_recorded_email() {
        let tree = Tree::new();
        let path = tree.root.join(".claude.json");
        assert_eq!(recorded_email(&path), None);
        fs::write(
            &path,
            r#"{"mcpServers": {"x": {"env": {"KEY": "secret"}}},
                "oauthAccount": {"emailAddress": "you@company.com", "organizationName": "Acme"}}"#,
        )
        .unwrap();
        assert_eq!(recorded_email(&path).as_deref(), Some("you@company.com"));
        fs::write(&path, r#"{"oauthAccount": null}"#).unwrap();
        assert_eq!(recorded_email(&path), None);
        fs::write(&path, "not json").unwrap();
        assert_eq!(recorded_email(&path), None);
    }
}
