//! Account resolution. First match wins:
//!
//! 1. `CAM_ACCOUNT` (a name, or `system`).
//! 2. The nearest `.claude-account`, from the current directory up to `/`.
//! 3. Nothing: `system`, Claude Code's own setup.
//!
//! Unknown or invalid names are hard errors; resolution never falls back to
//! a lower step. The config is read only to check that a name exists, so
//! `system` works even when the config is broken.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::account::Selection;
use crate::config::Config;
use crate::error::Error;
use crate::marker;

pub const ENV_ACCOUNT: &str = "CAM_ACCOUNT";

/// Where a resolution came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Env,
    Marker(PathBuf),
    Nothing,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env => f.write_str(ENV_ACCOUNT),
            Self::Marker(path) => write!(f, "{}", path.display()),
            Self::Nothing => write!(f, "no {ENV_ACCOUNT} or {}", marker::FILE_NAME),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub selection: Selection,
    pub origin: Origin,
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Origin::Nothing => write!(f, "{} ({})", self.selection, self.origin),
            origin => write!(f, "{} (from {origin})", self.selection),
        }
    }
}

/// `CAM_ACCOUNT` from the process environment.
pub fn env_account() -> Result<Option<String>, Error> {
    match env::var(ENV_ACCOUNT) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(Error::EnvNotUnicode),
    }
}

