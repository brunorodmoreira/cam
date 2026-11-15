//! Errors from config handling, markers, resolution, account dirs, `claude
//! auth` and launching `claude`.
//!
//! Messages may contain user-supplied names, so everything printed goes
//! through [`crate::redact`] first.

use std::io;
use std::path::PathBuf;

use crate::account::NAME_RULE;
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

    #[error("invalid account name \"{name}\" in {origin}: names must match {rule}", rule = NAME_RULE)]
    InvalidName { name: String, origin: Origin },

    #[error("unknown account \"{name}\" (from {origin})\nhint: add it with `cam add {name}`")]
    UnknownAccount { name: String, origin: Origin },

    #[error("invalid account name \"{name}\": names must match {rule}", rule = NAME_RULE)]
    InvalidNameArg { name: String },

    #[error("unknown account \"{name}\"\nhint: add it with `cam add {name}`")]
    UnknownAccountArg { name: String },

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

    #[error("nothing to remove: \"{name}\" has no config entry and no account dir")]
    NothingToRemove { name: String },

    #[error("the login didn't finish ({status})")]
    LoginFailed { status: String },

    #[error("\"{name}\" isn't logged in\nhint: run `cam login {name}`")]
    NotLoggedIn { name: String },

    #[error("cannot read what `claude auth status` reports ({status})")]
    AuthStatus { status: String },

    #[error("`claude auth logout` failed ({status})")]
    Logout { status: String },

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
        "{} already exists and isn't a cam shim, so cam leaves it alone\nhint: remove it yourself, or pick another --dir",
        path.display()
    )]
    ShimOccupied { path: PathBuf },

    #[error("{} isn't a cam shim, so cam leaves it alone", path.display())]
    NotAShim { path: PathBuf },

    #[error("no shim at {}", path.display())]
    NoShim { path: PathBuf },

    #[error("cannot run {}", path.display())]
    Exec {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
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
