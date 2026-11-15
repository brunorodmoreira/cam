//! Errors from config handling, markers, resolution, account dirs, `claude
//! auth` and launching `claude`.
//!
//! Messages may contain user-supplied names, so everything printed goes
//! through [`crate::redact`] first.

use std::io;
use std::path::{Path, PathBuf};

use crate::account::{self, NAME_RULE};
use crate::discover::ENV_CLAUDE_BIN;
use crate::launch::ENV_DEPTH;
use crate::resolve::Origin;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot find the home directory; set HOME")]
    NoHome,

    #[error("CAM_ACCOUNT is not valid UTF-8")]
    EnvNotUnicode,

    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is not valid UTF-8")]
    NotUtf8 { path: PathBuf },

    #[error("cannot parse {path}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: BoxError,
    },

    #[error("cannot edit {path}: write accounts as [accounts.<name>] tables")]
    ConfigShape { path: PathBuf },

    #[error(
        "{path} is from cam's token mode (it has `default` or `token_created`)\n\
         hint: delete those lines, then run `cam add <name>` for each account. Old tokens stay in the \
         Keychain until you delete them: `security delete-generic-password -s cam -a <name>`"
    )]
    TokenModeConfig { path: PathBuf },

    #[error(
        "invalid account [accounts.{name}] in {path}: names must match {rule} and \"system\" is reserved",
        rule = NAME_RULE
    )]
    InvalidConfigAccount { path: PathBuf, name: String },

    #[error(
        "{path} names no account\nhint: put an account name (or `system`) in it, or remove it with `cam unuse`"
    )]
    EmptyMarker { path: PathBuf },

    #[error(
        "invalid account name \"{name}\" in {origin}: names must match {rule}{}",
        name_hint(.name),
        rule = NAME_RULE
    )]
    InvalidName { name: String, origin: Origin },

    /// `hint` comes from [`unknown_hint`].
    #[error("unknown account \"{name}\" (from {origin})\nhint: {hint}")]
    UnknownAccount {
        name: String,
        origin: Origin,
        hint: String,
    },

    #[error(
        "invalid account name \"{name}\": names must match {rule}{}",
        name_hint(.name),
        rule = NAME_RULE
    )]
    InvalidNameArg { name: String },

    /// `hint` comes from [`unknown_hint`].
    #[error("unknown account \"{name}\"\nhint: {hint}")]
    UnknownAccountArg { name: String, hint: String },

    #[error("\"system\" is reserved: it's Claude Code's own setup, not an account")]
    ReservedName,

    #[error(
        "{} already exists\nhint: to log in again, run `cam login {name}`; to start over, `cam remove {name}` first",
        dir.display()
    )]
    AccountExists { name: String, dir: PathBuf },

    #[error(
        "account \"{name}\" has no dir at {}\nhint: set it up again with `cam add {name}`",
        dir.display()
    )]
    NoAccountDir { name: String, dir: PathBuf },

    #[error("nothing to remove: \"{name}\" has no config entry and no account dir\nhint: {hint}")]
    NothingToRemove { name: String, hint: String },

    #[error(
        "not removing \"{name}\" without asking, and there's no terminal to ask on\nhint: pass --yes to remove it"
    )]
    Unconfirmed { name: String },

    /// `retry` is the command that tries again.
    #[error("the login didn't finish ({status})\nhint: run `{retry}` to try again")]
    LoginFailed { status: String, retry: String },

    #[error(
        "Claude Code logged in with {method}, not a Claude subscription\n\
         hint: log in with claude.ai (Pro, Max, Team or Enterprise). Claude Code keeps a Console \
         login outside the account's dir, so cam can't keep one per account"
    )]
    NoSubscription { method: String },

    #[error("\"{name}\" isn't logged in\nhint: run `cam login {name}`")]
    NotLoggedIn { name: String },

    #[error("cannot read what `claude auth status` reports ({status})")]
    AuthStatus { status: String },

    /// `output` is what `claude` printed, if anything.
    #[error("`claude auth logout` failed ({status}){}", output.as_deref().map(|text| format!(": {text}")).unwrap_or_default())]
    Logout {
        status: String,
        output: Option<String>,
    },

    #[error(
        "cannot find the real `claude` on PATH\nhint: install Claude Code, or set {ENV_CLAUDE_BIN} to its full path"
    )]
    ClaudeNotFound,

    #[error("{ENV_CLAUDE_BIN} must be an absolute path, not \"{}\"", value.display())]
    ClaudeBinNotAbsolute { value: PathBuf },

    #[error("{ENV_CLAUDE_BIN} is {}, which is not an executable file", path.display())]
    ClaudeBinMissing { path: PathBuf },

    #[error(
        "{ENV_CLAUDE_BIN} is {}, which is cam itself; point it at the real `claude`",
        path.display()
    )]
    ClaudeBinIsCam { path: PathBuf },

    #[error(
        "`claude` keeps leading back to cam ({ENV_DEPTH} is {depth})\nhint: set {ENV_CLAUDE_BIN} to the real `claude` binary"
    )]
    ShimLoop { depth: u32 },

    #[error("cannot locate cam's own executable")]
    CurrentExe(#[source] io::Error),

    #[error(
        "{} already exists and isn't cam's link, so cam leaves it alone\n\
         hint: plain `claude` can't go through cam until you move it away",
        path.display()
    )]
    ShimOccupied { path: PathBuf },

    #[error("cannot run {}", path.display())]
    Exec {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// "\nhint: try `work`", for an invalid name that has a valid one close by.
fn name_hint(name: &str) -> String {
    account::suggest_name(name)
        .map(|name| format!("\nhint: try `{name}`"))
        .unwrap_or_default()
}

/// Where an unknown account name came from, for [`unknown_hint`].
#[derive(Debug, Clone, Copy)]
pub enum Named<'a> {
    /// A marker in this dir.
    Marker(&'a Path),
    Env,
    /// A command-line argument.
    Arg,
}

/// The hint for an unknown account `name`, given the `known` ones:
/// `system` for token mode's `none`, a close name for a typo, else `cam add`.
pub fn unknown_hint<'a>(
    name: &str,
    known: impl IntoIterator<Item = &'a str>,
    named: Named<'_>,
) -> String {
    let fix = |to: &str| match named {
        Named::Marker(dir) => Some(format!(
            "Fix it with `cam use {to} --dir {}`",
            dir.display()
        )),
        Named::Env => Some(format!("Set CAM_ACCOUNT={to}")),
        Named::Arg => None,
    };
    if name == "none" {
        let fix = fix(account::SYSTEM)
            .map(|fix| format!(". {fix}"))
            .unwrap_or_default();
        return format!(
            "`none` was older cam's name for Claude Code's own setup, which is `system` now{fix}"
        );
    }
    let add = format!("add \"{name}\" with `cam add {name}`");
    match (account::closest(name, known), named) {
        (Some(close), Named::Arg) => format!("did you mean `{close}`? Or {add}"),
        (Some(close), _) => format!(
            "did you mean `{close}`? {}, or {add}",
            fix(close).unwrap_or_default()
        ),
        (None, _) => format!("add it with `cam add {name}`"),
    }
}

