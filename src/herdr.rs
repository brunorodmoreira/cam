//! Herdr, the terminal workspace manager for coding agents: the label on a
//! pane's `claude`, and what cam's Herdr plugin asks of Herdr. See
//! DESIGN.md, Herdr.
//!
//! Herdr's API is one JSON request per line, one per connection, on the
//! socket that `HERDR_SOCKET_PATH` names (a named pipe on Windows).

use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::launch::ENV_CONFIG_DIR;
use crate::platform;

pub const ENV_PANE: &str = "HERDR_PANE_ID";
pub const ENV_SOCKET: &str = "HERDR_SOCKET_PATH";
pub const ENV_BIN: &str = "HERDR_BIN_PATH";
/// What Herdr tells an action: the focused pane, the workspace, their
/// folders.
pub const ENV_CONTEXT: &str = "HERDR_PLUGIN_CONTEXT_JSON";
pub const ENV_PLUGIN: &str = "HERDR_PLUGIN_ID";

/// cam's plugin, as `integrations/herdr/herdr-plugin.toml` names it.
pub const PLUGIN_ID: &str = "cam";
/// The plugin's pane that runs `cam herdr accounts`.
const ACCOUNTS_PANE: &str = "accounts";

/// The label helper's job: the pane, and the account to name. cam starts
/// the helper with these rather than with arguments, because its own
/// executable can be the shim, which runs as `claude` whatever it's given.
const ENV_LABEL_PANE: &str = "CAM_HERDR_LABEL_PANE";
const ENV_LABEL_ACCOUNT: &str = "CAM_HERDR_LABEL_ACCOUNT";
/// Set for the second copy, which does the work.
const ENV_LABEL_WORKER: &str = "CAM_HERDR_LABEL_WORKER";

/// Who reports the label, and the agent it belongs to: Herdr clears it
/// when that agent leaves the pane.
const SOURCE: &str = "cam";
const AGENT: &str = "claude";

/// Herdr's Claude Code hook, in `hooks/` of a config dir.
pub const HOOK: &str = "herdr-agent-state.sh";

/// How long a request may take. Herdr answers in about a millisecond.
const TIMEOUT: Duration = Duration::from_secs(1);
/// How often, and how many times, the helper asks whether `claude` is in
/// the pane yet: 20 s.
const POLL: Duration = Duration::from_millis(100);
const WAIT_POLLS: u32 = 200;
/// How often, and how many times, it checks the label stayed: 5 s.
const SETTLE: Duration = Duration::from_millis(250);
const SETTLE_POLLS: u32 = 20;

/// The label for the `claude` of a pane that runs `account`.
pub fn label(account: &str) -> String {
    format!("{AGENT} · {account}")
}

/// A variable that is set and non-empty.
fn env_set(key: &str) -> Option<OsString> {
    env::var_os(key).filter(|value| !value.is_empty())
}

/// A Herdr session, reached through its socket.
pub struct Herdr {
    socket: PathBuf,
}

impl Herdr {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// The session cam runs in, if any: the socket `HERDR_SOCKET_PATH` names.
    pub fn from_env() -> Option<Self> {
        env_set(ENV_SOCKET).map(Self::new)
    }

    /// Sends one request and returns its `result`. Herdr's `error` becomes
    /// an error with its message.
    fn call(&self, method: &str, params: Value) -> io::Result<Value> {
        let mut socket = platform::connect_local(&self.socket, TIMEOUT)?;
        let request = json!({ "id": format!("cam:{method}"), "method": method, "params": params });
        socket.write_all(format!("{request}\n").as_bytes())?;
        socket.flush()?;
        let mut line = String::new();
        BufReader::new(socket).read_line(&mut line)?;
        let mut reply: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if let Some(result) = reply.get_mut("result") {
            return Ok(result.take());
        }
        let message = reply
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("no answer");
        Err(io::Error::other(format!("Herdr: {message}")))
    }

    /// Opens the plugin's accounts pane, in `dir` if given. The manifest
    /// says where it goes: a popup.
    pub fn open_accounts(&self, plugin: &str, dir: Option<&Path>) -> io::Result<()> {
        let mut params = json!({ "plugin_id": plugin, "entrypoint": ACCOUNTS_PANE, "focus": true });
        if let Some(dir) = dir {
            params["cwd"] = json!(dir);
        }
        self.call("plugin.pane.open", params).map(drop)
    }
}

