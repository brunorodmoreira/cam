//! `cam doctor`'s checks.
//!
//! Each check is ok, info, warn or fail. Only a fail makes `doctor` exit
//! non-zero. Logins are checked by asking `claude auth status`, never by
//! reading a credential.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use toml::value::Date;

use crate::account::SYSTEM;
use crate::auth::{self, Expiry};
use crate::config::Config;
use crate::conflict;
use crate::error::Error;
use crate::init::{self, Shell};
use crate::launch::{ENV_ACTIVE, ENV_CONFIG_DIR};
use crate::resolve::Resolution;
use crate::settings::{File, Inspection};
use crate::share::{self, Entry, Group, State};
use crate::shim::{self, Reaches};

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

// --- checks ---

/// The real `claude`, as `exec` would find it.
pub fn claude(found: Result<PathBuf, Error>) -> Check {
    match found {
        Ok(path) => Check::new(Status::Ok, format!("claude: {}", path.display())),
        Err(err) => Check::new(Status::Fail, format!("claude: {}", explain(&err))),
    }
}

/// What plain `claude` runs, going by `PATH`. `installed` is the shim dir,
/// if `cam init` has made the link in it. `shell` picks the setup line.
pub fn path(reaches: &Reaches, installed: Option<&Path>, shell: Option<Shell>) -> Check {
    let (file, line) = init::setup(shell);
    let setup = format!("add `{line}` to the end of {file}, then open a new terminal");
    match (reaches, installed) {
        (Reaches::Shim(path), _) => Check::new(
            Status::Ok,
            format!("path: plain `claude` runs through cam ({})", path.display()),
        ),
        (Reaches::Other(claude), Some(dir)) => Check::new(
            Status::Warn,
            format!(
                "path: plain `claude` runs {}, not cam\n\
                 hint: {setup}; it must come after any line that adds {}. \
                 If you'd rather not use cam for plain `claude`, delete {}",
                claude.display(),
                claude.parent().unwrap_or(claude).display(),
                dir.join(shim::NAME).display()
            ),
        ),
        (Reaches::Nothing, Some(dir)) => Check::new(
            Status::Warn,
            format!(
                "path: plain `claude` finds nothing on PATH\n\
                 hint: {setup}. If you'd rather not use cam for plain `claude`, delete {}",
                dir.join(shim::NAME).display()
            ),
        ),
        (Reaches::Other(claude), None) => Check::new(
            Status::Info,
            format!(
                "path: plain `claude` ({}) skips cam; `cam exec` uses the account\n\
                 hint: to have plain `claude` pick the account, {setup}",
                claude.display()
            ),
        ),
        (Reaches::Nothing, None) => Check::new(
            Status::Info,
            format!(
                "path: plain `claude` finds nothing on PATH\n\
                 hint: to have plain `claude` pick the account, {setup}"
            ),
        ),
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
    Check::new(Status::Ok, format!("config: {shown}, {count}"))
}

/// What asking Claude Code about a login gave.
pub enum Login {
    Checked(Result<auth::Status, Error>),
    /// There's no `claude` to ask; the `claude` check reports that.
    NoClaude,
}

/// "logged in as you@company.com (Acme, team), login expires ...", and
/// how the expiry stands.
fn logged_in(status: &auth::Status, today: Date) -> (String, Expiry) {
    let expiry = auth::expiry(status, today);
    let mut line = format!("logged in as {}", status.identity());
    if let Some(expires) = expiry.describe(today) {
        line.push_str(&format!(", login {expires}"));
    }
    (line, expiry)
}

/// Who Claude Code's own setup is logged in as. Not being logged in is
/// fine: someone may use only cam accounts.
pub fn system(login: &Login, today: Date) -> Check {
    let hint = format!("hint: run `cam login {SYSTEM}` to renew it");
    match login {
        Login::NoClaude => Check::new(
            Status::Info,
            "system: login not checked, since `claude` wasn't found",
        ),
        Login::Checked(Err(err)) => Check::new(
            Status::Warn,
            format!("system: cannot check the login: {}", explain(err)),
        ),
        Login::Checked(Ok(status)) if status.is_login() => {
            let (line, expiry) = logged_in(status, today);
            match expiry {
                Expiry::Good(_) | Expiry::Unknown => {
                    Check::new(Status::Ok, format!("system: {line}"))
                }
                Expiry::Soon(_) | Expiry::Expired(_) => {
                    Check::new(Status::Warn, format!("system: {line}\n{hint}"))
                }
            }
        }
        Login::Checked(Ok(status)) => match status.other_method() {
            Some(method) => Check::new(
                Status::Warn,
                format!("system: Claude Code uses {method} instead of a login"),
            ),
            None => Check::new(
                Status::Info,
                format!(
                    "system: Claude Code's own setup isn't logged in\nhint: run `cam login {SYSTEM}` if you use it"
                ),
            ),
        },
    }
}

/// One configured account: its dir, and its login.
pub fn account(name: &str, dir: &Path, dir_exists: bool, login: &Login, today: Date) -> Check {
    let subject = format!("account {name}");
    let login_hint = format!("hint: run `cam login {name}`");
    if !dir_exists {
        return Check::new(
            Status::Fail,
            format!(
                "{subject}: no dir at {}\nhint: set it up again with `cam add {name}`",
                dir.display()
            ),
        );
    }
    match login {
        Login::NoClaude => Check::new(
            Status::Info,
            format!("{subject}: login not checked, since `claude` wasn't found"),
        ),
        Login::Checked(Err(err)) => Check::new(
            Status::Fail,
            format!("{subject}: cannot check the login: {}", explain(err)),
        ),
        Login::Checked(Ok(status)) if status.is_login() => {
            let (line, expiry) = logged_in(status, today);
            match expiry {
                Expiry::Good(_) | Expiry::Unknown => {
                    Check::new(Status::Ok, format!("{subject}: {line}"))
                }
                Expiry::Soon(_) => Check::new(
                    Status::Warn,
                    format!("{subject}: {line}\n{login_hint} to renew it"),
                ),
                Expiry::Expired(_) => {
                    Check::new(Status::Fail, format!("{subject}: {line}\n{login_hint}"))
                }
            }
        }
        Login::Checked(Ok(status)) => match status.other_method() {
            Some(method) => Check::new(
                Status::Warn,
                format!("{subject}: Claude Code uses {method} instead of the account's login"),
            ),
            None => Check::new(
                Status::Fail,
                format!("{subject}: not logged in\n{login_hint}"),
            ),
        },
    }
}

/// The links of what account `name` shares, from their `states`, and the
/// entries `cam share` moved aside in its dir (`set_aside`).
pub fn links(name: &str, states: &[(Group, Entry, State)], set_aside: &[String]) -> Vec<Check> {
    let subject = format!("account {name}");
    let mut groups: Vec<Group> = states.iter().map(|(group, _, _)| *group).collect();
    groups.dedup();
    let mut checks = Vec::new();
    for (group, entry, state) in states {
        let shared = entry.shared_shown();
        match state {
            State::Linked => {}
            State::Missing => checks.push(Check::new(
                Status::Warn,
                format!(
                    "{subject}: {} isn't linked to {shared}\nhint: the next `claude` in {name} links it",
                    entry.name
                ),
            )),
            State::NotYet => checks.push(Check::new(
                Status::Info,
                format!(
                    "{subject}: {shared} doesn't exist yet; once it does, the next `claude` in {name} links it"
                ),
            )),
            State::Own(own) => checks.push(Check::new(
                Status::Warn,
                format!(
                    "{subject}: its {} is {}, not a link to {shared}, so {name} doesn't share it\n\
                     hint: run `cam share {name} {group}` to move it aside and link it",
                    entry.name,
                    own.describe()
                ),
            )),
        }
    }
    if !groups.is_empty() && checks.iter().all(|check| check.status != Status::Warn) {
        checks.insert(
            0,
            Check::new(
                Status::Ok,
                format!(
                    "{subject}: shares {}, linked to ~/.claude",
                    share::describe(&groups)
                ),
            ),
        );
    }
    checks.extend(set_aside.iter().map(|aside| {
        Check::new(
            Status::Info,
            format!(
                "{subject}: {aside} is {name}'s own copy from before it shared\n\
                 hint: copy what you want to keep from it into ~/.claude, then delete it"
            ),
        )
    }));
    checks
}

/// Accounts (and `system`) logged in as the same claude.ai account: the
/// same email in the same org. `logins` holds (name, email, org). One
/// email in two orgs is fine: a person can be in several.
pub fn same_account(logins: &[(String, String, Option<String>)]) -> Vec<Check> {
    let mut by_account: BTreeMap<(String, Option<&str>), Vec<&str>> = BTreeMap::new();
    for (name, email, org) in logins {
        by_account
            .entry((email.to_lowercase(), org.as_deref()))
            .or_default()
            .push(name.as_str());
    }
    by_account
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|((email, _), names)| {
            let fix = names
                .iter()
                .find(|name| **name != SYSTEM)
                .copied()
                .unwrap_or("<name>");
            let verb = if names.len() == 2 {
                "are both"
            } else {
                "are all"
            };
            Check::new(
                Status::Warn,
                format!(
                    "same account: {} {verb} logged in as {email}\n\
                     hint: if {fix} should be another claude.ai account, switch accounts in the browser, then run `cam login {fix}`",
                    and_list(&names)
                ),
            )
        })
        .collect()
}

