//! Account names, the reserved `system` name, and where account dirs live.

use std::fmt;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// Reserved name: Claude Code's own setup, untouched.
pub const SYSTEM: &str = "system";

/// The account name rule, as shown in error messages.
pub const NAME_RULE: &str = "^[a-z0-9][a-z0-9_-]*$";

/// Where account dirs live, under the home directory.
const ACCOUNTS_DIR: &str = ".local/share/cam/accounts";

/// Whether `name` matches [`NAME_RULE`]. `system` matches too; callers
/// decide whether it is allowed in their position.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// What a name resolves to: a configured account, or Claude Code's own
/// setup (`system`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Account(String),
    System,
}

impl Selection {
    /// Parses `system` or a valid account name. Does not check that the
    /// account exists.
    pub fn parse(name: &str) -> Option<Self> {
        if name == SYSTEM {
            Some(Self::System)
        } else if is_valid_name(name) {
            Some(Self::Account(name.to_owned()))
        } else {
            None
        }
    }
}

impl fmt::Display for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Account(name) => f.write_str(name),
            Self::System => f.write_str(SYSTEM),
        }
    }
}

/// The config dir of account `name`: `<home>/.local/share/cam/accounts/<name>`.
/// Always spelled the same way, because Claude Code keys the login's
/// Keychain slot to the exact path.
pub fn dir(home: &Path, name: &str) -> PathBuf {
    home.join(ACCOUNTS_DIR).join(name)
}

/// Creates an account dir with mode 0700, and its parents as needed.
/// Fails if it already exists.
pub fn create_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    DirBuilder::new().mode(0o700).create(dir)
}

/// Whether anything (a dir, a file or a link) is at `path`.
pub fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::testutil::Tree;

    #[test]
    fn valid_names() {
        for name in [
            "work",
            "a",
            "0",
            "9lives",
            "my-account",
            "my_account",
            "a-",
            "a_",
            "system",
            "none",
        ] {
            assert!(is_valid_name(name), "{name:?} should be valid");
        }
    }

    #[test]
    fn invalid_names() {
        for name in [
            "", "-work", "_work", "Work", "WORK", "wo rk", " work", "work ", "wörk", "a.b", "a/b",
            "a#b", "a\tb",
        ] {
            assert!(!is_valid_name(name), "{name:?} should be invalid");
        }
    }

    #[test]
    fn parse_selection() {
        assert_eq!(Selection::parse("system"), Some(Selection::System));
        assert_eq!(
            Selection::parse("work"),
            Some(Selection::Account("work".into()))
        );
        // Token mode's reserved name is an ordinary one now.
        assert_eq!(
            Selection::parse("none"),
            Some(Selection::Account("none".into()))
        );
        assert_eq!(Selection::parse("System"), None);
        assert_eq!(Selection::parse("Work"), None);
        assert_eq!(Selection::parse(""), None);
    }

    #[test]
    fn display_selection() {
        assert_eq!(Selection::Account("work".into()).to_string(), "work");
        assert_eq!(Selection::System.to_string(), "system");
    }

    #[test]
    fn account_dir_is_under_the_home_dir() {
        assert_eq!(
            dir(Path::new("/home/me"), "work"),
            PathBuf::from("/home/me/.local/share/cam/accounts/work")
        );
    }

    #[test]
    fn create_dir_is_private_and_refuses_an_existing_one() {
        let tree = Tree::new();
        let path = dir(&tree.root, "work");
        create_dir(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(exists(&path));
        assert_eq!(
            create_dir(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
