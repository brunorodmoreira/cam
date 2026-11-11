//! Account names and the reserved `none` selector.

use std::fmt;

/// Reserved name: forces pass-through wherever a name is accepted.
pub const NONE: &str = "none";

/// The account name rule, as shown in error messages.
pub const NAME_RULE: &str = "^[a-z0-9][a-z0-9_-]*$";

/// Whether `name` matches [`NAME_RULE`]. `none` matches too; callers decide
/// whether it is allowed in their position.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// What a name resolves to: a configured account, or pass-through (`none`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Account(String),
    PassThrough,
}

impl Selection {
    /// Parses `none` or a valid account name. Does not check that the account exists.
    pub fn parse(name: &str) -> Option<Self> {
        if name == NONE {
            Some(Self::PassThrough)
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
            Self::PassThrough => f.write_str(NONE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(Selection::parse("none"), Some(Selection::PassThrough));
        assert_eq!(
            Selection::parse("work"),
            Some(Selection::Account("work".into()))
        );
        assert_eq!(Selection::parse("None"), None);
        assert_eq!(Selection::parse("Work"), None);
        assert_eq!(Selection::parse(""), None);
    }

    #[test]
    fn display_selection() {
        assert_eq!(Selection::Account("work".into()).to_string(), "work");
        assert_eq!(Selection::PassThrough.to_string(), "none");
    }
}