/// "a and b", "a, b and c".
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The account for the current directory, as `which` reports it.
pub fn here(resolved: Result<Resolution, Error>) -> Check {
    match resolved {
        Ok(resolution) => Check::new(Status::Ok, format!("here: {resolution}")),
        Err(err) => Check::new(Status::Fail, format!("here: {}", explain(&err))),
    }
}

/// Variables in this shell that would beat the login, or move `system`.
pub fn environment(get: impl Fn(&str) -> Option<OsString>) -> Vec<Check> {
    let set = |var: &str| get(var).filter(|value| !value.is_empty());
    let mut checks: Vec<Check> = conflict::in_env(&get)
        .into_iter()
        .map(|conflict| {
            let hint = match conflict {
                conflict::Conflict::SharedStore => "unset it, so each account keeps its own login",
                _ => "unset it to use the account cam picks",
            };
            Check::new(
                Status::Warn,
                format!(
                    "environment: {} is set, so {}\nhint: {hint}",
                    conflict.name(),
                    conflict.effect()
                ),
            )
        })
        .collect();
    match (set(ENV_ACTIVE), set(ENV_CONFIG_DIR)) {
        (Some(name), _) => checks.push(Check::new(
            Status::Ok,
            format!(
                "environment: inside a `claude` that cam started as {}",
                name.to_string_lossy()
            ),
        )),
        (None, Some(dir)) => checks.push(Check::new(
            Status::Warn,
            format!(
                "environment: {ENV_CONFIG_DIR} is exported as {}, so `system` uses that instead of ~/.claude\n\
                 hint: unset it, unless you keep Claude Code's own setup there",
                PathBuf::from(dir).display()
            ),
        )),
        (None, None) => {}
    }
    if checks.is_empty() {
        checks.push(Check::new(
            Status::Ok,
            "environment: nothing outranks the login",
        ));
    }
    checks
}

