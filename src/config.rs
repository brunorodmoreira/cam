//! The config file: `$CAM_CONFIG`, else `~/.config/cam/config.toml`.
//! It holds account names and metadata, never secrets.
//!
//! Reading goes through serde and is validated strictly. Writing goes
//! through `toml_edit`, so the user's comments and formatting survive.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use serde::Deserialize;
use toml::value::{Date, Datetime};
use toml_edit::{DocumentMut, Item, Table};

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
pub struct Account {
    /// Expected identity, checked by `doctor`.
    pub email: Option<String>,
    pub token_created: Option<Datetime>,
}

/// What [`remove_account`] changed.
#[derive(Debug, PartialEq, Eq)]
pub struct Removed {
    pub existed: bool,
    pub was_default: bool,
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

    pub fn accounts(&self) -> &BTreeMap<String, Account> {
        &self.accounts
    }

    /// The configured `default`, if any.
    pub fn default_selection(&self) -> Option<Selection> {
        self.default.as_deref().and_then(Selection::parse)
    }
}

/// Records `name` with `token_created = created`, and `email` if given.
/// An existing entry keeps its other keys, including its email when `email`
/// is `None`.
pub fn add_account(
    path: &Path,
    name: &str,
    email: Option<&str>,
    created: Date,
) -> Result<(), Error> {
    edit(path, |doc| {
        let accounts = doc.entry("accounts").or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        });
        let accounts = accounts.as_table_like_mut()?;
        if accounts.get(name).is_none() {
            let mut table = Table::new();
            table.decor_mut().set_prefix("\n");
            accounts.insert(name, Item::Table(table));
        }
        let entry = accounts.get_mut(name)?.as_table_like_mut()?;
        if let Some(email) = email {
            entry.insert("email", toml_edit::value(email));
        }
        let created = Datetime {
            date: Some(created),
            time: None,
            offset: None,
        };
        entry.insert("token_created", toml_edit::value(created));
        Some(())
    })
}

/// Deletes the entry for `name`, and `default` if it named `name`.
pub fn remove_account(path: &Path, name: &str) -> Result<Removed, Error> {
    edit(path, |doc| {
        let existed = match doc.get_mut("accounts") {
            Some(accounts) => accounts.as_table_like_mut()?.remove(name).is_some(),
            None => false,
        };
        let was_default = doc.get("default").and_then(Item::as_str) == Some(name);
        if was_default {
            doc.remove("default");
        }
        Some(Removed {
            existed,
            was_default,
        })
    })
}

/// Applies `change` to the config document and writes it back if anything
/// changed. Refuses to edit a config that doesn't validate, and checks the
/// result validates too. `change` returns `None` if the document has a
/// shape it can't edit.
fn edit<T>(path: &Path, change: impl FnOnce(&mut DocumentMut) -> Option<T>) -> Result<T, Error> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(Error::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    Config::parse(&text, path)?;
    let mut doc: DocumentMut = text.parse().map_err(|err| Error::ParseConfig {
        path: path.to_owned(),
        source: Box::new(err),
    })?;
    let result = change(&mut doc).ok_or_else(|| Error::ConfigShape {
        path: path.to_owned(),
    })?;
    let mut new_text = doc.to_string();
    if text.trim().is_empty() {
        new_text = new_text.trim_start().to_owned();
    }
    if new_text != text {
        Config::parse(&new_text, path)?;
        write_atomic(path, &new_text)?;
    }
    Ok(result)
}