/// What Herdr shows for a pane: the agent it sees there, and that agent's
/// label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    pub agent: Option<String>,
    pub label: Option<String>,
}

impl Seen {
    fn is_claude(&self) -> bool {
        self.agent.as_deref() == Some(AGENT)
    }
}

/// What the label helper needs of Herdr. [`Herdr`] is the real one.
pub trait Panes {
    fn seen(&self, pane: &str) -> io::Result<Seen>;
    fn report(&self, pane: &str, label: &str) -> io::Result<()>;
}

impl Panes for Herdr {
    fn seen(&self, pane: &str) -> io::Result<Seen> {
        let result = self.call("pane.get", json!({ "pane_id": pane }))?;
        let field = |name: &str| {
            result
                .pointer(&format!("/pane/{name}"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        Ok(Seen {
            agent: field("agent"),
            label: field("display_agent"),
        })
    }

    fn report(&self, pane: &str, label: &str) -> io::Result<()> {
        let params = json!({
            "pane_id": pane,
            "source": SOURCE,
            "agent": AGENT,
            "display_agent": label,
        });
        self.call("pane.report_metadata", params).map(drop)
    }
}

/// Labels `pane`'s `claude` once Herdr sees it, then watches for a while
/// and labels it again if the label goes while `claude` stays: right after
/// another `claude` exits in the pane, Herdr can still show that one, and
/// clears the label with it. Gives up when `claude` doesn't show within
/// [`WAIT_POLLS`], or on any error. Returns whether the label is on.
pub fn label_pane(
    panes: &impl Panes,
    pane: &str,
    label: &str,
    mut sleep: impl FnMut(Duration),
) -> bool {
    let mut polls = 0;
    loop {
        match panes.seen(pane) {
            Ok(seen) if seen.is_claude() => break,
            Ok(_) => {}
            Err(_) => return false,
        }
        polls += 1;
        if polls == WAIT_POLLS {
            return false;
        }
        sleep(POLL);
    }
    if panes.report(pane, label).is_err() {
        return false;
    }
    let mut on = true;
    for _ in 0..SETTLE_POLLS {
        sleep(SETTLE);
        match panes.seen(pane) {
            Ok(seen) if seen.is_claude() && seen.label.as_deref() != Some(label) => {
                on = panes.report(pane, label).is_ok();
            }
            Ok(seen) => on = seen.label.as_deref() == Some(label),
            Err(_) => return false,
        }
    }
    on
}

/// Starts the label helper for the `claude` that cam is about to run as
/// `account`, when cam runs in a Herdr pane. The helper starts a copy of
/// itself apart from the terminal and exits, so this returns at once and
/// nothing is left behind under `claude`. Best effort and silent.
pub fn start_label(account: &str) {
    let (Some(pane), Some(_)) = (env_set(ENV_PANE), env_set(ENV_SOCKET)) else {
        return;
    };
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let mut command = Command::new(exe);
    command
        .env(ENV_LABEL_PANE, pane)
        .env(ENV_LABEL_ACCOUNT, account)
        .env_remove(ENV_LABEL_WORKER);
    let _ = run_quietly(&mut command);
}

/// Runs `command` with no input or output, and waits for it.
fn run_quietly(command: &mut Command) -> io::Result<ExitStatus> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

/// The label helper's job, when cam was started as the helper.
pub struct LabelJob {
    pane: String,
    account: String,
    worker: bool,
}

impl LabelJob {
    /// The job [`start_label`] gave this process, if it gave one.
    pub fn from_env() -> Option<Self> {
        let text = |key| env_set(key)?.into_string().ok();
        Some(Self {
            pane: text(ENV_LABEL_PANE)?,
            account: text(ENV_LABEL_ACCOUNT)?,
            worker: env_set(ENV_LABEL_WORKER).is_some(),
        })
    }

    /// The first copy starts the worker, detached, and returns. The worker
    /// labels the pane.
    pub fn run(self) {
        if !self.worker {
            if let Ok(exe) = env::current_exe() {
                let mut command = Command::new(exe);
                command
                    .env(ENV_LABEL_WORKER, "1")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let _ = platform::detach(&mut command).spawn();
            }
            return;
        }
        if let Some(herdr) = Herdr::from_env() {
            label_pane(&herdr, &self.pane, &label(&self.account), thread::sleep);
        }
    }
}

/// The folder an action's context names: the focused pane's, else the
/// workspace's.
pub fn context_dir(context: &str) -> Option<PathBuf> {
    let context: Value = serde_json::from_str(context).ok()?;
    ["focused_pane_cwd", "workspace_cwd"]
        .iter()
        .find_map(|key| {
            context
                .get(key)?
                .as_str()
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
        })
}

/// Whether Herdr's Claude Code hook is in the config dir `dir`.
pub fn has_hook(dir: &Path) -> bool {
    dir.join("hooks").join(HOOK).exists()
}

/// Of the accounts in `dirs` (name, dir), those whose dir lacks Herdr's
/// hook while `~/.claude` has it. Without the hook in `~/.claude`, the user
/// hasn't set Herdr up for Claude Code, so none is listed.
pub fn missing_hook<'a>(home: &Path, dirs: &'a [(String, PathBuf)]) -> Vec<&'a (String, PathBuf)> {
    if !has_hook(&home.join(".claude")) {
        return Vec::new();
    }
    dirs.iter()
        .filter(|(_, dir)| dir.is_dir() && !has_hook(dir))
        .collect()
}

/// Herdr's own command: `HERDR_BIN_PATH`, else `herdr` on `PATH`.
fn herdr_command() -> Command {
    Command::new(env_set(ENV_BIN).unwrap_or_else(|| "herdr".into()))
}

/// Puts Herdr's Claude Code hook in the config dir `dir`, with Herdr's own
/// installer, which honours `CLAUDE_CONFIG_DIR`.
pub fn install_hook(dir: &Path) -> io::Result<ExitStatus> {
    herdr_command()
        .args(["integration", "install", "claude"])
        .env(ENV_CONFIG_DIR, dir)
        .status()
}

/// The command line that [`install_hook`] runs, for a hint.
pub fn install_hook_line(dir: &Path) -> String {
    if cfg!(windows) {
        format!(
            "$env:{ENV_CONFIG_DIR} = '{}'; herdr integration install claude; Remove-Item env:{ENV_CONFIG_DIR}",
            dir.display()
        )
    } else {
        format!(
            "{ENV_CONFIG_DIR}='{}' herdr integration install claude",
            dir.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;

    use super::*;
    use crate::testutil::Tree;

    /// A pane that shows `claude` from the `from`-th question on, and
    /// remembers its label, unless `cleared` drops it once.
    struct Fake {
        from: u32,
        asked: RefCell<u32>,
        label: RefCell<Option<String>>,
        reports: RefCell<u32>,
        /// The question after which the label is dropped once.
        drop_after: Option<u32>,
        fail: bool,
    }

    impl Fake {
        fn new(from: u32) -> Self {
            Self {
                from,
                asked: RefCell::new(0),
                label: RefCell::new(None),
                reports: RefCell::new(0),
                drop_after: None,
                fail: false,
            }
        }
    }

    impl Panes for Fake {
        fn seen(&self, _pane: &str) -> io::Result<Seen> {
            if self.fail {
                return Err(io::Error::other("no Herdr"));
            }
            let mut asked = self.asked.borrow_mut();
            *asked += 1;
            if self.drop_after == Some(*asked) {
                self.label.borrow_mut().take();
            }
            Ok(Seen {
                agent: (*asked >= self.from).then(|| AGENT.to_owned()),
                label: self.label.borrow().clone(),
            })
        }

        fn report(&self, _pane: &str, label: &str) -> io::Result<()> {
            *self.reports.borrow_mut() += 1;
            *self.label.borrow_mut() = Some(label.to_owned());
            Ok(())
        }
    }

    #[test]
    fn labels_claude_once_it_shows() {
        let fake = Fake::new(3);
        assert!(label_pane(&fake, "w1:p1", "claude · work", |_| {}));
        assert_eq!(*fake.reports.borrow(), 1);
        assert_eq!(fake.label.borrow().as_deref(), Some("claude · work"));
        assert_eq!(*fake.asked.borrow(), 3 + SETTLE_POLLS);
    }

    #[test]
    fn labels_again_when_the_label_goes_while_claude_stays() {
        let fake = Fake {
            drop_after: Some(5),
            ..Fake::new(1)
        };
        assert!(label_pane(&fake, "w1:p1", "claude · work", |_| {}));
        assert_eq!(*fake.reports.borrow(), 2);
    }

    #[test]
    fn gives_up_when_claude_never_shows() {
        let fake = Fake::new(u32::MAX);
        let mut slept = Duration::ZERO;
        assert!(!label_pane(&fake, "w1:p1", "claude · work", |d| slept += d));
        assert_eq!(*fake.reports.borrow(), 0);
        assert_eq!(*fake.asked.borrow(), WAIT_POLLS);
        assert!(slept < Duration::from_secs(21), "{slept:?}");
    }

    #[test]
    fn gives_up_without_herdr() {
        let fake = Fake {
            fail: true,
            ..Fake::new(1)
        };
        assert!(!label_pane(&fake, "w1:p1", "claude · work", |_| {}));
        assert_eq!(*fake.reports.borrow(), 0);
    }

    #[test]
    fn the_label_names_the_account() {
        assert_eq!(label("work"), "claude · work");
    }

    #[test]
    fn the_context_names_the_focused_folder_first() {
        let both = r#"{"focused_pane_cwd": "/w/repo", "workspace_cwd": "/w"}"#;
        assert_eq!(context_dir(both), Some(PathBuf::from("/w/repo")));
        let workspace = r#"{"focused_pane_cwd": null, "workspace_cwd": "/w"}"#;
        assert_eq!(context_dir(workspace), Some(PathBuf::from("/w")));
        assert_eq!(context_dir(r#"{"focused_pane_cwd": ""}"#), None);
        assert_eq!(context_dir("not json"), None);
    }

    #[test]
    fn only_accounts_without_the_hook_count_once_claude_has_it() {
        let tree = Tree::new();
        let home = tree.dir("home");
        let with = tree.dir("home/accounts/with/hooks");
        fs::write(with.join(HOOK), "").unwrap();
        let without = tree.dir("home/accounts/without");
        let dirs = vec![
            ("with".to_owned(), tree.root.join("home/accounts/with")),
            ("without".to_owned(), without),
            ("gone".to_owned(), tree.root.join("home/accounts/gone")),
        ];
        assert!(
            missing_hook(&home, &dirs).is_empty(),
            "no hook in ~/.claude"
        );

        let hooks = tree.dir("home/.claude/hooks");
        fs::write(hooks.join(HOOK), "").unwrap();
        let missing: Vec<_> = missing_hook(&home, &dirs)
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(missing, ["without"]);
    }

    #[cfg(unix)]
    #[test]
    fn talks_to_herdr_one_line_each_way() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let socket = dir.path().join("h.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in [
                r#"{"id":"x","result":{"type":"pane_info","pane":{"pane_id":"w1:p1","agent":"claude","display_agent":null}}}"#,
                r#"{"id":"x","result":{"type":"ok"}}"#,
                r#"{"id":"x","error":{"code":"pane_not_found","message":"pane w9:p9 not found"}}"#,
            ] {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                requests.push(serde_json::from_str::<Value>(&line).unwrap());
                (&stream)
                    .write_all(format!("{reply}\n").as_bytes())
                    .unwrap();
            }
            requests
        });

        let herdr = Herdr::new(&socket);
        assert_eq!(
            herdr.seen("w1:p1").unwrap(),
            Seen {
                agent: Some("claude".into()),
                label: None
            }
        );
        herdr.report("w1:p1", "claude · work").unwrap();
        let err = herdr.seen("w9:p9").unwrap_err();
        assert_eq!(err.to_string(), "Herdr: pane w9:p9 not found");

        let requests = server.join().unwrap();
        assert_eq!(requests[0]["method"], "pane.get");
        assert_eq!(requests[0]["params"], json!({ "pane_id": "w1:p1" }));
        assert_eq!(requests[1]["method"], "pane.report_metadata");
        assert_eq!(
            requests[1]["params"],
            json!({
                "pane_id": "w1:p1",
                "source": "cam",
                "agent": "claude",
                "display_agent": "claude · work",
            })
        );
    }
}
