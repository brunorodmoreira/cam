//! The config file: `$CAM_CONFIG`, else `~/.config/cam/config.toml`.
//! It holds account names and metadata, never secrets.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::account::{NONE, Selection, is_valid_name};
use crate::error::Error;
use crate::resolve::Origin;

pub const ENV_CONFIG: &str = "CAM_CONFIG";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    default: Option<String>,
    #[serde(default)]
    accounts: BTreeMap<String, Account>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "read by `list` and `doctor` (milestones 2 and 5)")]
pub struct Account {
    /// Expected identity, checked by `doctor`.
    pub email: Option<String>,
    pub token_created: Option<toml::value::Datetime>,
}

/// `$CAM_CONFIG` if set and non-empty, else `<home>/.config/cam/config.toml`.
pub fn path(env: Option<OsString>, home: Option<PathBuf>) -> Result<PathBuf, Error> {
    match env {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => home
            .map(|home| home.join(".config/cam/config.toml"))
            .ok_or(Error::NoHome),
    }
}

impl Config {
    /// Loads and validates the config at `path`. A missing file is an empty config.
    pub fn load(path: &Path) -> Result<Self, Error> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, path),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    fn parse(text: &str, path: &Path) -> Result<Self, Error> {
        let config: Self = toml::from_str(text).map_err(|err| Error::ParseConfig {
            path: path.to_owned(),
            source: Box::new(err),
        })?;
        config.validate(path)?;
        Ok(config)
    }

    /// Account names must be valid and not `none`; `default` must be `none`
    /// or a configured account.
    fn validate(&self, path: &Path) -> Result<(), Error> {
        if let Some(name) = self
            .accounts
            .keys()
            .find(|name| !is_valid_name(name) || name.as_str() == NONE)
        {
            return Err(Error::InvalidConfigAccount {
                path: path.to_owned(),
                name: name.clone(),
            });
        }
        if let Some(name) = &self.default {
            let origin = || Origin::Default(path.to_owned());
            match Selection::parse(name) {
                None => {
                    return Err(Error::InvalidName {
                        name: name.clone(),
                        origin: origin(),
                    });
                }
                Some(Selection::Account(name)) if !self.has_account(&name) => {
                    return Err(Error::UnknownAccount {
                        name,
                        origin: origin(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    pub fn has_account(&self, name: &str) -> bool {
        self.accounts.contains_key(name)
    }

    /// The configured `default`, if any.
    pub fn default_selection(&self) -> Option<Selection> {
        self.default.as_deref().and_then(Selection::parse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, Error> {
        Config::parse(text, Path::new("/cfg/config.toml"))
    }

    #[test]
    fn parses_spec_example() {
        let config = parse(
            "default = \"personal\"\n\n\
             [accounts.work]\nemail = \"you@company.com\"\ntoken_created = 2026-11-11\n\n\
             [accounts.personal]\n",
        )
        .unwrap();
        assert_eq!(
            config.default_selection(),
            Some(Selection::Account("personal".into()))
        );
        assert!(config.has_account("work"));
        assert!(config.has_account("personal"));
        assert!(!config.has_account("oss"));
        let work = &config.accounts["work"];
        assert_eq!(work.email.as_deref(), Some("you@company.com"));
        assert_eq!(work.token_created.unwrap().to_string(), "2026-11-11");
    }

    #[test]
    fn empty_config() {
        let config = parse("").unwrap();
        assert_eq!(config.default_selection(), None);
        assert!(!config.has_account("work"));
    }

    #[test]
    fn missing_file_is_empty_config() {
        let tree = crate::testutil::Tree::new();
        let config = Config::load(&tree.root.join("nope.toml")).unwrap();
        assert_eq!(config.default_selection(), None);
    }

    #[test]
    fn load_reads_file() {
        let tree = crate::testutil::Tree::new();
        let path = tree.config("[accounts.work]\n");
        assert!(Config::load(&path).unwrap().has_account("work"));
    }

    #[test]
    fn rejects_bad_toml() {
        assert!(matches!(
            parse("default = "),
            Err(Error::ParseConfig { .. })
        ));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(matches!(
            parse("defualt = \"work\"\n[accounts.work]\n"),
            Err(Error::ParseConfig { .. })
        ));
        assert!(matches!(
            parse("[accounts.work]\nemial = \"x@y.z\"\n"),
            Err(Error::ParseConfig { .. })
        ));
    }

    #[test]
    fn rejects_invalid_account_names() {
        for key in ["Work", "\"-x\"", "\"a b\""] {
            let err = parse(&format!("[accounts.{key}]\n")).unwrap_err();
            assert!(matches!(err, Error::InvalidConfigAccount { .. }), "{key}");
        }
    }

    #[test]
    fn rejects_none_as_account_name() {
        assert!(matches!(
            parse("[accounts.none]\n"),
            Err(Error::InvalidConfigAccount { name, .. }) if name == "none"
        ));
    }

    #[test]
    fn rejects_unknown_default() {
        let err = parse("default = \"ghost\"\n[accounts.work]\n").unwrap_err();
        assert!(matches!(
            &err,
            Error::UnknownAccount { name, origin: Origin::Default(_) } if name == "ghost"
        ));
        assert!(err.to_string().contains("cam add ghost"));
    }

    #[test]
    fn rejects_invalid_default() {
        assert!(matches!(
            parse("default = \"Work\"\n"),
            Err(Error::InvalidName {
                origin: Origin::Default(_),
                ..
            })
        ));
    }

    #[test]
    fn default_none_is_pass_through() {
        let config = parse("default = \"none\"\n").unwrap();
        assert_eq!(config.default_selection(), Some(Selection::PassThrough));
    }

    #[test]
    fn path_prefers_env() {
        assert_eq!(
            path(Some("/x/cam.toml".into()), Some("/home/me".into())).unwrap(),
            PathBuf::from("/x/cam.toml")
        );
    }

    #[test]
    fn path_falls_back_to_home() {
        let expected = PathBuf::from("/home/me/.config/cam/config.toml");
        assert_eq!(path(None, Some("/home/me".into())).unwrap(), expected);
        assert_eq!(
            path(Some("".into()), Some("/home/me".into())).unwrap(),
            expected
        );
    }

    #[test]
    fn path_without_home_or_env() {
        assert!(matches!(path(None, None), Err(Error::NoHome)));
    }
}