/// Resolves the account for `cwd`. `env_account` is the value of
/// `CAM_ACCOUNT`; empty or blank counts as unset.
pub fn resolve(
    env_account: Option<&str>,
    cwd: &Path,
    config_path: &Path,
) -> Result<Resolution, Error> {
    let (name, origin) = match env_account.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => (name.to_owned(), Origin::Env),
        None => match marker::find(cwd)? {
            Some((path, name)) => (name, Origin::Marker(path)),
            None => {
                return Ok(Resolution {
                    selection: Selection::System,
                    origin: Origin::Nothing,
                });
            }
        },
    };
    let Some(selection) = Selection::parse(&name) else {
        return Err(Error::InvalidName { name, origin });
    };
    if let Selection::Account(name) = &selection
        && !Config::load(config_path)?.has_account(name)
    {
        return Err(Error::UnknownAccount {
            name: name.clone(),
            origin,
        });
    }
    Ok(Resolution { selection, origin })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testutil::Tree;

    const CONFIG: &str = "[accounts.work]\n\
                          [accounts.personal]\n\
                          [accounts.oss]\n";
    const BROKEN_CONFIG: &str = "this is not toml";

    fn account(name: &str, origin: Origin) -> Resolution {
        Resolution {
            selection: Selection::Account(name.into()),
            origin,
        }
    }

    fn system(origin: Origin) -> Resolution {
        Resolution {
            selection: Selection::System,
            origin,
        }
    }

    // --- Step 2: nearest marker ---

    #[test]
    fn marker_in_cwd() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        let marker = tree.marker("work", "work\n");
        assert_eq!(
            resolve(None, &tree.dir("work"), &config).unwrap(),
            account("work", Origin::Marker(marker))
        );
    }

    #[test]
    fn marker_in_ancestor_covers_nested_dirs() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        let marker = tree.marker("work", "work\n");
        assert_eq!(
            resolve(None, &tree.dir("work/client/repo/src"), &config).unwrap(),
            account("work", Origin::Marker(marker))
        );
    }

    #[test]
    fn nearest_marker_wins() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("", "personal\n");
        let inner = tree.marker("work", "work\n");
        assert_eq!(
            resolve(None, &tree.dir("work/repo"), &config).unwrap(),
            account("work", Origin::Marker(inner))
        );
        assert_eq!(
            resolve(None, &tree.dir("hobby"), &config).unwrap(),
            account(
                "personal",
                Origin::Marker(tree.root.join(".claude-account"))
            )
        );
    }

    #[test]
    fn system_marker_overrides_parent() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("work", "work\n");
        let none = tree.marker("work/oss", "system\n");
        assert_eq!(
            resolve(None, &tree.dir("work/oss/repo"), &config).unwrap(),
            system(Origin::Marker(none))
        );
    }

    #[test]
    fn system_marker_does_not_read_config() {
        let tree = Tree::new();
        let config = tree.config(BROKEN_CONFIG);
        let none = tree.marker("oss", "system\n");
        assert_eq!(
            resolve(None, &tree.dir("oss"), &config).unwrap(),
            system(Origin::Marker(none))
        );
    }

    #[test]
    fn marker_comments_and_whitespace() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        let marker = tree.marker("w", "# pinned by cam\n\n   work   # day job\r\npersonal\n");
        assert_eq!(
            resolve(None, &tree.dir("w"), &config).unwrap(),
            account("work", Origin::Marker(marker))
        );
    }

    #[test]
    fn unknown_marker_name_is_an_error_not_a_fallback() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("", "work\n");
        let ghost = tree.marker("a", "ghost\n");
        let err = resolve(None, &tree.dir("a/b"), &config).unwrap_err();
        assert!(matches!(
            &err,
            Error::UnknownAccount { name, origin: Origin::Marker(path) } if name == "ghost" && *path == ghost
        ));
        assert!(err.to_string().contains("cam add ghost"));
    }

    #[test]
    fn invalid_marker_name_is_an_error() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("", "work\n");
        for bad in ["Work", "-work", "work account", "wörk"] {
            let marker = tree.marker("a", bad);
            let err = resolve(None, &tree.dir("a"), &config).unwrap_err();
            assert!(
                matches!(&err, Error::InvalidName { name, origin: Origin::Marker(path) } if name == bad && *path == marker),
                "{bad:?}: {err:?}"
            );
        }
    }

    #[test]
    fn empty_marker_is_an_error_not_skipped() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("", "work\n");
        tree.marker("a", "# todo\n\n");
        assert!(matches!(
            resolve(None, &tree.dir("a"), &config),
            Err(Error::EmptyMarker { .. })
        ));
    }

    #[test]
    fn unreadable_marker_is_an_error_not_skipped() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("", "work\n");
        fs::create_dir(tree.dir("a").join(".claude-account")).unwrap();
        assert!(matches!(
            resolve(None, &tree.dir("a"), &config),
            Err(Error::Read { .. })
        ));
    }

    #[test]
    fn non_utf8_marker_is_an_error() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        fs::write(tree.dir("a").join(".claude-account"), [0xc3, 0x28]).unwrap();
        assert!(matches!(
            resolve(None, &tree.dir("a"), &config),
            Err(Error::NotUtf8 { .. })
        ));
    }

    #[test]
    fn broken_config_fails_when_a_name_must_be_checked() {
        let tree = Tree::new();
        let config = tree.config(BROKEN_CONFIG);
        tree.marker("w", "work\n");
        assert!(matches!(
            resolve(None, &tree.dir("w"), &config),
            Err(Error::ParseConfig { .. })
        ));
    }

    // --- Step 1: CAM_ACCOUNT ---

    #[test]
    fn env_beats_marker() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("w", "work\n");
        assert_eq!(
            resolve(Some("oss"), &tree.dir("w"), &config).unwrap(),
            account("oss", Origin::Env)
        );
    }

    #[test]
    fn env_none_forces_system() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("w", "work\n");
        assert_eq!(
            resolve(Some("system"), &tree.dir("w"), &config).unwrap(),
            system(Origin::Env)
        );
    }

    #[test]
    fn env_beats_a_broken_marker() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("w", "Not Valid\n");
        assert_eq!(
            resolve(Some("work"), &tree.dir("w"), &config).unwrap(),
            account("work", Origin::Env)
        );
    }

    #[test]
    fn env_system_does_not_read_config() {
        let tree = Tree::new();
        let config = tree.config(BROKEN_CONFIG);
        assert_eq!(
            resolve(Some("system"), &tree.dir("w"), &config).unwrap(),
            system(Origin::Env)
        );
    }

    #[test]
    fn env_is_trimmed() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        assert_eq!(
            resolve(Some(" work\n"), &tree.dir("w"), &config).unwrap(),
            account("work", Origin::Env)
        );
    }

    #[test]
    fn empty_env_counts_as_unset() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        let marker = tree.marker("w", "work\n");
        for empty in ["", "  "] {
            assert_eq!(
                resolve(Some(empty), &tree.dir("w"), &config).unwrap(),
                account("work", Origin::Marker(marker.clone()))
            );
        }
    }

    #[test]
    fn unknown_env_name_is_an_error() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("w", "work\n");
        assert!(matches!(
            resolve(Some("ghost"), &tree.dir("w"), &config),
            Err(Error::UnknownAccount { name, origin: Origin::Env }) if name == "ghost"
        ));
    }

    #[test]
    fn invalid_env_name_is_an_error() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        assert!(matches!(
            resolve(Some("Work"), &tree.dir("w"), &config),
            Err(Error::InvalidName { name, origin: Origin::Env }) if name == "Work"
        ));
    }

    // --- Step 3: system ---

    #[test]
    fn nothing_set_is_system() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        assert_eq!(
            resolve(None, &tree.dir("a"), &config).unwrap(),
            system(Origin::Nothing)
        );
    }

    #[test]
    fn missing_or_broken_config_is_still_system() {
        let tree = Tree::new();
        assert_eq!(
            resolve(None, &tree.dir("a"), &tree.root.join("missing.toml")).unwrap(),
            system(Origin::Nothing)
        );
        let broken = tree.config(BROKEN_CONFIG);
        assert_eq!(
            resolve(None, &tree.dir("a"), &broken).unwrap(),
            system(Origin::Nothing)
        );
    }

    #[test]
    fn a_none_marker_is_an_ordinary_name() {
        let tree = Tree::new();
        let config = tree.config(CONFIG);
        tree.marker("a", "none\n");
        assert!(matches!(
            resolve(None, &tree.dir("a"), &config),
            Err(Error::UnknownAccount { name, .. }) if name == "none"
        ));
    }

    // --- Display (what `cam which` prints) ---

    #[test]
    fn display() {
        let marker = PathBuf::from("/w/.claude-account");
        let cases = [
            (account("work", Origin::Env), "work (from CAM_ACCOUNT)"),
            (
                account("work", Origin::Marker(marker.clone())),
                "work (from /w/.claude-account)",
            ),
            (system(Origin::Env), "system (from CAM_ACCOUNT)"),
            (
                system(Origin::Marker(marker)),
                "system (from /w/.claude-account)",
            ),
            (
                system(Origin::Nothing),
                "system (no CAM_ACCOUNT or .claude-account)",
            ),
        ];
        for (resolution, expected) in cases {
            assert_eq!(resolution.to_string(), expected);
        }
    }
}
