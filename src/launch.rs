//! The command and environment for the real `claude`.
//!
//! - An account sets `CLAUDE_CODE_OAUTH_TOKEN` and `CAM_ACTIVE_ACCOUNT`.
//! - Pass-through leaves the environment alone, except that it drops a token
//!   that a parent cam injected (`CAM_ACTIVE_ACCOUNT` is set), so a
//!   pass-through folder inside a cam session still gets the normal login.
//! - Both set `CAM_SHIM_DEPTH` one higher than cam received it.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use crate::error::Error;
use crate::token::Token;

pub const ENV_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
pub const ENV_ACTIVE: &str = "CAM_ACTIVE_ACCOUNT";
pub const ENV_DEPTH: &str = "CAM_SHIM_DEPTH";

/// Variables that Claude Code ranks above `CLAUDE_CODE_OAUTH_TOKEN`.
pub const OUTRANKING: [&str; 2] = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"];

/// The depth at which cam stops. Nested sessions raise the depth legitimately
/// (claude's Bash tool running `claude`), so only a depth that no real
/// nesting reaches counts as a loop. A loop reaches it in milliseconds.
pub const MAX_DEPTH: u32 = 10;

/// What the child runs as.
pub enum Credential {
    Account { name: String, token: Token },
    PassThrough,
}

/// The `CAM_SHIM_DEPTH` for the child: one more than `incoming`. A missing
/// or garbled value counts as 0. Fails once the incoming depth reaches
/// [`MAX_DEPTH`].
pub fn child_depth(incoming: Option<&OsStr>) -> Result<u32, Error> {
    let depth = incoming
        .and_then(OsStr::to_str)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if depth >= MAX_DEPTH {
        return Err(Error::ShimLoop { depth });
    }
    Ok(depth + 1)
}

/// The command that runs `program` with `args` as `credential`.
/// `inherited_active` says whether cam itself got `CAM_ACTIVE_ACCOUNT`.
///
/// Never format the result with `{:?}`: `Command`'s `Debug` shows the
/// environment, token included. The token never goes into argv.
pub fn command(
    program: &Path,
    args: &[OsString],
    credential: &Credential,
    inherited_active: bool,
    depth: u32,
) -> Command {
    let mut command = Command::new(program);
    command.args(args).env(ENV_DEPTH, depth.to_string());
    match credential {
        Credential::Account { name, token } => {
            command.env(ENV_TOKEN, token.expose()).env(ENV_ACTIVE, name);
        }
        Credential::PassThrough if inherited_active => {
            command.env_remove(ENV_TOKEN).env_remove(ENV_ACTIVE);
        }
        Credential::PassThrough => {}
    }
    command
}

/// The [`OUTRANKING`] variables that `get` reports as set and non-empty.
pub fn outranking(get: impl Fn(&str) -> Option<OsString>) -> Vec<&'static str> {
    OUTRANKING
        .into_iter()
        .filter(|name| get(name).is_some_and(|value| !value.is_empty()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::testutil::fake_token;

    /// The variables `command` set (`Some`) or removed (`None`).
    fn envs(command: &Command) -> BTreeMap<String, Option<String>> {
        command
            .get_envs()
            .map(|(key, value)| {
                let text = |s: &OsStr| s.to_str().unwrap().to_owned();
                (text(key), value.map(text))
            })
            .collect()
    }

    fn args() -> Vec<OsString> {
        ["-p", "hello world", "--model", "x"]
            .map(OsString::from)
            .to_vec()
    }

    #[test]
    fn account_sets_token_and_name() {
        let credential = Credential::Account {
            name: "work".into(),
            token: fake_token(1),
        };
        let command = command(Path::new("/bin/claude"), &args(), &credential, false, 1);
        assert_eq!(command.get_program(), "/bin/claude");
        assert_eq!(command.get_args().collect::<Vec<_>>(), args());
        assert_eq!(
            envs(&command),
            BTreeMap::from([
                (ENV_ACTIVE.into(), Some("work".into())),
                (ENV_TOKEN.into(), Some(fake_token(1).expose().into())),
                (ENV_DEPTH.into(), Some("1".into())),
            ])
        );
    }

    #[test]
    fn account_replaces_an_inherited_token() {
        let credential = Credential::Account {
            name: "personal".into(),
            token: fake_token(2),
        };
        let envs = envs(&command(Path::new("/c"), &[], &credential, true, 2));
        assert_eq!(envs[ENV_TOKEN], Some(fake_token(2).expose().into()));
        assert_eq!(envs[ENV_ACTIVE], Some("personal".into()));
        assert_eq!(envs[ENV_DEPTH], Some("2".into()));
    }

    #[test]
    fn pass_through_leaves_the_environment_alone() {
        let command = command(Path::new("/c"), &args(), &Credential::PassThrough, false, 1);
        assert_eq!(
            envs(&command),
            BTreeMap::from([(ENV_DEPTH.into(), Some("1".into()))])
        );
    }

    #[test]
    fn pass_through_drops_a_token_from_a_parent_cam() {
        let command = command(Path::new("/c"), &[], &Credential::PassThrough, true, 3);
        assert_eq!(
            envs(&command),
            BTreeMap::from([
                (ENV_ACTIVE.into(), None),
                (ENV_TOKEN.into(), None),
                (ENV_DEPTH.into(), Some("3".into())),
            ])
        );
    }

    #[test]
    fn depth_counts_up_and_stops_at_the_limit() {
        assert_eq!(child_depth(None).unwrap(), 1);
        assert_eq!(child_depth(Some(OsStr::new("0"))).unwrap(), 1);
        assert_eq!(child_depth(Some(OsStr::new(" 3 "))).unwrap(), 4);
        assert_eq!(child_depth(Some(OsStr::new("junk"))).unwrap(), 1);
        assert_eq!(child_depth(Some(OsStr::new("-2"))).unwrap(), 1);
        assert_eq!(
            child_depth(Some(OsStr::new(&(MAX_DEPTH - 1).to_string()))).unwrap(),
            MAX_DEPTH
        );
        assert!(matches!(
            child_depth(Some(OsStr::new(&MAX_DEPTH.to_string()))),
            Err(Error::ShimLoop { depth: MAX_DEPTH })
        ));
    }

    #[test]
    fn outranking_lists_set_non_empty_variables() {
        let env = BTreeMap::from([
            ("ANTHROPIC_API_KEY", "x"),
            ("ANTHROPIC_AUTH_TOKEN", ""),
            ("OTHER", "y"),
        ]);
        let get = |name: &str| env.get(name).map(OsString::from);
        assert_eq!(outranking(get), ["ANTHROPIC_API_KEY"]);
        assert!(outranking(|_| None).is_empty());
    }
}
