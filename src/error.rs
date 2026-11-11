//! Errors from config handling, markers, resolution and the token store.
//!
//! Messages may contain user-supplied names, so everything printed goes
//! through [`crate::redact`] first. Token errors never include the input.

use std::io;
use std::path::PathBuf;

use crate::account::NAME_RULE;
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

    #[error("invalid email \"{email}\"")]
    InvalidEmail { email: String },

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
}