/// Writes via a temp file and a rename. A symlinked config (say, from a
/// dotfiles repo) is written through, so the link survives. An existing
/// file keeps its permissions.
fn write_atomic(path: &Path, text: &str) -> Result<(), Error> {
    let error = |source| Error::Write {
        path: path.to_owned(),
        source,
    };
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        Err(err) if err.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(err) => return Err(error(err)),
    };
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(error)?;
    }
    let file_name = target
        .file_name()
        .map_or_else(|| "config".into(), |name| name.to_string_lossy());
    let tmp = target.with_file_name(format!(".{file_name}.{}.tmp", process::id()));
    let written = fs::write(&tmp, text)
        .and_then(|()| match fs::metadata(&target) {
            Ok(meta) => fs::set_permissions(&tmp, meta.permissions()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        })
        .and_then(|()| fs::rename(&tmp, &target));
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(error(err));
    }
    Ok(())
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

    // --- editing ---

    use crate::testutil::Tree;

    const DAY: Date = Date {
        year: 2026,
        month: 11,
        day: 11,
    };

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn add_creates_missing_file_and_dirs() {
        let tree = Tree::new();
        let path = tree.root.join("nested/cam/config.toml");
        add_account(&path, "work", Some("you@company.com"), DAY).unwrap();
        assert_eq!(
            read(&path),
            "[accounts.work]\nemail = \"you@company.com\"\ntoken_created = 2026-11-11\n"
        );
        assert!(Config::load(&path).unwrap().has_account("work"));
    }

    #[test]
    fn add_without_email() {
        let tree = Tree::new();
        let path = tree.root.join("config.toml");
        add_account(&path, "work", None, DAY).unwrap();
        assert_eq!(read(&path), "[accounts.work]\ntoken_created = 2026-11-11\n");
    }

    #[test]
    fn add_keeps_comments_and_other_entries() {
        let tree = Tree::new();
        let path = tree.config(
            "# my accounts\ndefault = \"personal\" # most folders\n\n\
             [accounts.personal]\nemail = \"me@example.com\" # home\n",
        );
        add_account(&path, "work", Some("you@company.com"), DAY).unwrap();
        assert_eq!(
            read(&path),
            "# my accounts\ndefault = \"personal\" # most folders\n\n\
             [accounts.personal]\nemail = \"me@example.com\" # home\n\n\
             [accounts.work]\nemail = \"you@company.com\"\ntoken_created = 2026-11-11\n"
        );
    }

    #[test]
    fn add_updates_existing_entry_in_place() {
        let tree = Tree::new();
        let path = tree.config(
            "[accounts.work]\n# who this should be\nemail = \"old@company.com\"\ntoken_created = 2025-01-01\n\n\
             [accounts.personal]\n",
        );
        add_account(&path, "work", None, DAY).unwrap();
        let text = read(&path);
        assert!(
            text.contains("# who this should be\nemail = \"old@company.com\""),
            "{text}"
        );
        assert!(text.contains("token_created = 2026-11-11"), "{text}");
        assert!(!text.contains("2025-01-01"), "{text}");

        add_account(&path, "work", Some("new@company.com"), DAY).unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(
            config.accounts()["work"].email.as_deref(),
            Some("new@company.com")
        );
        assert!(config.has_account("personal"));
    }

    #[test]
    fn edit_refuses_an_invalid_config() {
        let tree = Tree::new();
        let broken = "defualt = \"work\"\n";
        let path = tree.config(broken);
        assert!(matches!(
            add_account(&path, "work", None, DAY),
            Err(Error::ParseConfig { .. })
        ));
        assert_eq!(read(&path), broken);
    }

    #[test]
    fn add_works_with_inline_account_tables() {
        let tree = Tree::new();
        let path = tree.config("accounts = { personal = {} }\n");
        add_account(&path, "work", None, DAY).unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.has_account("personal") && config.has_account("work"));
    }

    #[test]
    fn remove_deletes_entry_and_clears_default() {
        let tree = Tree::new();
        let path = tree.config(
            "default = \"work\"\n\n[accounts.work]\nemail = \"you@company.com\"\n\n[accounts.personal]\n",
        );
        assert_eq!(
            remove_account(&path, "work").unwrap(),
            Removed {
                existed: true,
                was_default: true
            }
        );
        let config = Config::load(&path).unwrap();
        assert!(!config.has_account("work"));
        assert!(config.has_account("personal"));
        assert_eq!(config.default_selection(), None);
    }

    #[test]
    fn remove_keeps_other_default() {
        let tree = Tree::new();
        let path = tree.config("default = \"personal\"\n[accounts.work]\n[accounts.personal]\n");
        assert_eq!(
            remove_account(&path, "work").unwrap(),
            Removed {
                existed: true,
                was_default: false
            }
        );
        assert_eq!(
            Config::load(&path).unwrap().default_selection(),
            Some(Selection::Account("personal".into()))
        );
    }

    #[test]
    fn remove_missing_entry_changes_nothing() {
        let tree = Tree::new();
        let original = "# keep me\n[accounts.personal]\n";
        let path = tree.config(original);
        assert_eq!(
            remove_account(&path, "work").unwrap(),
            Removed {
                existed: false,
                was_default: false
            }
        );
        assert_eq!(read(&path), original);

        let missing = tree.root.join("missing.toml");
        assert!(!remove_account(&missing, "work").unwrap().existed);
        assert!(!missing.exists(), "removing must not create the file");
    }

    #[test]
    fn writes_through_a_symlink() {
        let tree = Tree::new();
        let real = tree.dir("dotfiles").join("cam.toml");
        fs::write(&real, "[accounts.personal]\n").unwrap();
        let link = tree.dir("home/.config/cam").join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        add_account(&link, "work", None, DAY).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(read(&real).contains("[accounts.work]"));
    }

    #[test]
    fn keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tree = Tree::new();
        let path = tree.config("[accounts.personal]\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        add_account(&path, "work", None, DAY).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn failed_write_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let tree = Tree::new();
        let dir = tree.dir("locked");
        let path = dir.join("config.toml");
        fs::write(&path, "").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        let result = add_account(&path, "work", None, DAY);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(result, Err(Error::Write { .. })));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
}
