//! Errors from config handling, markers, resolution, the token store and
//! launching `claude`.
//!
//! Messages may contain user-supplied names, so everything printed goes
//! through [`crate::redact`] first. Token errors never include the input.

use std::io;
use std::path::PathBuf;

use crate::account::NAME_RULE;
use crate::discover::ENV_CLAUDE_BIN;
use crate::launch::ENV_DEPTH;
use crate::resolve::Origin;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot find the home directory; set CAM_CONFIG")]
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
        "invalid account [accounts.{name}] in {path}: names must match {rule} and \"none\" is reserved",
        rule = NAME_RULE
    )]
    InvalidConfigAccount { path: PathBuf, name: String },

    #[error(
        "{path} names no account\nhint: put an account name (or `none`) in it, or remove it with `cam unuse`"
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

    #[error("\"none\" is reserved and can't be an account name")]
    ReservedName,

    #[error("cannot read the token")]
    ReadToken(#[source] io::Error),

    #[error("no token entered")]
    TokenEmpty,

    #[error(
        "that looks like an Anthropic API key; cam needs the OAuth token that `claude setup-token` prints"
    )]
    TokenIsApiKey,

    #[error(
        "that doesn't look like a token from `claude setup-token` (it starts with sk-ant- and has no spaces)"
    )]
    TokenMalformed,

    #[error(
        "a token for \"{name}\" is already stored in {location}\nhint: pass --force to replace it"
    )]
    TokenExists { name: String, location: String },

    #[error("nothing to remove: \"{name}\" has no config entry and no stored token")]
    NothingToRemove { name: String },

    #[error("cannot {action} the token for \"{account}\" in {location}")]
    Store {
        action: &'static str,
        account: String,
        location: String,
        #[source]
        source: BoxError,
    },

    #[error("no token stored for \"{name}\" in {location}\nhint: add one with `cam add {name}`")]
    NoToken { name: String, location: String },

    #[error(
        "the token stored for \"{name}\" in {location} is not valid\nhint: replace it with `cam add {name} --force`"
    )]
    StoredTokenInvalid { name: String, location: String },

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
