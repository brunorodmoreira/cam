//! `cam doctor`'s checks, and the token-age rules `whoami` shares.
//!
//! Each check is ok, info, warn or fail. Only a fail makes `doctor` exit
//! non-zero. Nothing here reads a token: stored tokens are checked with
//! [`SecretStore::contains`](crate::store::SecretStore::contains), so
//! `doctor` can't cause a Keychain prompt.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use toml::value::Date;

use crate::config::{Account, Config};
use crate::conflict;
use crate::date;
use crate::error::Error;
use crate::launch::{ENV_ACTIVE, ENV_TOKEN};
use crate::resolve::Resolution;
use crate::settings::{File, Inspection};
use crate::shim::Reaches;

/// `setup-token` says its tokens are "valid for 1 year".
pub const LIFETIME_DAYS: i64 = 365;
/// From here on, `doctor` and `whoami` suggest renewing.
pub const RENEW_AFTER_DAYS: i64 = 330;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Info,
    Warn,
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub status: Status,
    /// One or more lines. Later lines are usually a `hint:`.
    pub message: String,
}

impl Check {
    pub fn new(status: Status, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// The lines for a check: the status, then the message, with later lines
/// indented under the first.
pub fn render(check: &Check) -> Vec<String> {
    check
        .message
        .lines()
        .enumerate()
        .map(|(index, line)| match index {
            0 => format!("{:<4}  {line}", check.status.label()),
            _ => format!("      {line}"),
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Tally {
    pub problems: usize,
    pub warnings: usize,
}

pub fn tally(checks: &[Check]) -> Tally {
    let count = |status| checks.iter().filter(|check| check.status == status).count();
    Tally {
        problems: count(Status::Fail),
        warnings: count(Status::Warn),
    }
}

impl fmt::Display for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = |count: usize, word: &str| match count {
            1 => format!("1 {word}"),
            _ => format!("{count} {word}s"),
        };
        match (self.problems, self.warnings) {
            (0, 0) => f.write_str("no problems found"),
            (0, warnings) => f.write_str(&plural(warnings, "warning")),
            (problems, 0) => f.write_str(&plural(problems, "problem")),
            (problems, warnings) => write!(
                f,
                "{}, {}",
                plural(problems, "problem"),
                plural(warnings, "warning")
            ),
        }
    }
}

/// An error with its causes, joined the way `cam` prints errors.
fn explain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    text
}

// --- token age ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAge {
    /// No `token_created` in the config.
    Unknown,
    Good {
        created: Date,
        expires: Date,
    },
    Renew {
        created: Date,
        expires: Date,
    },
    Expired {
        created: Date,
        expires: Date,
    },
}

pub fn token_age(created: Option<Date>, today: Date) -> TokenAge {
    let Some(created) = created else {
        return TokenAge::Unknown;
    };
    let expires = date::from_days(date::to_days(created) + LIFETIME_DAYS);
    match date::to_days(today) - date::to_days(created) {
        days if days > LIFETIME_DAYS => TokenAge::Expired { created, expires },
        days if days >= RENEW_AFTER_DAYS => TokenAge::Renew { created, expires },
        _ => TokenAge::Good { created, expires },
    }
}

impl TokenAge {
    /// "added 2026-11-11 (3 days ago), good until about 2027-11-11", or
    /// what's known instead.
    pub fn describe(self, today: Date) -> String {
        let added = |created| format!("added {created} ({})", date::age(created, today));
        match self {
            Self::Unknown => "date unknown (no token_created in the config)".to_owned(),
            Self::Good { created, expires } => {
                format!("{}, good until about {expires}", added(created))
            }
            Self::Renew { created, expires } => {
                format!("{}, expires around {expires}", added(created))
            }
            Self::Expired { created, expires } => {
                format!("{}, probably expired around {expires}", added(created))
            }
        }
    }
}

/// How to get a new token for `name`.
pub fn renew_hint(name: &str) -> String {
    format!(
        "hint: run `claude setup-token` and sign in with the account for {name}, then `cam add {name} --force`"
    )
}

// --- checks ---

/// The real `claude`, as `exec` would find it.
pub fn claude(found: Result<PathBuf, Error>) -> Check {
    match found {
        Ok(path) => Check::new(Status::Ok, format!("claude: {}", path.display())),
        Err(err) => Check::new(Status::Fail, format!("claude: {}", explain(&err))),
    }
}

/// What plain `claude` runs. `installed` is the default shim dir, if a shim
/// is in it.
pub fn shim(reaches: &Reaches, installed: Option<&Path>) -> Check {
    const FIX: &str = "hint: run `cam shim install` to see the line to add or move";
    const OPTIONS: &str = "hint: use `cam exec`, or run `cam shim install`";
    match (reaches, installed) {
        (Reaches::Shim(path), _) => Check::new(
            Status::Ok,
            format!("shim: plain `claude` runs through cam ({})", path.display()),
        ),
        (Reaches::Other(claude), Some(dir)) => Check::new(
            Status::Warn,
            format!(
                "shim: installed in {}, but plain `claude` runs {} first, without cam\n{FIX}",
                dir.display(),
                claude.display()
            ),
        ),
        (Reaches::Nothing, Some(dir)) => Check::new(
            Status::Warn,
            format!(
                "shim: installed in {}, but that dir isn't on PATH\n{FIX}",
                dir.display()
            ),
        ),
        (Reaches::Other(claude), None) => Check::new(
            Status::Info,
            format!(
                "shim: not installed, so plain `claude` ({}) skips cam\n{OPTIONS}",
                claude.display()
            ),
        ),
        (Reaches::Nothing, None) => {
            Check::new(Status::Info, format!("shim: not installed\n{OPTIONS}"))
        }
    }
}

pub fn config(path: &Path, exists: bool, loaded: &Result<Config, Error>) -> Check {
    let shown = path.display();
    let config = match loaded {
        Ok(config) => config,
        Err(err) => return Check::new(Status::Fail, format!("config: {}", explain(err))),
    };
    if !exists {
        return Check::new(
            Status::Info,
            format!(
                "config: {shown} doesn't exist yet\nhint: add an account with `cam add <name>`"
            ),
        );
    }
    let count = match config.accounts().len() {
        0 => {
            return Check::new(
                Status::Info,
                format!("config: {shown} has no accounts yet\nhint: add one with `cam add <name>`"),
            );
        }
        1 => "1 account".to_owned(),
        count => format!("{count} accounts"),
    };
    let default = match config.default_selection() {
        Some(selection) => format!("default {selection}"),
        None => "no default".to_owned(),
    };
    Check::new(Status::Ok, format!("config: {shown}, {count}, {default}"))
}

/// One configured account: is its token stored, and how old is it?
pub fn account(
    name: &str,
    account: &Account,
    stored: Result<bool, Error>,
    location: &str,
    today: Date,
) -> Check {
    let subject = format!("account {name}");
    match stored {
        Err(err) => return Check::new(Status::Fail, format!("{subject}: {}", explain(&err))),
        Ok(false) => {
            return Check::new(
                Status::Fail,
                format!("{subject}: no token in {location}\nhint: add one with `cam add {name}`"),
            );
        }
        Ok(true) => {}
    }
    let age = token_age(
        account.token_created.and_then(|created| created.date),
        today,
    );
    let line = format!("{subject}: token stored, {}", age.describe(today));
    let hint = renew_hint(name);
    match age {
        TokenAge::Good { .. } => Check::new(Status::Ok, line),
        TokenAge::Unknown => Check::new(
            Status::Info,
            format!("{line}, so cam can't warn before it expires"),
        ),
        TokenAge::Renew { .. } => Check::new(Status::Warn, format!("{line}\n{hint}")),
        TokenAge::Expired { .. } => Check::new(Status::Fail, format!("{line}\n{hint}")),
    }
}

/// The account for the current directory, as `which` reports it.
pub fn here(resolved: Result<Resolution, Error>) -> Check {
    match resolved {
        Ok(resolution) => Check::new(Status::Ok, format!("here: {resolution}")),
        Err(err) => Check::new(Status::Fail, format!("here: {}", explain(&err))),
    }
}

/// Variables in this shell that would beat cam's token or leak it.
pub fn environment(get: impl Fn(&str) -> Option<OsString>) -> Vec<Check> {
    let set = |var: &str| get(var).filter(|value| !value.is_empty());
    let mut checks: Vec<Check> = conflict::in_env(&get)
        .into_iter()
        .map(|conflict| {
            Check::new(
                Status::Warn,
                format!(
                    "environment: {} is set, so {}\nhint: unset it to use the account cam picks",
                    conflict.name(),
                    conflict.effect()
                ),
            )
        })
        .collect();
    match (set(ENV_ACTIVE), set(ENV_TOKEN)) {
        (Some(name), Some(_)) => checks.push(Check::new(
            Status::Warn,
            format!(
                "environment: this shell is inside a `claude` that cam started as {}, and that session's token is visible here\n\
                 hint: any program run from the session can read it; see CLAUDE_CODE_SUBPROCESS_ENV_SCRUB in Claude Code's docs",
                name.to_string_lossy()
            ),
        )),
        (Some(name), None) => checks.push(Check::new(
            Status::Ok,
            format!(
                "environment: inside a `claude` that cam started as {}; its token isn't visible to this shell",
                name.to_string_lossy()
            ),
        )),
        (None, Some(_)) => checks.push(Check::new(
            Status::Warn,
            format!(
                "environment: {ENV_TOKEN} is exported in this shell, so every program started here can read it, and pass-through folders use it instead of your /login\n\
                 hint: unset it and let cam pass tokens to `claude`"
            ),
        )),
        (None, None) => {}
    }
    if checks.is_empty() {
        checks.push(Check::new(
            Status::Ok,
            "environment: nothing outranks cam's tokens",
        ));
    }
    checks
}

/// What Claude Code's settings files would do to cam's token.
pub fn settings(inspected: &[(File, Inspection)]) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut read = 0;
    for (file, inspection) in inspected {
        let shown = file.path.display();
        match inspection {
            Inspection::Missing => {}
            Inspection::Unreadable(reason) => checks.push(Check::new(
                Status::Warn,
                format!("settings: cannot check {shown}: {reason}"),
            )),
            Inspection::Read(conflicts) => {
                read += 1;
                checks.extend(conflicts.iter().map(|conflict| {
                    let place = match conflict {
                        conflict::Conflict::ApiKeyHelper => String::new(),
                        _ => " in `env`".to_owned(),
                    };
                    Check::new(
                        Status::Warn,
                        format!(
                            "settings: {shown} sets {}{place}, so {}",
                            conflict.name(),
                            conflict.effect()
                        ),
                    )
                }));
            }
        }
    }
    if checks.is_empty() {
        let files = match read {
            0 => "no settings files found".to_owned(),
            1 => "1 file checked".to_owned(),
            read => format!("{read} files checked"),
        };
        checks.push(Check::new(
            Status::Ok,
            format!("settings: nothing outranks cam's tokens ({files})"),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use toml::value::Datetime;

    use super::*;
    use crate::account::Selection;
    use crate::conflict::Conflict;
    use crate::resolve::Origin;
    use crate::settings::Scope;

    fn day(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    const TODAY: Date = Date {
        year: 2026,
        month: 11,
        day: 14,
    };

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    fn statuses(checks: &[Check]) -> Vec<Status> {
        checks.iter().map(|check| check.status).collect()
    }

    #[test]
    fn render_indents_later_lines() {
        let check = Check::new(Status::Warn, "first\nhint: second");
        assert_eq!(render(&check), ["warn  first", "      hint: second"]);
        assert_eq!(render(&Check::new(Status::Ok, "fine")), ["ok    fine"]);
    }

    #[test]
    fn tally_counts_problems_and_warnings() {
        let checks = |statuses: &[Status]| -> Vec<Check> {
            statuses
                .iter()
                .map(|status| Check::new(*status, "x"))
                .collect()
        };
        let shown = |statuses: &[Status]| tally(&checks(statuses)).to_string();
        assert_eq!(shown(&[Status::Ok, Status::Info]), "no problems found");
        assert_eq!(shown(&[Status::Warn]), "1 warning");
        assert_eq!(shown(&[Status::Fail, Status::Fail]), "2 problems");
        assert_eq!(
            shown(&[Status::Fail, Status::Warn, Status::Warn]),
            "1 problem, 2 warnings"
        );
    }

    #[test]
    fn token_age_thresholds() {
        let created = day(2025, 11, 14);
        let expires = day(2026, 11, 14);
        let at = |today| token_age(Some(created), today);
        assert_eq!(at(day(2025, 11, 14)), TokenAge::Good { created, expires });
        // 329 days old: still good. 330: time to renew.
        assert_eq!(at(day(2026, 10, 9)), TokenAge::Good { created, expires });
        assert_eq!(at(day(2026, 10, 10)), TokenAge::Renew { created, expires });
        // The last day of its year, then the day after.
        assert_eq!(at(day(2026, 11, 14)), TokenAge::Renew { created, expires });
        assert_eq!(
            at(day(2026, 11, 15)),
            TokenAge::Expired { created, expires }
        );
        assert_eq!(token_age(None, TODAY), TokenAge::Unknown);
        // A date in the future (a hand edit, or a clock change) is fine.
        assert!(matches!(
            token_age(Some(day(2027, 1, 1)), TODAY),
            TokenAge::Good { .. }
        ));
    }

    #[test]
    fn token_age_descriptions() {
        let created = day(2026, 11, 11);
        assert_eq!(
            token_age(Some(created), TODAY).describe(TODAY),
            "added 2026-11-11 (3 days ago), good until about 2027-11-11"
        );
        assert_eq!(
            token_age(Some(day(2025, 12, 1)), TODAY).describe(TODAY),
            "added 2025-12-01 (348 days ago), expires around 2026-12-01"
        );
        assert_eq!(
            token_age(Some(day(2025, 1, 1)), TODAY).describe(TODAY),
            "added 2025-01-01 (682 days ago), probably expired around 2026-01-01"
        );
    }

    #[test]
    fn renew_hint_names_the_account() {
        assert_eq!(
            renew_hint("work"),
            "hint: run `claude setup-token` and sign in with the account for work, then `cam add work --force`"
        );
    }

    fn account_with(created: Option<Date>) -> Account {
        Account {
            token_created: created.map(|date| Datetime {
                date: Some(date),
                time: None,
                offset: None,
            }),
        }
    }

    #[test]
    fn account_checks() {
        let check = |created, stored| {
            account(
                "work",
                &account_with(created),
                stored,
                "the macOS Keychain",
                TODAY,
            )
        };
        let good = check(Some(day(2026, 11, 11)), Ok(true));
        assert_eq!(good.status, Status::Ok);
        assert_eq!(
            good.message,
            "account work: token stored, added 2026-11-11 (3 days ago), good until about 2027-11-11"
        );

        let missing = check(Some(day(2026, 11, 11)), Ok(false));
        assert_eq!(missing.status, Status::Fail);
        assert_eq!(
            missing.message,
            "account work: no token in the macOS Keychain\nhint: add one with `cam add work`"
        );

        let old = check(Some(day(2025, 12, 1)), Ok(true));
        assert_eq!(old.status, Status::Warn);
        assert!(old.message.ends_with("--force`"), "{}", old.message);

        assert_eq!(check(Some(day(2025, 1, 1)), Ok(true)).status, Status::Fail);
        assert_eq!(check(None, Ok(true)).status, Status::Info);

        let broken = check(None, Err(Error::ClaudeNotFound));
        assert_eq!(broken.status, Status::Fail);
    }

    #[test]
    fn shim_checks() {
        let shim_path = PathBuf::from("/h/.local/share/cam/shims/claude");
        let real = PathBuf::from("/h/.local/bin/claude");
        let dir = Path::new("/h/.local/share/cam/shims");
        let status = |reaches, installed| shim(&reaches, installed).status;
        assert_eq!(status(Reaches::Shim(shim_path), Some(dir)), Status::Ok);
        assert_eq!(
            status(Reaches::Other(real.clone()), Some(dir)),
            Status::Warn
        );
        assert_eq!(status(Reaches::Nothing, Some(dir)), Status::Warn);
        assert_eq!(status(Reaches::Other(real), None), Status::Info);
        assert_eq!(status(Reaches::Nothing, None), Status::Info);
    }

    #[test]
    fn here_reports_the_resolution_or_the_error() {
        let ok = here(Ok(Resolution {
            selection: Selection::Account("work".to_owned()),
            origin: Origin::Env,
        }));
        assert_eq!(ok, Check::new(Status::Ok, "here: work (from CAM_ACCOUNT)"));
        let err = here(Err(Error::UnknownAccount {
            name: "typo".to_owned(),
            origin: Origin::Env,
        }));
        assert_eq!(err.status, Status::Fail);
        assert!(err.message.contains("hint: add it with `cam add typo`"));
    }

    #[test]
    fn environment_checks() {
        let clean = environment(env(&[]));
        assert_eq!(
            clean,
            [Check::new(
                Status::Ok,
                "environment: nothing outranks cam's tokens"
            )]
        );

        let conflicts = environment(env(&[
            ("ANTHROPIC_API_KEY", "SECRET-VALUE"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ]));
        assert_eq!(statuses(&conflicts), [Status::Warn, Status::Warn]);
        assert!(conflicts[0].message.contains("uses Amazon Bedrock"));

        // Inside a cam session: fine if the token stayed out of the shell.
        let hidden = environment(env(&[("CAM_ACTIVE_ACCOUNT", "work")]));
        assert_eq!(statuses(&hidden), [Status::Ok]);
        assert!(hidden[0].message.contains("started as work"));
        let visible = environment(env(&[
            ("CAM_ACTIVE_ACCOUNT", "work"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "SECRET-VALUE"),
        ]));
        assert_eq!(statuses(&visible), [Status::Warn]);

        // A token the user exported themselves.
        let exported = environment(env(&[("CLAUDE_CODE_OAUTH_TOKEN", "SECRET-VALUE")]));
        assert_eq!(statuses(&exported), [Status::Warn]);
        assert!(exported[0].message.contains("is exported"));
        // Values are never shown.
        for check in conflicts.iter().chain(&visible).chain(&exported) {
            assert!(!check.message.contains("SECRET"), "{}", check.message);
        }
    }

    #[test]
    fn settings_checks() {
        let file = |path: &str, scope| File {
            path: PathBuf::from(path),
            scope,
        };
        let checks = settings(&[
            (
                file("/h/.claude/settings.json", Scope::User),
                Inspection::Read(vec![Conflict::ApiKeyHelper, Conflict::Token]),
            ),
            (
                file("/w/.claude/settings.json", Scope::Project),
                Inspection::Missing,
            ),
            (
                file("/w/.claude/settings.local.json", Scope::Project),
                Inspection::Unreadable("not valid JSON (x)".to_owned()),
            ),
        ]);
        let messages: Vec<&str> = checks.iter().map(|check| check.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "settings: /h/.claude/settings.json sets apiKeyHelper, so Claude Code uses its key instead of cam's token or your /login",
                "settings: /h/.claude/settings.json sets CLAUDE_CODE_OAUTH_TOKEN in `env`, so it replaces the token cam passes",
                "settings: cannot check /w/.claude/settings.local.json: not valid JSON (x)",
            ]
        );
        assert_eq!(
            statuses(&checks),
            [Status::Warn, Status::Warn, Status::Warn]
        );

        let clean = settings(&[
            (
                file("/h/.claude/settings.json", Scope::User),
                Inspection::Read(vec![]),
            ),
            (
                file("/w/.claude/settings.json", Scope::Project),
                Inspection::Missing,
            ),
        ]);
        assert_eq!(
            clean,
            [Check::new(
                Status::Ok,
                "settings: nothing outranks cam's tokens (1 file checked)"
            )]
        );
        assert_eq!(
            settings(&[])[0].message,
            "settings: nothing outranks cam's tokens (no settings files found)"
        );
    }
}
