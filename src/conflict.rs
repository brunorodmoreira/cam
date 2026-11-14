//! What makes Claude Code ignore the token cam passes. Per
//! https://code.claude.com/docs/en/authentication.md#authentication-precedence,
//! these outrank `CLAUDE_CODE_OAUTH_TOKEN` (and `/login`): a cloud-provider
//! switch, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `apiKeyHelper`.
//! A settings file's `env` can also replace the token itself, because Claude
//! Code writes `env` entries over the inherited environment.
//!
//! Only names are ever reported, never values: they can be secrets.

use std::ffi::OsString;

use crate::launch::{self, ENV_TOKEN};

/// The cloud-provider switches and the provider each one selects.
pub const CLOUD: [(&str, &str); 3] = [
    ("CLAUDE_CODE_USE_BEDROCK", "Amazon Bedrock"),
    ("CLAUDE_CODE_USE_VERTEX", "Google Cloud"),
    ("CLAUDE_CODE_USE_FOUNDRY", "Microsoft Foundry"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// `ANTHROPIC_AUTH_TOKEN` or `ANTHROPIC_API_KEY`, set and non-empty.
    Credential(&'static str),
    /// A cloud-provider switch that is on.
    Cloud {
        var: &'static str,
        provider: &'static str,
    },
    /// `apiKeyHelper` in a settings file.
    ApiKeyHelper,
    /// `CLAUDE_CODE_OAUTH_TOKEN` in a settings file's `env`.
    Token,
}

impl Conflict {
    /// The variable or setting, as the user would write it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Credential(var) | Self::Cloud { var, .. } => var,
            Self::ApiKeyHelper => "apiKeyHelper",
            Self::Token => ENV_TOKEN,
        }
    }

    /// What it does to the account cam picks, as a clause.
    pub fn effect(&self) -> String {
        match self {
            Self::Credential(_) => {
                "Claude Code uses it instead of cam's token or your /login".to_owned()
            }
            Self::Cloud { provider, .. } => {
                format!("Claude Code uses {provider} instead of cam's token or your /login")
            }
            Self::ApiKeyHelper => {
                "Claude Code uses its key instead of cam's token or your /login".to_owned()
            }
            Self::Token => "it replaces the token cam passes".to_owned(),
        }
    }
}

/// Whether a switch's value turns it on: `1`, `true`, `yes` or `on`, the
/// values Claude Code's docs give for boolean variables.
pub fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The conflicts in an environment, read through `get`.
pub fn in_env(get: impl Fn(&str) -> Option<OsString>) -> Vec<Conflict> {
    let credentials = launch::outranking(&get)
        .into_iter()
        .map(Conflict::Credential);
    let clouds = CLOUD
        .into_iter()
        .filter(|(var, _)| {
            get(var)
                .and_then(|value| value.into_string().ok())
                .is_some_and(|value| is_truthy(&value))
        })
        .map(|(var, provider)| Conflict::Cloud { var, provider });
    clouds.chain(credentials).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(is_truthy(value), "{value}");
        }
        for value in ["", "0", "false", "no", "off", "2"] {
            assert!(!is_truthy(value), "{value}");
        }
    }

    #[test]
    fn finds_cloud_switches_and_credentials() {
        let found = in_env(env(&[
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CODE_USE_VERTEX", "0"),
            ("ANTHROPIC_API_KEY", "x"),
            ("ANTHROPIC_AUTH_TOKEN", ""),
        ]));
        assert_eq!(
            found,
            [
                Conflict::Cloud {
                    var: "CLAUDE_CODE_USE_BEDROCK",
                    provider: "Amazon Bedrock"
                },
                Conflict::Credential("ANTHROPIC_API_KEY"),
            ]
        );
        assert!(in_env(env(&[])).is_empty());
    }

    #[test]
    fn names_and_effects() {
        assert_eq!(Conflict::ApiKeyHelper.name(), "apiKeyHelper");
        assert_eq!(Conflict::Token.name(), "CLAUDE_CODE_OAUTH_TOKEN");
        assert_eq!(
            Conflict::Credential("ANTHROPIC_API_KEY").effect(),
            "Claude Code uses it instead of cam's token or your /login"
        );
    }
}
