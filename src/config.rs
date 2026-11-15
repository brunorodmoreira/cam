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
use toml_edit::{DocumentMut, Item, Table};

use crate::account::{SYSTEM, is_valid_name};
use crate::error::Error;

pub const ENV_CONFIG: &str = "CAM_CONFIG";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    accounts: BTreeMap<String, Account>,
}

/// An account's settings. None yet: M8 adds what it shares.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {}

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
        let config: Self = toml::from_str(text).map_err(|err| {
            if is_token_mode(text) {
                Error::TokenModeConfig {
                    path: path.to_owned(),
                }
            } else {
                Error::ParseConfig {
                    path: path.to_owned(),
                    source: Box::new(err),
                }
            }
        })?;
        config.validate(path)?;
        Ok(config)
    }

    /// Account names must be valid and not `system`.
    fn validate(&self, path: &Path) -> Result<(), Error> {
        if let Some(name) = self
            .accounts
            .keys()
            .find(|name| !is_valid_name(name) || name.as_str() == SYSTEM)
        {
            return Err(Error::InvalidConfigAccount {
                path: path.to_owned(),
                name: name.clone(),
            });
        }
        Ok(())
    }

    pub fn has_account(&self, name: &str) -> bool {
        self.accounts.contains_key(name)
    }

    pub fn accounts(&self) -> &BTreeMap<String, Account> {
        &self.accounts
    }
}

/// Whether `text` is a config from token mode (M1–M6): it has a top-level
/// `default`, or an account with `token_created`.
fn is_token_mode(text: &str) -> bool {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return false;
    };
    let has_token_date = table
        .get("accounts")
        .and_then(toml::Value::as_table)
        .is_some_and(|accounts| {
            accounts.values().any(|account| {
                account
                    .as_table()
                    .is_some_and(|account| account.contains_key("token_created"))
            })
        });
    table.contains_key("default") || has_token_date
}

/// Records `name`. An existing entry is left as it is, comments included.
pub fn add_account(path: &Path, name: &str) -> Result<(), Error> {
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
        Some(())
    })
}

