//! The command and environment for the real `claude`.
//!
//! - An account sets `CLAUDE_CONFIG_DIR` to its dir, and `CAM_ACTIVE_ACCOUNT`.
//! - `system` leaves the environment alone, except inside a session a
//!   parent cam started (`CAM_ACTIVE_ACCOUNT` is set): then it drops that
//!   session's `CLAUDE_CONFIG_DIR`, so a `system` folder really gets Claude
//!   Code's own setup.
//! - Both set `CAM_SHIM_DEPTH` one higher than cam received it.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use crate::conflict::CLOUD;
use crate::error::Error;

pub const ENV_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
pub const ENV_ACTIVE: &str = "CAM_ACTIVE_ACCOUNT";
pub const ENV_DEPTH: &str = "CAM_SHIM_DEPTH";

pub const ENV_OAUTH_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
pub const ENV_API_KEY: &str = "ANTHROPIC_API_KEY";

/// Credentials in the environment that Claude Code ranks above `/login`.
pub const OUTRANKING: [&str; 3] = ["ANTHROPIC_AUTH_TOKEN", ENV_API_KEY, ENV_OAUTH_TOKEN];

/// The depth at which cam stops. Nested sessions raise the depth legitimately
/// (claude's Bash tool running `claude`), so only a depth that no real
/// nesting reaches counts as a loop. A loop reaches it in milliseconds.
pub const MAX_DEPTH: u32 = 10;

/// What the child runs as.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    Account { name: &'a str, dir: &'a Path },
    System,
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

/// The command that runs `program` with `args` as `target`.
/// `inherited_active` says whether cam itself got `CAM_ACTIVE_ACCOUNT`.
pub fn command<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    target: Target<'_>,
    inherited_active: bool,
    depth: u32,
) -> Command {
    let mut command = Command::new(program);
    command.args(args).env(ENV_DEPTH, depth.to_string());
    match target {
        Target::Account { name, dir } => {
            command.env(ENV_CONFIG_DIR, dir).env(ENV_ACTIVE, name);
        }
        Target::System if inherited_active => {
            command.env_remove(ENV_CONFIG_DIR).env_remove(ENV_ACTIVE);
        }
        Target::System => {}
    }
    command
}

/// Removes what outranks a login from `command`'s environment: the
/// [`OUTRANKING`] credentials and the cloud-provider switches. Used to ask
/// `claude auth status` about the login itself.
pub fn without_outranking(command: &mut Command) -> &mut Command {
    for var in OUTRANKING.into_iter().chain(CLOUD.map(|(var, _)| var)) {
        command.env_remove(var);
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

    fn work() -> Target<'static> {
        Target::Account {
            name: "work",
            dir: Path::new("/home/me/.local/share/cam/accounts/work"),
        }
    }

    #[test]
    fn account_sets_its_config_dir_and_name() {
        let command = command(Path::new("/bin/claude"), &args(), work(), false, 1);
        assert_eq!(command.get_program(), "/bin/claude");
        assert_eq!(command.get_args().collect::<Vec<_>>(), args());
        assert_eq!(
            envs(&command),
            BTreeMap::from([
                (ENV_ACTIVE.into(), Some("work".into())),
                (
                    ENV_CONFIG_DIR.into(),
                    Some("/home/me/.local/share/cam/accounts/work".into())
                ),
                (ENV_DEPTH.into(), Some("1".into())),
            ])
        );
    }

    #[test]
    fn account_replaces_an_inherited_account() {
        let envs = envs(&command(Path::new("/c"), &[] as &[&str], work(), true, 2));
        assert_eq!(envs[ENV_ACTIVE], Some("work".into()));
        assert_eq!(
            envs[ENV_CONFIG_DIR],
            Some("/home/me/.local/share/cam/accounts/work".into())
        );
        assert_eq!(envs[ENV_DEPTH], Some("2".into()));
    }

    #[test]
    fn system_leaves_the_environment_alone() {
        let command = command(Path::new("/c"), &args(), Target::System, false, 1);
        assert_eq!(
            envs(&command),
            BTreeMap::from([(ENV_DEPTH.into(), Some("1".into()))])
        );
    }

    #[test]
    fn system_drops_the_config_dir_of_a_parent_cam() {
        let command = command(Path::new("/c"), &[] as &[&str], Target::System, true, 3);
        assert_eq!(
            envs(&command),
            BTreeMap::from([
                (ENV_ACTIVE.into(), None),
                (ENV_CONFIG_DIR.into(), None),
                (ENV_DEPTH.into(), Some("3".into())),
            ])
        );
    }

    #[test]
    fn without_outranking_removes_credentials_and_cloud_switches() {
        let mut command = command(Path::new("/c"), &[] as &[&str], work(), false, 1);
        without_outranking(&mut command);
        let envs = envs(&command);
        for var in OUTRANKING.into_iter().chain(CLOUD.map(|(var, _)| var)) {
            assert_eq!(envs[var], None, "{var}");
        }
        assert_eq!(envs[ENV_ACTIVE], Some("work".into()));
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
            ("CLAUDE_CODE_OAUTH_TOKEN", "y"),
            ("OTHER", "z"),
        ]);
        let get = |name: &str| env.get(name).map(OsString::from);
        assert_eq!(
            outranking(get),
            ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
        );
        assert!(outranking(|_| None).is_empty());
    }
}
