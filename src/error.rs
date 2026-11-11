//! Errors from config loading, marker reading and resolution.
//!
//! Messages may contain user-supplied names, so everything printed goes
//! through [`crate::redact`] first.

use std::io;
use std::path::PathBuf;

use crate::account::NAME_RULE;
use crate::resolve::Origin;

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

    #[error("{path} is not valid UTF-8")]
    NotUtf8 { path: PathBuf },

    #[error("cannot parse {path}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

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
}