/// What Claude Code's settings files would do to the login.
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
            format!("settings: nothing outranks the login ({files})"),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::account::Selection;
    use crate::conflict::Conflict;
    use crate::resolve::Origin;
    use crate::settings::Scope;

    const TODAY: Date = Date {
        year: 2026,
        month: 11,
        day: 15,
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

    fn login(expires: &str) -> Login {
        Login::Checked(Ok(auth::Status {
            logged_in: true,
            auth_method: Some(auth::CLAUDE_AI.to_owned()),
            email: Some("you@company.com".to_owned()),
            org_name: Some("Acme".to_owned()),
            subscription_type: Some("team".to_owned()),
            login_expires_at: Some(format!("{expires}T12:00:00.000Z")),
            ..auth::Status::default()
        }))
    }

    fn logged_out() -> Login {
        Login::Checked(Ok(auth::Status::default()))
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
    fn account_checks() {
        let dir = Path::new("/h/.local/share/cam/accounts/work");
        let check = |exists, login: &Login| account("work", dir, exists, login, TODAY);

        let good = check(true, &login("2026-12-13"));
        assert_eq!(good.status, Status::Ok);
        assert_eq!(
            good.message,
            "account work: logged in as you@company.com (Acme, team), login expires 2026-12-13 (in 28 days)"
        );

        let soon = check(true, &login("2026-11-17"));
        assert_eq!(soon.status, Status::Warn);
        assert!(
            soon.message
                .ends_with("in 2 days)\nhint: run `cam login work` to renew it"),
            "{}",
            soon.message
        );

        let expired = check(true, &login("2026-11-10"));
        assert_eq!(expired.status, Status::Fail);
        assert!(
            expired
                .message
                .contains("login expired 2026-11-10 (5 days ago)"),
            "{}",
            expired.message
        );

        let out = check(true, &logged_out());
        assert_eq!(
            out,
            Check::new(
                Status::Fail,
                "account work: not logged in\nhint: run `cam login work`"
            )
        );

        let no_dir = check(false, &login("2026-12-13"));
        assert_eq!(no_dir.status, Status::Fail);
        assert_eq!(
            no_dir.message,
            "account work: no dir at /h/.local/share/cam/accounts/work\nhint: set it up again with `cam add work`"
        );

        assert_eq!(check(true, &Login::NoClaude).status, Status::Info);
        let broken = check(
            true,
            &Login::Checked(Err(Error::AuthStatus {
                status: "exit status 2".to_owned(),
            })),
        );
        assert_eq!(broken.status, Status::Fail);

        let key = Login::Checked(Ok(auth::Status {
            logged_in: true,
            auth_method: Some("api_key".to_owned()),
            api_key_source: Some("apiKeyHelper".to_owned()),
            ..auth::Status::default()
        }));
        let key = check(true, &key);
        assert_eq!(key.status, Status::Warn);
        assert!(key.message.contains("uses apiKeyHelper"), "{}", key.message);
    }

    #[test]
    fn system_checks() {
        let good = system(&login("2026-12-13"), TODAY);
        assert_eq!(good.status, Status::Ok);
        assert!(
            good.message
                .starts_with("system: logged in as you@company.com")
        );
        // Not logged in, or about to expire, isn't a problem for cam.
        assert_eq!(system(&logged_out(), TODAY).status, Status::Info);
        assert_eq!(system(&login("2026-11-10"), TODAY).status, Status::Warn);
        assert_eq!(system(&Login::NoClaude, TODAY).status, Status::Info);
    }

    #[test]
    fn same_account_checks() {
        let logins = |list: &[(&str, &str, &str)]| -> Vec<(String, String, Option<String>)> {
            list.iter()
                .map(|(name, email, org)| {
                    (
                        (*name).to_owned(),
                        (*email).to_owned(),
                        Some((*org).to_owned()),
                    )
                })
                .collect()
        };
        assert!(
            same_account(&logins(&[
                ("system", "a@x.org", "Me"),
                ("work", "b@y.com", "Acme")
            ]))
            .is_empty()
        );
        // One person in two orgs.
        assert!(
            same_account(&logins(&[
                ("system", "a@x.org", "Me"),
                ("work", "a@x.org", "Acme")
            ]))
            .is_empty()
        );
        let both = same_account(&logins(&[
            ("system", "a@x.org", "Me"),
            ("work", "A@x.org", "Me"),
        ]));
        assert_eq!(statuses(&both), [Status::Warn]);
        assert_eq!(
            both[0].message,
            "same account: system and work are both logged in as a@x.org\n\
             hint: if work should be another claude.ai account, switch accounts in the browser, then run `cam login work`"
        );
        let three = same_account(&logins(&[
            ("system", "a@x.org", "Me"),
            ("personal", "a@x.org", "Me"),
            ("work", "a@x.org", "Me"),
        ]));
        assert!(
            three[0]
                .message
                .starts_with("same account: system, personal and work are all logged in"),
            "{}",
            three[0].message
        );
    }

    #[test]
    fn path_checks() {
        let shim_path = PathBuf::from("/h/.local/share/cam/shims/claude");
        let real = PathBuf::from("/h/.local/bin/claude");
        let dir = Path::new("/h/.local/share/cam/shims");
        let zsh = Some(Shell::Zsh);
        let status = |reaches, installed| path(&reaches, installed, zsh).status;
        assert_eq!(status(Reaches::Shim(shim_path), Some(dir)), Status::Ok);
        assert_eq!(
            status(Reaches::Other(real.clone()), Some(dir)),
            Status::Warn
        );
        assert_eq!(status(Reaches::Nothing, Some(dir)), Status::Warn);
        assert_eq!(status(Reaches::Other(real.clone()), None), Status::Info);
        assert_eq!(status(Reaches::Nothing, None), Status::Info);
        assert_eq!(
            path(&Reaches::Other(real), Some(dir), zsh).message,
            "path: plain `claude` runs /h/.local/bin/claude, not cam\n\
             hint: add `eval \"$(cam init zsh)\"` to the end of ~/.zshrc, then open a new terminal; \
             it must come after any line that adds /h/.local/bin. \
             If you'd rather not use cam for plain `claude`, delete /h/.local/share/cam/shims/claude"
        );
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
            hint: "add it with `cam add typo`".to_owned(),
        }));
        assert_eq!(err.status, Status::Fail);
        assert!(err.message.contains("hint: add it with `cam add typo`"));
    }

    #[test]
    fn environment_checks() {
        assert_eq!(
            environment(env(&[])),
            [Check::new(
                Status::Ok,
                "environment: nothing outranks the login"
            )]
        );

        let conflicts = environment(env(&[
            ("ANTHROPIC_API_KEY", "SECRET-VALUE"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "SECRET-VALUE"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ]));
        assert_eq!(
            statuses(&conflicts),
            [Status::Warn, Status::Warn, Status::Warn]
        );
        assert!(conflicts[0].message.contains("uses Amazon Bedrock"));
        for check in &conflicts {
            assert!(!check.message.contains("SECRET"), "{}", check.message);
        }

        let session = environment(env(&[
            ("CAM_ACTIVE_ACCOUNT", "work"),
            ("CLAUDE_CONFIG_DIR", "/h/.local/share/cam/accounts/work"),
        ]));
        assert_eq!(statuses(&session), [Status::Ok]);
        assert!(session[0].message.contains("started as work"));

        let exported = environment(env(&[("CLAUDE_CONFIG_DIR", "/elsewhere")]));
        assert_eq!(statuses(&exported), [Status::Warn]);
        assert!(
            exported[0].message.contains("exported as /elsewhere"),
            "{}",
            exported[0].message
        );

        let shared = environment(env(&[("CLAUDE_SECURESTORAGE_CONFIG_DIR", "")]));
        assert_eq!(
            shared,
            [Check::new(
                Status::Warn,
                "environment: CLAUDE_SECURESTORAGE_CONFIG_DIR is set, so every account keeps its login \
                 in the same place, and the last one logged in is used\n\
                 hint: unset it, so each account keeps its own login"
            )]
        );
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
                Inspection::Read(vec![
                    Conflict::ApiKeyHelper,
                    Conflict::Credential("CLAUDE_CODE_OAUTH_TOKEN"),
                ]),
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
                "settings: /h/.claude/settings.json sets apiKeyHelper, so Claude Code uses its key instead of the login",
                "settings: /h/.claude/settings.json sets CLAUDE_CODE_OAUTH_TOKEN in `env`, so Claude Code uses it instead of the login",
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
                "settings: nothing outranks the login (1 file checked)"
            )]
        );
        assert_eq!(
            settings(&[])[0].message,
            "settings: nothing outranks the login (no settings files found)"
        );
    }
}