/// Deletes the entry for `name`. Returns whether there was one.
pub fn remove_account(path: &Path, name: &str) -> Result<bool, Error> {
    edit(path, |doc| match doc.get_mut("accounts") {
        Some(accounts) => Some(accounts.as_table_like_mut()?.remove(name).is_some()),
        None => Some(false),
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
    // Don't start the file with blank lines it didn't have, as when the
    // first table goes and the next one's leading blank line stays.
    if !text.starts_with(char::is_whitespace) || text.trim().is_empty() {
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
    use crate::testutil::Tree;

    fn parse(text: &str) -> Result<Config, Error> {
        Config::parse(text, Path::new("/cfg/config.toml"))
    }

    #[test]
    fn parses_spec_example() {
        let config = parse("[accounts.work]\n\n[accounts.personal]\n").unwrap();
        assert!(config.has_account("work"));
        assert!(config.has_account("personal"));
        assert!(!config.has_account("oss"));
    }

    #[test]
    fn empty_config() {
        assert!(parse("").unwrap().accounts().is_empty());
    }

    #[test]
    fn missing_file_is_empty_config() {
        let tree = Tree::new();
        let config = Config::load(&tree.root.join("nope.toml")).unwrap();
        assert!(config.accounts().is_empty());
    }

    #[test]
    fn load_reads_file() {
        let tree = Tree::new();
        let path = tree.config("[accounts.work]\n");
        assert!(Config::load(&path).unwrap().has_account("work"));
    }

    #[test]
    fn rejects_bad_toml() {
        assert!(matches!(
            parse("accounts = "),
            Err(Error::ParseConfig { .. })
        ));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(matches!(
            parse("acounts = 1\n[accounts.work]\n"),
            Err(Error::ParseConfig { .. })
        ));
        assert!(matches!(
            parse("[accounts.work]\nemail = \"x@y.z\"\n"),
            Err(Error::ParseConfig { .. })
        ));
    }

    #[test]
    fn explains_a_token_mode_config() {
        for text in [
            "default = \"work\"\n[accounts.work]\n",
            "[accounts.work]\ntoken_created = 2026-11-11\n",
        ] {
            let err = parse(text).unwrap_err();
            assert!(matches!(err, Error::TokenModeConfig { .. }), "{text}");
            assert!(err.to_string().contains("token mode"), "{err}");
        }
    }

    #[test]
    fn rejects_invalid_account_names() {
        for key in ["Work", "\"-x\"", "\"a b\""] {
            let err = parse(&format!("[accounts.{key}]\n")).unwrap_err();
            assert!(matches!(err, Error::InvalidConfigAccount { .. }), "{key}");
        }
    }

    #[test]
    fn rejects_system_as_account_name() {
        assert!(matches!(
            parse("[accounts.system]\n"),
            Err(Error::InvalidConfigAccount { name, .. }) if name == "system"
        ));
        // Token mode's reserved name is an ordinary one now.
        assert!(parse("[accounts.none]\n").unwrap().has_account("none"));
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

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn add_creates_missing_file_and_dirs() {
        let tree = Tree::new();
        let path = tree.root.join("nested/cam/config.toml");
        add_account(&path, "work").unwrap();
        assert_eq!(read(&path), "[accounts.work]\n");
        assert!(Config::load(&path).unwrap().has_account("work"));
    }

    #[test]
    fn add_keeps_comments_and_other_entries() {
        let tree = Tree::new();
        let path = tree.config("# my accounts\n\n[accounts.personal] # home\n");
        add_account(&path, "work").unwrap();
        assert_eq!(
            read(&path),
            "# my accounts\n\n[accounts.personal] # home\n\n[accounts.work]\n"
        );
    }

    #[test]
    fn add_leaves_an_existing_entry_alone() {
        let tree = Tree::new();
        let original = "# work stuff\n[accounts.work] # day job\n\n[accounts.personal]\n";
        let path = tree.config(original);
        add_account(&path, "work").unwrap();
        assert_eq!(read(&path), original);
    }

    #[test]
    fn edit_refuses_an_invalid_config() {
        let tree = Tree::new();
        let broken = "acounts = 1\n";
        let path = tree.config(broken);
        assert!(matches!(
            add_account(&path, "work"),
            Err(Error::ParseConfig { .. })
        ));
        assert_eq!(read(&path), broken);
    }

    #[test]
    fn add_works_with_inline_account_tables() {
        let tree = Tree::new();
        let path = tree.config("accounts = { personal = {} }\n");
        add_account(&path, "work").unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.has_account("personal") && config.has_account("work"));
    }

    #[test]
    fn remove_deletes_the_entry() {
        let tree = Tree::new();
        let path = tree.config("[accounts.work]\n\n[accounts.personal]\n");
        assert!(remove_account(&path, "work").unwrap());
        let config = Config::load(&path).unwrap();
        assert!(!config.has_account("work"));
        assert!(config.has_account("personal"));
    }

    #[test]
    fn remove_the_first_entry_leaves_no_leading_blank_line() {
        let tree = Tree::new();
        let path = tree.config("[accounts.work]\n\n[accounts.personal]\n");
        assert!(remove_account(&path, "work").unwrap());
        assert_eq!(read(&path), "[accounts.personal]\n");
    }

    #[test]
    fn remove_missing_entry_changes_nothing() {
        let tree = Tree::new();
        let original = "# keep me\n[accounts.personal]\n";
        let path = tree.config(original);
        assert!(!remove_account(&path, "work").unwrap());
        assert_eq!(read(&path), original);

        let missing = tree.root.join("missing.toml");
        assert!(!remove_account(&missing, "work").unwrap());
        assert!(!missing.exists(), "removing must not create the file");
    }

    #[test]
    fn writes_through_a_symlink() {
        let tree = Tree::new();
        let real = tree.dir("dotfiles").join("cam.toml");
        fs::write(&real, "[accounts.personal]\n").unwrap();
        let link = tree.dir("home/.config/cam").join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        add_account(&link, "work").unwrap();
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
        add_account(&path, "work").unwrap();
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
        let result = add_account(&path, "work");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(result, Err(Error::Write { .. })));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
}