impl Error {
    /// The process exit code. Like `env` and the shell: 127 when `claude`
    /// can't be found, 126 when it can't be run, else 1.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::ClaudeNotFound | Self::ClaudeBinMissing { .. } => 127,
            Self::Exec { source, .. } if source.kind() == io::ErrorKind::NotFound => 127,
            Self::Exec { .. } => 126,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_hints() {
        let known = ["work", "personal"];
        let dir = Path::new("/home/me/Work");
        assert_eq!(
            unknown_hint("wrok", known, Named::Marker(dir)),
            "did you mean `work`? Fix it with `cam use work --dir /home/me/Work`, or add \"wrok\" with `cam add wrok`"
        );
        assert_eq!(
            unknown_hint("wrok", known, Named::Env),
            "did you mean `work`? Set CAM_ACCOUNT=work, or add \"wrok\" with `cam add wrok`"
        );
        assert_eq!(
            unknown_hint("wrok", known, Named::Arg),
            "did you mean `work`? Or add \"wrok\" with `cam add wrok`"
        );
        assert_eq!(
            unknown_hint("client", known, Named::Arg),
            "add it with `cam add client`"
        );
        assert_eq!(
            unknown_hint("none", ["nine"], Named::Marker(dir)),
            "`none` was older cam's name for Claude Code's own setup, which is `system` now. Fix it with `cam use system --dir /home/me/Work`"
        );
        assert_eq!(
            unknown_hint("none", known, Named::Arg),
            "`none` was older cam's name for Claude Code's own setup, which is `system` now"
        );
    }

    #[test]
    fn invalid_names_get_a_suggestion() {
        assert_eq!(
            Error::InvalidNameArg {
                name: "Work".into()
            }
            .to_string(),
            "invalid account name \"Work\": names must match ^[a-z0-9][a-z0-9_-]*$\nhint: try `work`"
        );
        assert_eq!(
            Error::InvalidNameArg { name: "--".into() }.to_string(),
            "invalid account name \"--\": names must match ^[a-z0-9][a-z0-9_-]*$"
        );
    }
}
