//! End-to-end tests for the `cam` binary.
//!
//! Every command runs with a cleared environment: only `CAM_CONFIG`, `HOME`,
//! `PATH` and `FAKE_LOG`, all inside a temp dir. So the real config,
//! markers, accounts and `claude` are never touched. The `claude` cam finds
//! is [`FAKE_CLAUDE`], which keeps a fake login in the config dir it gets.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

const CONFIG: &str = "[accounts.work]\n\n[accounts.personal]\n";

struct Sandbox {
    _tmp: TempDir,
    root: PathBuf,
    config: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        Self::with_config(Some(CONFIG))
    }

    fn with_config(config: Option<&str>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let config_path = root.join("cam-config.toml");
        if let Some(text) = config {
            fs::write(&config_path, text).unwrap();
        }
        fs::create_dir(root.join("home")).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        Self {
            _tmp: tmp,
            root,
            config: config_path,
        }
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn marker(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.dir(rel).join(".claude-account");
        fs::write(&path, content).unwrap();
        path
    }

    fn cam(&self, cwd: &Path) -> Command {
        self.command(Path::new(CAM), cwd)
    }

    /// `program` (cam, or a shim pointing at it) in the sandbox environment.
    fn command(&self, program: &Path, cwd: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(cwd)
            .env_clear()
            .env("CAM_CONFIG", &self.config)
            .env("HOME", self.home())
            .env("PATH", self.bin())
            .env("FAKE_LOG", self.root.join("claude.log"));
        cmd
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The sandbox's only `PATH` entry. Empty until a test adds a `claude`.
    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Writes [`FAKE_CLAUDE`] as `<rel>/claude` and returns its path.
    fn fake_claude(&self, rel: &str) -> PathBuf {
        self.script(rel, FAKE_CLAUDE)
    }

    /// Writes an executable `<rel>/claude` with `body` and returns its path.
    fn script(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.dir(rel).join("claude");
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The default shim dir under the sandbox's `HOME`.
    fn shims(&self) -> PathBuf {
        self.root.join("home/.local/share/cam/shims")
    }

    /// `PATH` made of these sandbox dirs, in order.
    fn path(&self, dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// Where cam keeps account `name`.
    fn account_dir(&self, name: &str) -> PathBuf {
        self.home().join(".local/share/cam/accounts").join(name)
    }

    /// Creates account `name`'s dir, logged in as `email` the way
    /// [`FAKE_CLAUDE`] records a login, and returns the dir.
    fn logged_in(&self, name: &str, email: &str) -> PathBuf {
        let dir = self.account_dir(name);
        fs::create_dir_all(&dir).unwrap();
        fake_login(&dir.join(".claude.json"), email);
        dir
    }

    /// Logs Claude Code's own setup (`~/.claude.json`) in as `email`.
    fn system_login(&self, email: &str) {
        fake_login(&self.home().join(".claude.json"), email);
    }

    /// What [`FAKE_CLAUDE`] was asked to do, one line per `auth` call.
    fn log(&self) -> String {
        fs::read_to_string(self.root.join("claude.log")).unwrap_or_default()
    }
}

/// Records a login as [`FAKE_CLAUDE`] does: the email in `.claude.json`,
/// where `cam list` reads it, and in a file next to it for `auth status`.
fn fake_login(json: &Path, email: &str) {
    fs::write(
        json,
        format!("{{\"oauthAccount\": {{\"emailAddress\": \"{email}\"}}}}\n"),
    )
    .unwrap();
    let mut flag = json.as_os_str().to_owned();
    flag.push(".email");
    fs::write(flag, format!("{email}\n")).unwrap();
}

const CAM: &str = env!("CARGO_BIN_EXE_cam");

/// A stand-in for `claude`. `auth login`, `auth status` and `auth logout`
/// act on a fake login in `$CLAUDE_CONFIG_DIR`, or in `$HOME` for Claude
/// Code's own setup, and log the call to `$FAKE_LOG`. Anything else prints
/// its path, its argv and cam's variables, and exits with `$FAKE_EXIT`.
/// Shell builtins only, since the sandbox `PATH` has nothing else.
///
/// - `FAKE_LOGIN=fail` makes the login fail; `nothing` makes it succeed
///   without logging in.
/// - `FAKE_EMAIL` is the email a login gets (default: `<account>@example.com`).
/// - `FAKE_EXPIRES` is the `loginExpiresAt` that `auth status` reports.
/// - `FAKE_LOGOUT=fail` makes the logout fail.
/// - With `ANTHROPIC_API_KEY` set, `auth status` reports the key instead.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = auth ] && [ -n "${FAKE_LOG-}" ]; then
    echo "$* (${CLAUDE_CONFIG_DIR-system})" >> "$FAKE_LOG"
fi
if [ -n "${CLAUDE_CONFIG_DIR-}" ]; then
    json="$CLAUDE_CONFIG_DIR/.claude.json"
else
    json="$HOME/.claude.json"
fi
case "$1 $2" in
'auth login')
    case "${FAKE_LOGIN-}" in
        fail) echo 'Login cancelled.' >&2; exit 1 ;;
        nothing) exit 0 ;;
    esac
    email="${FAKE_EMAIL:-${CAM_ACTIVE_ACCOUNT:-system}@example.com}"
    printf '{"oauthAccount": {"emailAddress": "%s"}}\n' "$email" > "$json"
    printf '%s\n' "$email" > "$json.email"
    echo 'Login successful.'
    exit 0 ;;
'auth status')
    if [ -n "${ANTHROPIC_API_KEY-}" ]; then
        echo '{"loggedIn": true, "authMethod": "api_key", "apiKeySource": "ANTHROPIC_API_KEY"}'
        exit 0
    fi
    if [ -s "$json.email" ]; then
        read -r email < "$json.email"
        printf '{"loggedIn": true, "authMethod": "claude.ai", "email": "%s", "orgName": "Acme", "subscriptionType": "team", "loginExpiresAt": "%s"}\n' \
            "$email" "${FAKE_EXPIRES:-2099-01-01T00:00:00.000Z}"
        exit 0
    fi
    echo '{"loggedIn": false, "authMethod": "none", "loginExpiresAt": null}'
    exit 1 ;;
'auth logout')
    if [ "${FAKE_LOGOUT-}" = fail ]; then echo 'Logout failed.' >&2; exit 1; fi
    : > "$json.email"
    echo 'Successfully logged out from your Anthropic account.'
    exit 0 ;;
esac
echo "claude: $0"
printf 'argv:'
for arg in "$@"; do printf ' [%s]' "$arg"; done
echo
echo "config: ${CLAUDE_CONFIG_DIR-unset}"
echo "account: ${CAM_ACTIVE_ACCOUNT-unset}"
echo "depth: ${CAM_SHIM_DEPTH-unset}"
exit "${FAKE_EXIT:-0}"
"#;

/// What [`FAKE_CLAUDE`] prints when it's started as a session.
fn launched(claude: &Path, argv: &str, config: &str, account: &str, depth: u32) -> String {
    format!(
        "claude: {}\nargv:{argv}\nconfig: {config}\naccount: {account}\ndepth: {depth}\n",
        claude.display()
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// `text` with every "(in 12345 days)" written as "(in N days)", since the
/// days left until a fixed expiry change with the date.
fn any_days(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("(in ") {
        out.push_str(&rest[..at + 4]);
        rest = &rest[at + 4..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with(" days)") {
            out.push('N');
            rest = &rest[digits..];
        }
    }
    out.push_str(rest);
    out
}

fn stdout_of(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

// --- which ---

#[test]
fn which_system_when_nothing_is_set() {
    let sb = Sandbox::new();
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .success()
        .stdout("system (no CAM_ACCOUNT or .claude-account)\n");
}

#[test]
fn which_system_without_config_file() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .success()
        .stdout("system (no CAM_ACCOUNT or .claude-account)\n");
}

#[test]
fn which_reports_nearest_marker() {
    let sb = Sandbox::new();
    sb.marker("", "personal\n");
    let work = sb.marker("work", "work\n");
    sb.cam(&sb.dir("work/repo/src"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!("work (from {})\n", work.display()));
}

#[test]
fn which_reports_system_marker() {
    let sb = Sandbox::new();
    sb.marker("work", "work\n");
    let system = sb.marker("work/oss", "system # my own login\n");
    sb.cam(&sb.dir("work/oss/repo"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!("system (from {})\n", system.display()));
}

#[test]
fn which_treats_an_old_none_marker_as_an_unknown_account() {
    let sb = Sandbox::new();
    sb.marker("oss", "none\n");
    sb.cam(&sb.dir("oss"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown account \"none\""));
}

#[test]
fn which_env_beats_marker() {
    let sb = Sandbox::new();
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("which")
        .env("CAM_ACCOUNT", "personal")
        .assert()
        .success()
        .stdout("personal (from CAM_ACCOUNT)\n");
}

#[test]
fn which_env_system_works_even_with_broken_config() {
    let sb = Sandbox::with_config(Some("not toml at all"));
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("which")
        .env("CAM_ACCOUNT", "system")
        .assert()
        .success()
        .stdout("system (from CAM_ACCOUNT)\n");
}

#[test]
fn which_empty_env_is_ignored() {
    let sb = Sandbox::new();
    let work = sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("which")
        .env("CAM_ACCOUNT", "")
        .assert()
        .success()
        .stdout(format!("work (from {})\n", work.display()));
}

#[test]
fn which_unknown_marker_name_fails_with_hint() {
    let sb = Sandbox::new();
    let ghost = sb.marker("a", "ghost\n");
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(format!(
            "cam: unknown account \"ghost\" (from {})\nhint: add it with `cam add ghost`\n",
            ghost.display()
        ));
}

#[test]
fn which_unknown_env_name_fails_with_hint() {
    let sb = Sandbox::new();
    sb.cam(&sb.dir("a"))
        .arg("which")
        .env("CAM_ACCOUNT", "ghost")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "unknown account \"ghost\" (from CAM_ACCOUNT)",
        ))
        .stderr(predicate::str::contains("cam add ghost"));
}

#[test]
fn which_invalid_marker_name_fails() {
    let sb = Sandbox::new();
    let marker = sb.marker("a", "Work\n");
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .code(1)
        .stderr(format!(
            "cam: invalid account name \"Work\" in {}: names must match ^[a-z0-9][a-z0-9_-]*$\n",
            marker.display()
        ));
}

#[test]
fn which_empty_marker_fails() {
    let sb = Sandbox::new();
    sb.marker("", "work\n");
    sb.marker("a", "# nothing\n");
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains("names no account"));
}

#[test]
fn which_broken_config_fails() {
    let sb = Sandbox::with_config(Some("acounts = 1\n"));
    sb.marker("a", "work\n");
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains(format!(
            "cam: cannot parse {}",
            sb.config.display()
        )))
        .stderr(predicate::str::contains("unknown field `acounts`"));
}

#[test]
fn which_explains_a_token_mode_config() {
    let sb = Sandbox::with_config(Some(
        "default = \"work\"\n[accounts.work]\ntoken_created = 2026-11-11\n",
    ));
    sb.marker("a", "work\n");
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains("is from cam's token mode"))
        .stderr(predicate::str::contains(
            "security delete-generic-password -s cam",
        ));
}

#[test]
fn which_redacts_token_like_names() {
    // Not a real token: just enough to match the redaction pattern.
    let fake = "sk-ant-oat01-NOTAREALTOKEN";
    let sb = Sandbox::new();
    sb.marker("a", &format!("{fake}\n"));
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sk-ant-[REDACTED]"))
        .stderr(predicate::str::contains("NOTAREALTOKEN").not());

    let fake_lower = "sk-ant-oat01-notarealtoken";
    sb.cam(&sb.dir("b"))
        .arg("which")
        .env("CAM_ACCOUNT", fake_lower)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unknown account \"sk-ant-[REDACTED]\"",
        ))
        .stderr(predicate::str::contains("notarealtoken").not());
}

// --- use ---

#[test]
fn use_writes_marker_in_cwd() {
    let sb = Sandbox::new();
    let dir = sb.dir("work");
    let marker = dir.join(".claude-account");
    sb.cam(&dir)
        .args(["use", "work"])
        .assert()
        .success()
        .stdout(format!("wrote {} (work)\n", marker.display()));
    assert_eq!(read(&marker), "work\n");

    sb.cam(&sb.dir("work/repo"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!("work (from {})\n", marker.display()));
}

#[test]
fn use_with_dir() {
    let sb = Sandbox::new();
    let target = sb.dir("projects/work");
    sb.cam(&sb.dir("elsewhere"))
        .args(["use", "work", "--dir"])
        .arg(&target)
        .assert()
        .success()
        .stdout(format!(
            "wrote {} (work)\n",
            target.join(".claude-account").display()
        ));
    assert_eq!(read(&target.join(".claude-account")), "work\n");
    assert!(!sb.root.join("elsewhere/.claude-account").exists());
}

#[test]
fn use_with_relative_dir() {
    let sb = Sandbox::new();
    let target = sb.dir("projects/work");
    sb.cam(&sb.dir("projects"))
        .args(["use", "work", "--dir", "work"])
        .assert()
        .success()
        .stdout(format!(
            "wrote {} (work)\n",
            target.join(".claude-account").display()
        ));
    assert_eq!(read(&target.join(".claude-account")), "work\n");
}

#[test]
fn use_replaces_existing_marker() {
    let sb = Sandbox::new();
    let marker = sb.marker("a", "# old\npersonal\n");
    sb.cam(&sb.dir("a"))
        .args(["use", "work"])
        .assert()
        .success();
    assert_eq!(read(&marker), "work\n");
}

#[test]
fn use_system_writes_a_marker_without_config() {
    let sb = Sandbox::with_config(None);
    let dir = sb.dir("oss");
    sb.cam(&dir)
        .args(["use", "system"])
        .assert()
        .success()
        .stdout(format!(
            "wrote {} (system)\n",
            dir.join(".claude-account").display()
        ));
    assert_eq!(read(&dir.join(".claude-account")), "system\n");
}

#[test]
fn use_unknown_account_fails_and_writes_nothing() {
    let sb = Sandbox::new();
    let dir = sb.dir("a");
    sb.cam(&dir)
        .args(["use", "ghost"])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr("cam: unknown account \"ghost\"\nhint: add it with `cam add ghost`\n");
    assert!(!dir.join(".claude-account").exists());
}

#[test]
fn use_invalid_name_fails_and_writes_nothing() {
    let sb = Sandbox::new();
    let dir = sb.dir("a");
    for bad in ["Work", "_x", "a b", "work.prod", ""] {
        sb.cam(&dir)
            .args(["use", bad])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains(format!(
                "invalid account name \"{bad}\""
            )));
    }
    assert!(!dir.join(".claude-account").exists());
}

#[test]
fn use_missing_dir_fails() {
    let sb = Sandbox::new();
    sb.cam(&sb.dir("a"))
        .args(["use", "work", "--dir", "does-not-exist"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("cannot open does-not-exist"));
}

#[test]
fn use_dir_that_is_a_file_fails() {
    let sb = Sandbox::new();
    let file = sb.dir("a").join("file.txt");
    fs::write(&file, "x").unwrap();
    sb.cam(&sb.dir("a"))
        .args(["use", "work", "--dir", "file.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("file.txt is not a directory"));
}

// --- unuse ---

#[test]
fn unuse_removes_marker_in_cwd() {
    let sb = Sandbox::new();
    let marker = sb.marker("a", "work\n");
    sb.cam(&sb.dir("a"))
        .arg("unuse")
        .assert()
        .success()
        .stdout(format!("removed {}\n", marker.display()));
    assert!(!marker.exists());
}

#[test]
fn unuse_with_dir() {
    let sb = Sandbox::new();
    let marker = sb.marker("projects/work", "work\n");
    sb.cam(&sb.dir("elsewhere"))
        .args(["unuse", "--dir"])
        .arg(sb.dir("projects/work"))
        .assert()
        .success()
        .stdout(format!("removed {}\n", marker.display()));
    assert!(!marker.exists());
}

#[test]
fn unuse_only_touches_the_target_dir() {
    let sb = Sandbox::new();
    let parent = sb.marker("work", "work\n");
    let repo = sb.dir("work/repo");
    sb.cam(&repo)
        .arg("unuse")
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(format!(
            "cam: no .claude-account in {}\nhint: the nearest one is {}; pass --dir {} to remove it\n",
            repo.display(),
            parent.display(),
            sb.root.join("work").display()
        ));
    assert_eq!(read(&parent), "work\n");
}

#[test]
fn unuse_without_any_marker_fails() {
    let sb = Sandbox::new();
    let dir = sb.dir("a");
    sb.cam(&dir)
        .arg("unuse")
        .assert()
        .failure()
        .code(1)
        .stderr(format!("cam: no .claude-account in {}\n", dir.display()));
}

#[test]
fn unuse_then_parent_marker_applies() {
    let sb = Sandbox::new();
    let parent = sb.marker("work", "work\n");
    sb.marker("work/oss", "none\n");
    let oss = sb.dir("work/oss");
    sb.cam(&oss).arg("unuse").assert().success();
    sb.cam(&oss)
        .arg("which")
        .assert()
        .success()
        .stdout(format!("work (from {})\n", parent.display()));
}

// --- CLI surface ---

#[test]
fn no_arguments_prints_help() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: cam"));
}

#[test]
fn usage_errors_are_redacted() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("sk-ant-oat01-NOTAREALTOKEN")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("NOTAREALTOKEN").not());
}

// --- add ---

#[test]
fn add_logs_in_and_records_the_account() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .assert()
        .success()
        .stdout("Login successful.\nadded work: logged in as work@example.com (Acme, team)\n")
        .stderr("");
    assert_eq!(read(&sb.config), "[accounts.work]\n");
    let dir = sb.account_dir("work");
    let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(
        sb.log(),
        format!(
            "auth login ({dir})\nauth status --json ({dir})\n",
            dir = dir.display()
        )
    );
}

#[test]
fn add_keeps_existing_config_comments() {
    let sb = Sandbox::with_config(Some("# my accounts\n[accounts.personal] # home\n"));
    sb.fake_claude("bin");
    sb.cam(&sb.root).args(["add", "work"]).assert().success();
    assert_eq!(
        read(&sb.config),
        "# my accounts\n[accounts.personal] # home\n\n[accounts.work]\n"
    );
}

#[test]
fn add_refuses_an_account_that_already_has_a_dir() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .assert()
        .code(1)
        .stderr(format!(
            "cam: {} already exists\nhint: to log in again, run `cam login work`; to start over, `cam remove work` first\n",
            sb.account_dir("work").display()
        ));
    assert_eq!(sb.log(), "");
}

#[test]
fn add_sets_up_a_configured_account_that_has_no_dir() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .assert()
        .success()
        .stdout(predicate::str::ends_with(
            "added work: logged in as work@example.com (Acme, team)\n",
        ));
    assert_eq!(read(&sb.config), CONFIG);
}

#[test]
fn add_cleans_up_after_a_failed_login() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .env("FAKE_LOGIN", "fail")
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "Login cancelled.\ncam: work was not added: the login didn't finish (exit status 1)\n",
        );
    assert!(!sb.account_dir("work").exists());
    assert!(!sb.config.exists());
    assert!(sb.log().ends_with(&format!(
        "auth logout ({})\n",
        sb.account_dir("work").display()
    )));
}

#[test]
fn add_cleans_up_when_claude_reports_no_login() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .env("FAKE_LOGIN", "nothing")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "work was not added: Claude Code reports no login",
        ));
    assert!(!sb.account_dir("work").exists());
    assert!(!sb.config.exists());
}

#[test]
fn add_rejects_bad_names_before_doing_anything() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "system"])
        .assert()
        .code(1)
        .stderr("cam: \"system\" is reserved: it's Claude Code's own setup, not an account\n");
    for bad in ["Work", "_x", "a b", ""] {
        sb.cam(&sb.root)
            .args(["add", bad])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("invalid account name"));
    }
    assert!(!sb.home().join(".local").exists());
    assert_eq!(sb.log(), "");
}

#[test]
fn add_with_a_broken_config_creates_nothing() {
    let sb = Sandbox::with_config(Some("not toml"));
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot parse"));
    assert!(!sb.account_dir("work").exists());
    assert_eq!(sb.log(), "");
}

#[test]
fn add_without_claude_creates_nothing() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work"])
        .assert()
        .code(127)
        .stderr(predicate::str::starts_with(
            "cam: cannot find the real `claude` on PATH",
        ));
    assert!(!sb.account_dir("work").exists());
}

#[test]
fn add_warns_when_the_email_is_already_used() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.system_login("me@home.org");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .env("FAKE_EMAIL", "Me@Home.org")
        .assert()
        .success()
        .stderr(
            "warning: work is logged in as Me@Home.org, the same as system; the browser may have been signed in to the wrong claude.ai account\n\
             hint: sign it in to the right one, then run `cam login work`\n",
        );
}

#[test]
fn add_checks_the_login_itself_despite_an_api_key() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .assert()
        .success()
        .stdout(predicate::str::contains("added work: logged in as work@example.com"))
        .stderr(
            "warning: ANTHROPIC_API_KEY is set and outranks the login of \"work\"; unset it to use that account\n",
        );
}

// --- login ---

#[test]
fn login_logs_the_named_account_in_again() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "old@company.com");
    sb.cam(&sb.root)
        .args(["login", "work"])
        .env("FAKE_EMAIL", "new@company.com")
        .assert()
        .success()
        .stdout("Login successful.\nwork: logged in as new@company.com (Acme, team)\n")
        .stderr("");
    assert!(read(&dir.join(".claude.json")).contains("new@company.com"));
}

#[test]
fn login_defaults_to_the_account_here() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    let marker = sb.marker("work", "work\n");
    sb.cam(&sb.dir("work/repo"))
        .arg("login")
        .assert()
        .success()
        .stdout(format!(
            "work (from {})\nLogin successful.\nwork: logged in as work@example.com (Acme, team)\n",
            marker.display()
        ));
}

#[test]
fn login_system_is_claude_codes_own_login() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["login", "system"])
        // Inside a session for `work`: its config dir must not leak in.
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CLAUDE_CONFIG_DIR", sb.account_dir("work"))
        .assert()
        .success()
        .stdout("Login successful.\nsystem: logged in as system@example.com (Acme, team)\n");
    assert_eq!(
        sb.log(),
        "auth login (system)\nauth status --json (system)\n"
    );
    assert!(read(&sb.home().join(".claude.json")).contains("system@example.com"));
}

#[test]
fn a_failed_login_deletes_nothing() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["login", "work"])
        .env("FAKE_LOGIN", "fail")
        .assert()
        .code(1)
        .stderr("Login cancelled.\ncam: the login didn't finish (exit status 1)\n");
    assert!(read(&dir.join(".claude.json")).contains("you@company.com"));
    assert!(!sb.log().contains("logout"));
}

#[test]
fn login_needs_a_configured_account_with_a_dir() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["login", "ghost"])
        .assert()
        .code(1)
        .stderr("cam: unknown account \"ghost\"\nhint: add it with `cam add ghost`\n");
    sb.cam(&sb.root)
        .args(["login", "work"])
        .assert()
        .code(1)
        .stderr(format!(
            "cam: account \"work\" has no dir at {}\nhint: set it up again with `cam add work`\n",
            sb.account_dir("work").display()
        ));
    assert_eq!(sb.log(), "");
}

// --- remove ---

#[test]
fn remove_logs_out_then_deletes_the_dir_and_entry() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .assert()
        .success()
        .stdout(format!(
            "removed work: logged out, deleted {} and its config entry\n",
            dir.display()
        ));
    assert!(!dir.exists());
    assert_eq!(read(&sb.config), "[accounts.personal]\n");
    assert_eq!(sb.log(), format!("auth logout ({})\n", dir.display()));
}

#[test]
fn remove_keeps_everything_if_the_logout_fails() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .env("FAKE_LOGOUT", "fail")
        .assert()
        .code(1)
        .stderr(
            "cam: cannot log work out, so nothing was removed: `claude auth logout` failed (exit status 1)\n",
        );
    assert!(dir.join(".claude.json").exists());
    assert_eq!(read(&sb.config), CONFIG);
}

#[test]
fn remove_without_claude_keeps_the_dir() {
    let sb = Sandbox::new();
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .assert()
        .code(127)
        .stderr(predicate::str::starts_with("cam: cannot log work out"));
    assert!(dir.exists());
}

#[test]
fn remove_handles_partial_state() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    // A config entry without a dir.
    sb.cam(&sb.root)
        .args(["remove", "personal"])
        .assert()
        .success()
        .stdout("removed personal: its config entry (it had no account dir)\n");
    // A dir without a config entry.
    let dir = sb.logged_in("stray", "x@y.org");
    sb.cam(&sb.root)
        .args(["remove", "stray"])
        .assert()
        .success()
        .stdout(format!(
            "removed stray: logged out and deleted {} (it had no config entry)\n",
            dir.display()
        ));
    assert!(!dir.exists());
    // Neither.
    sb.cam(&sb.root)
        .args(["remove", "stray"])
        .assert()
        .code(1)
        .stderr("cam: nothing to remove: \"stray\" has no config entry and no account dir\n");
}

#[test]
fn remove_never_follows_links_out_of_the_dir() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    let shared = sb.dir("home/.claude/skills");
    fs::write(shared.join("keep.md"), "mine\n").unwrap();
    symlink(&shared, dir.join("skills")).unwrap();
    sb.cam(&sb.root).args(["remove", "work"]).assert().success();
    assert!(!dir.exists());
    assert_eq!(read(&shared.join("keep.md")), "mine\n");
}

#[test]
fn remove_rejects_system_and_bad_names() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["remove", "system"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"system\" is reserved"));
    sb.cam(&sb.root)
        .args(["remove", "Work"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid account name"));
}

// --- list ---

#[test]
fn list_shows_system_and_each_account_without_running_claude() {
    // No `claude` on PATH: `list` reads files only.
    let sb = Sandbox::new();
    sb.system_login("me@home.org");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .arg("list")
        .assert()
        .success()
        .stdout(
            "ACCOUNT   EMAIL\n\
             system    me@home.org\n\
             personal  -\n\
             work      you@company.com\n",
        )
        .stderr("");
}

#[test]
fn list_without_accounts() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .arg("list")
        .assert()
        .success()
        .stdout("ACCOUNT  EMAIL\nsystem   -\n\nno accounts yet; add one with `cam add <name>`\n");
}

#[test]
fn list_with_a_broken_config_fails() {
    let sb = Sandbox::with_config(Some("not toml"));
    sb.cam(&sb.root)
        .arg("list")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot parse"));
}

// --- exec ---

#[test]
fn exec_with_an_account_sets_its_config_dir() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let work = sb.logged_in("work", "you@company.com");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work/repo"))
        .args(["exec", "-p", "hello world", "--model", "x"])
        .assert()
        .success()
        .stdout(launched(
            &claude,
            " [-p] [hello world] [--model] [x]",
            &work.display().to_string(),
            "work",
            1,
        ))
        .stderr("");
}

#[test]
fn exec_follows_cam_account() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    let personal = sb.logged_in("personal", "me@home.org");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .env("CAM_ACCOUNT", "personal")
        .assert()
        .success()
        .stdout(launched(
            &claude,
            "",
            &personal.display().to_string(),
            "personal",
            1,
        ));
}

#[test]
fn exec_system_leaves_the_environment_alone() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    // A CLAUDE_CONFIG_DIR the user exported themselves is theirs to keep.
    sb.cam(&sb.dir("a"))
        .args(["exec", "--", "--help"])
        .env("CLAUDE_CONFIG_DIR", "/somewhere/else")
        .assert()
        .success()
        .stdout(launched(
            &claude,
            " [--help]",
            "/somewhere/else",
            "unset",
            1,
        ));
}

#[test]
fn exec_system_drops_the_config_dir_of_a_parent_cam() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.marker("oss", "system\n");
    // As if run from inside a session that cam started for `work`.
    sb.cam(&sb.dir("oss"))
        .arg("exec")
        .env("CLAUDE_CONFIG_DIR", sb.account_dir("work"))
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CAM_SHIM_DEPTH", "1")
        .assert()
        .success()
        .stdout(launched(&claude, "", "unset", "unset", 2));
}

#[test]
fn exec_nested_session_switches_account() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let personal = sb.logged_in("personal", "me@home.org");
    sb.marker("home-stuff", "personal\n");
    sb.cam(&sb.dir("home-stuff"))
        .arg("exec")
        .env("CLAUDE_CONFIG_DIR", sb.account_dir("work"))
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CAM_SHIM_DEPTH", "1")
        .assert()
        .success()
        .stdout(launched(
            &claude,
            "",
            &personal.display().to_string(),
            "personal",
            2,
        ));
}

#[test]
fn exec_passes_the_exit_code_through() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("exec")
        .env("FAKE_EXIT", "3")
        .assert()
        .code(3);
}

#[test]
fn exec_without_an_account_dir_fails_before_running_claude() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .assert()
        .code(1)
        .stdout("")
        .stderr(format!(
            "cam: account \"work\" has no dir at {}\nhint: set it up again with `cam add work`\n",
            sb.account_dir("work").display()
        ));
}

#[test]
fn exec_does_not_check_the_login() {
    // An expired or missing login is Claude Code's to ask about.
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let work = sb.account_dir("work");
    fs::create_dir_all(&work).unwrap();
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_ACCOUNT", "work")
        .assert()
        .success()
        .stdout(launched(
            &claude,
            "",
            &work.display().to_string(),
            "work",
            1,
        ));
    assert_eq!(sb.log(), "");
}

#[test]
fn exec_unknown_account_fails_before_running_claude() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.marker("x", "nobody\n");
    sb.cam(&sb.dir("x"))
        .arg("exec")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("unknown account \"nobody\""));
}

#[test]
fn exec_warns_about_variables_that_outrank_the_login() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let work = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_ACCOUNT", "work")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .env("ANTHROPIC_AUTH_TOKEN", "")
        .env("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-FAKE")
        .assert()
        .success()
        .stdout(launched(&claude, "", &work.display().to_string(), "work", 1))
        .stderr(
            "warning: ANTHROPIC_API_KEY is set and outranks the login of \"work\"; unset it to use that account\n\
             warning: CLAUDE_CODE_OAUTH_TOKEN is set and outranks the login of \"work\"; unset it to use that account\n",
        );
}

#[test]
fn exec_system_does_not_warn() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("exec")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .assert()
        .success()
        .stderr("");
}

#[test]
fn exec_skips_cam_and_the_shim_dir_on_path() {
    let sb = Sandbox::new();
    let own = sb.dir("own");
    symlink(CAM, own.join("claude")).unwrap();
    let shims = sb.script("home/.local/share/cam/shims", "#!/bin/sh\necho shim\n");
    let shims = shims.parent().unwrap();
    let claude = sb.fake_claude("bin");
    let path = std::env::join_paths([&own, shims, &sb.bin()]).unwrap();
    sb.cam(&sb.root)
        .arg("exec")
        .env("PATH", path)
        .assert()
        .success()
        .stdout(launched(&claude, "", "unset", "unset", 1));
}

#[test]
fn exec_never_runs_a_claude_from_the_current_directory() {
    let sb = Sandbox::new();
    let repo = sb.dir("repo");
    sb.fake_claude("repo");
    sb.cam(&repo)
        .arg("exec")
        .env("PATH", ":.:bin")
        .assert()
        .code(127)
        .stdout("")
        .stderr(predicate::str::contains(
            "cam: cannot find the real `claude` on PATH",
        ));
}

#[test]
fn exec_without_claude_exits_127() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("exec")
        .assert()
        .code(127)
        .stderr(
            "cam: cannot find the real `claude` on PATH\nhint: install Claude Code, or set CAM_CLAUDE_BIN to its full path\n",
        );
}

#[test]
fn exec_claude_bin_overrides_path() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let chosen = sb.fake_claude("elsewhere");
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_CLAUDE_BIN", &chosen)
        .assert()
        .success()
        .stdout(launched(&chosen, "", "unset", "unset", 1));
}

#[test]
fn exec_claude_bin_must_not_be_cam() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_CLAUDE_BIN", CAM)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("which is cam itself"));
}

#[test]
fn exec_claude_bin_must_exist() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_CLAUDE_BIN", sb.root.join("missing"))
        .assert()
        .code(127)
        .stderr(predicate::str::contains("which is not an executable file"));
}

#[test]
fn exec_that_cannot_start_claude_exits_126() {
    let sb = Sandbox::new();
    // An execute bit, so discovery picks it, but only for the group, so the
    // owner's exec fails with "permission denied". (A file the kernel can't
    // run would just be handed to /bin/sh, as a shell does.)
    let claude = sb.fake_claude("bin");
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o070)).unwrap();
    sb.cam(&sb.root)
        .arg("exec")
        .assert()
        .code(126)
        .stderr(predicate::str::starts_with(format!(
            "cam: cannot run {}: ",
            sb.bin().join("claude").display()
        )));
}

#[test]
fn exec_stops_a_loop_back_to_cam() {
    let sb = Sandbox::new();
    // A wrapper that hands everything back to cam, so each launch finds it again.
    sb.script("bin", &format!("#!/bin/sh\nexec '{CAM}' exec -- \"$@\"\n"));
    sb.cam(&sb.root)
        .arg("exec")
        .assert()
        .code(1)
        .stderr(
            "cam: `claude` keeps leading back to cam (CAM_SHIM_DEPTH is 10)\nhint: set CAM_CLAUDE_BIN to the real `claude` binary\n",
        );
}

#[test]
fn exec_help_is_cams() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["exec", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Run the real `claude` with the account that applies here",
        ));
}

// --- shim ---

#[test]
fn shim_install_creates_the_shim_and_prints_the_path_line() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .env("SHELL", "/bin/zsh")
        .assert()
        .success()
        .stdout(format!(
            "installed {shim} -> {CAM}\n\
             \n\
             Next, put the shim dir first on PATH. Add this line to the end of ~/.zshrc, below any line that adds {bin}:\n\
             \n    export PATH=\"$HOME/.local/share/cam/shims:$PATH\"\n\
             \n\
             Then open a new terminal. `cam which` shows the account `claude` will use.\n",
            shim = sb.shims().join("claude").display(),
            bin = sb.bin().display(),
        ))
        .stderr("");
    assert_eq!(
        fs::read_link(sb.shims().join("claude")).unwrap(),
        Path::new(CAM)
    );
}

#[test]
fn shim_runs_claude_with_the_account() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    let work = sb.logged_in("work", "you@company.com");
    sb.marker("work", "work\n");
    // Every argument reaches claude, even ones cam would otherwise parse.
    sb.command(&sb.shims().join("claude"), &sb.dir("work/repo"))
        .args(["--help", "exec", "-p", "hi there"])
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .success()
        .stdout(launched(
            &claude,
            " [--help] [exec] [-p] [hi there]",
            &work.display().to_string(),
            "work",
            1,
        ))
        .stderr("");
}

#[test]
fn shim_system_and_exit_code() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.command(&sb.shims().join("claude"), &sb.root)
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .env("FAKE_EXIT", "4")
        .assert()
        .code(4)
        .stdout(launched(&claude, "", "unset", "unset", 1));
}

#[test]
fn shim_without_a_real_claude_fails_instead_of_looping() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.command(&sb.shims().join("claude"), &sb.root)
        .env("PATH", sb.path(&[&sb.shims()]))
        .assert()
        .code(127)
        .stderr(predicate::str::starts_with(
            "cam: cannot find the real `claude` on PATH",
        ));
}

#[test]
fn shim_install_reports_when_the_shim_is_already_first() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Plain `claude` now runs through cam: the shim dir comes first on PATH.\n",
        ));
}

#[test]
fn shim_install_reports_when_the_shim_comes_too_late() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .env("PATH", sb.path(&[&sb.bin(), &sb.shims()]))
        .env("SHELL", "/usr/bin/fish")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "The shim dir is on PATH, but after {}, so plain `claude` still skips cam.\n",
            claude.display()
        )))
        .stdout(predicate::str::contains(
            "    set -gx PATH \"$HOME/.local/share/cam/shims\" $PATH\n",
        ));
}

#[test]
fn shim_install_is_idempotent() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(format!(
            "already installed: {} -> {CAM}\n",
            sb.shims().join("claude").display()
        )));
}

#[test]
fn shim_install_replaces_an_old_shim() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.shims()).unwrap();
    let old = sb.root.join("old-build/cam");
    symlink(&old, sb.shims().join("claude")).unwrap();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(format!(
            "replaced {} -> {CAM} (it pointed at {})\n",
            sb.shims().join("claude").display(),
            old.display()
        )));
}

#[test]
fn shim_install_points_at_the_path_spelling_of_cam() {
    let sb = Sandbox::new();
    let brew = sb.dir("brew/bin");
    symlink(CAM, brew.join("cam")).unwrap();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .env("PATH", sb.path(&[&brew, &sb.bin()]))
        .assert()
        .success();
    assert_eq!(
        fs::read_link(sb.shims().join("claude")).unwrap(),
        brew.join("cam")
    );
}

#[test]
fn shim_install_never_replaces_a_real_claude() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let before = read(&claude);
    sb.cam(&sb.root)
        .args(["shim", "install", "--dir"])
        .arg(sb.bin())
        .assert()
        .code(1)
        .stdout("")
        .stderr(format!(
            "cam: {} already exists and isn't a cam shim, so cam leaves it alone\nhint: remove it yourself, or pick another --dir\n",
            claude.display()
        ));
    assert_eq!(read(&claude), before);
}

#[test]
fn shim_install_notes_a_missing_claude() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success()
        .stderr(
            "note: no real `claude` found on PATH yet; install Claude Code, or set CAM_CLAUDE_BIN\n",
        );
}

#[test]
fn shim_install_with_relative_dir() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["shim", "install", "--dir", "my-shims"])
        .assert()
        .success();
    assert_eq!(
        fs::read_link(sb.root.join("my-shims/claude")).unwrap(),
        Path::new(CAM)
    );
}

#[test]
fn shim_uninstall_removes_only_a_shim() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.cam(&sb.root)
        .args(["shim", "uninstall"])
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .success()
        .stdout(format!("removed {}\n", sb.shims().join("claude").display()))
        .stderr(format!(
            "note: {} is still on PATH; you can remove that line from your shell's startup file\n",
            sb.shims().display()
        ));
    sb.cam(&sb.root)
        .args(["shim", "uninstall"])
        .assert()
        .code(1)
        .stderr(format!(
            "cam: no shim at {}\n",
            sb.shims().join("claude").display()
        ));

    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "uninstall", "--dir"])
        .arg(sb.bin())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("isn't a cam shim"));
    assert!(claude.is_file());
}

// --- whoami ---

// --- whoami ---

#[test]
fn whoami_shows_who_the_account_is_logged_in_as() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    let marker = sb.marker("work", "work\n");
    let assert = sb
        .cam(&sb.dir("work/repo"))
        .arg("whoami")
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        any_days(&stdout_of(&assert)),
        format!(
            "work (from {})\n  email:   you@company.com\n  org:     Acme (team)\n  login:   expires 2099-01-01 (in N days)\n",
            marker.display()
        )
    );
}

#[test]
fn whoami_system() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.system_login("me@home.org");
    let assert = sb.cam(&sb.dir("a")).arg("whoami").assert().success();
    assert_eq!(
        any_days(&stdout_of(&assert)),
        "system (no CAM_ACCOUNT or .claude-account)\n  email:   me@home.org\n  org:     Acme (team)\n  login:   expires 2099-01-01 (in N days)\n"
    );
}

#[test]
fn whoami_fails_when_not_logged_in() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    fs::create_dir_all(sb.account_dir("work")).unwrap();
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "work")
        .assert()
        .code(1)
        .stdout("work (from CAM_ACCOUNT)\n  login:   not logged in\n")
        .stderr("cam: \"work\" isn't logged in\nhint: run `cam login work`\n");
}

#[test]
fn whoami_fails_when_the_login_has_expired() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "work")
        .env("FAKE_EXPIRES", "2020-01-01T00:00:00.000Z")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("  login:   expired 2020-01-01 ("))
        .stderr(predicate::str::contains("hint: run `cam login work`"));
}

#[test]
fn whoami_shows_what_outranks_the_login() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "work")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE-SECRET")
        .assert()
        .success()
        .stdout(
            "work (from CAM_ACCOUNT)\n  login:   ANTHROPIC_API_KEY, which Claude Code uses instead of a login\n",
        )
        .stderr(
            "warning: ANTHROPIC_API_KEY is set, so Claude Code uses it instead of the login\n",
        );
}

#[test]
fn whoami_reports_the_session() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("personal", "me@home.org");
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "personal")
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CLAUDE_CONFIG_DIR", sb.account_dir("work"))
        .assert()
        .success()
        .stdout(predicate::str::ends_with(
            "  session: this shell is inside a `claude` that cam started as work\n",
        ));
}

#[test]
fn whoami_without_an_account_dir_fails() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "work")
        .assert()
        .code(1)
        .stdout("work (from CAM_ACCOUNT)\n")
        .stderr(predicate::str::contains("has no dir at"));
}

// --- doctor ---

#[test]
fn doctor_all_good() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.system_login("me@home.org");
    sb.logged_in("work", "you@company.com");
    sb.logged_in("personal", "personal@home.org");
    let marker = sb.marker("work", "work\n");
    let assert = sb
        .cam(&sb.dir("work"))
        .arg("doctor")
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        any_days(&stdout_of(&assert)),
        format!(
            "ok    claude: {claude}\n\
             ok    shim: plain `claude` runs through cam ({shim})\n\
             ok    config: {config}, 2 accounts\n\
             ok    system: logged in as me@home.org (Acme, team), login expires 2099-01-01 (in N days)\n\
             ok    account personal: logged in as personal@home.org (Acme, team), login expires 2099-01-01 (in N days)\n\
             ok    account work: logged in as you@company.com (Acme, team), login expires 2099-01-01 (in N days)\n\
             ok    here: work (from {marker})\n\
             ok    environment: nothing outranks the login\n\
             ok    settings: nothing outranks the login (no settings files found)\n\
             \n\
             no problems found\n",
            claude = claude.display(),
            shim = sb.shims().join("claude").display(),
            config = sb.config.display(),
            marker = marker.display(),
        )
    );
}

#[test]
fn doctor_reports_problems_and_fails() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    // `work` has no dir, and `personal` isn't logged in.
    fs::create_dir_all(sb.account_dir("personal")).unwrap();
    let settings = sb.dir("home/.claude").join("settings.json");
    fs::write(
        &settings,
        r#"{"apiKeyHelper": "~/bin/SECRET-helper.sh", "env": {"ANTHROPIC_AUTH_TOKEN": "SECRET-VALUE"}}"#,
    )
    .unwrap();
    let assert = sb
        .cam(&sb.root)
        .arg("doctor")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-SECRET")
        .assert()
        .code(1)
        .stderr("cam: 2 problems, 3 warnings\n");
    let out = stdout_of(&assert);
    for expected in [
        "info  system: Claude Code's own setup isn't logged in\n      hint: run `cam login system` if you use it\n".to_owned(),
        "fail  account personal: not logged in\n      hint: run `cam login personal`\n".to_owned(),
        format!(
            "fail  account work: no dir at {}\n      hint: set it up again with `cam add work`\n",
            sb.account_dir("work").display()
        ),
        "warn  environment: ANTHROPIC_API_KEY is set, so Claude Code uses it instead of the login\n".to_owned(),
        format!(
            "warn  settings: {} sets apiKeyHelper, so Claude Code uses its key instead of the login\n",
            settings.display()
        ),
        format!(
            "warn  settings: {} sets ANTHROPIC_AUTH_TOKEN in `env`, so Claude Code uses it instead of the login\n",
            settings.display()
        ),
    ] {
        assert!(out.contains(&expected), "missing {expected:?} in:\n{out}");
    }
    assert!(!out.contains("SECRET"), "{out}");
}

#[test]
fn doctor_checks_each_login_without_what_outranks_it() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.logged_in("personal", "me@home.org");
    sb.cam(&sb.root)
        .arg("doctor")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "ok    account work: logged in as you@company.com (Acme, team)",
        ));
}

#[test]
fn doctor_warns_about_a_login_about_to_expire_and_the_same_account() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.system_login("me@home.org");
    sb.logged_in("work", "you@company.com");
    sb.logged_in("personal", "ME@home.org");
    let soon = {
        // Tomorrow, in UTC, as cam counts days.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tomorrow = secs / 86_400 + 1;
        // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
        let z = tomorrow as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}T12:00:00.000Z")
    };
    let assert = sb
        .cam(&sb.root)
        .arg("doctor")
        .env("FAKE_EXPIRES", &soon)
        .assert()
        .success();
    let out = stdout_of(&assert);
    assert!(
        out.contains("(in 1 day)\n      hint: run `cam login work` to renew it\n"),
        "{out}"
    );
    assert!(
        out.contains(
            "warn  same account: system and personal are both logged in as me@home.org\n      \
             hint: sign the browser in to the right claude.ai account, then run `cam login personal`\n"
        ),
        "{out}"
    );
}

#[test]
fn doctor_warns_about_an_exported_config_dir() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("doctor")
        .env("CLAUDE_CONFIG_DIR", "/elsewhere")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "warn  environment: CLAUDE_CONFIG_DIR is exported as /elsewhere, so `system` uses that instead of ~/.claude\n",
        ));
}

#[test]
fn doctor_flags_a_shim_that_comes_too_late() {
    let sb = Sandbox::with_config(None);
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.cam(&sb.root)
        .arg("doctor")
        .env("PATH", sb.path(&[&sb.bin(), &sb.shims()]))
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "warn  shim: installed in {}, but plain `claude` runs {} first, without cam\n      \
             hint: run `cam shim install` to see the line to add or move\n",
            sb.shims().display(),
            claude.display()
        )));
}

#[test]
fn doctor_with_a_broken_config() {
    let sb = Sandbox::with_config(Some("not toml"));
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(format!(
            "fail  config: cannot parse {}",
            sb.config.display()
        )))
        .stderr("cam: 1 problem\n");
}

#[test]
fn doctor_without_a_config_file() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "info  config: {} doesn't exist yet\n      hint: add an account with `cam add <name>`\n",
            sb.config.display()
        )))
        .stdout(predicate::str::ends_with("\nno problems found\n"));
}

#[test]
fn doctor_without_claude_still_reports() {
    let sb = Sandbox::new();
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "fail  claude: cannot find the real `claude` on PATH",
        ))
        .stdout(predicate::str::contains(
            "info  account work: login not checked, since `claude` wasn't found",
        ));
}

// --- completions ---

#[test]
fn completions_fill_in_account_names() {
    let sb = Sandbox::new();
    let script = |shell: &str| {
        let output = sb
            .cam(&sb.root)
            .args(["completions", shell])
            .assert()
            .success()
            .stderr("")
            .get_output()
            .stdout
            .clone();
        String::from_utf8(output).unwrap()
    };
    let zsh = script("zsh");
    assert!(zsh.starts_with("#compdef cam\n"), "{zsh}");
    // `remove` gets the names, `use` and `login` the names plus `system`.
    assert!(zsh.contains(":(personal work)'"), "{zsh}");
    assert!(zsh.contains("((personal\\:\"\"\nwork\\:\"\"\n"), "{zsh}");
    assert!(
        zsh.contains(r#"system\:"Claude Code'\''s own setup""#),
        "{zsh}"
    );
    assert!(zsh.contains("personal"), "{zsh}");
    assert!(script("bash").contains("complete -F _cam"));
    let fish = script("fish");
    assert!(fish.contains("complete -c cam"), "{fish}");
    assert!(fish.contains("__fish_cam_using_subcommand"), "{fish}");
    assert!(
        fish.contains("complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'work'\n"),
        "{fish}"
    );
    assert!(
        fish.contains("complete -c cam -n \"__fish_cam_using_subcommand completions\" -f -a 'zsh'"),
        "{fish}"
    );
}

#[test]
fn completions_with_a_broken_config_still_work() {
    let sb = Sandbox::with_config(Some("not toml"));
    sb.cam(&sb.root)
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -F _cam"))
        .stdout(predicate::str::contains("personal").not())
        .stderr("");
}

/// What bash's completion for `cam` offers for the last of `words`.
fn bash_completes(script: &Path, words: &[&str]) -> Vec<String> {
    let output = std::process::Command::new("/bin/bash")
        .arg("-c")
        .arg(
            r#"eval "$(cat "$1")"; shift
               COMP_WORDS=("$@"); COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))
               COMP_LINE="$*"; COMP_POINT=${#COMP_LINE}
               _cam cam "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
               printf '%s\n' "${COMPREPLY[@]}""#,
        )
        .arg("_")
        .arg(script)
        .args(words)
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn bash_completes_account_names() {
    let sb = Sandbox::new();
    let output = sb
        .cam(&sb.root)
        .args(["completions", "bash"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let script = sb.root.join("cam.bash");
    fs::write(&script, output).unwrap();

    assert_eq!(bash_completes(&script, &["cam", "use", "w"]), ["work"]);
    let use_values = bash_completes(&script, &["cam", "use", ""]);
    for name in ["personal", "work", "system"] {
        assert!(
            use_values.iter().any(|value| value == name),
            "{use_values:?}"
        );
    }
    let remove_values = bash_completes(&script, &["cam", "remove", ""]);
    assert!(remove_values.iter().any(|value| value == "personal"));
    assert!(!remove_values.iter().any(|value| value == "system"));
    assert_eq!(bash_completes(&script, &["cam", "login", "s"]), ["system"]);
    // Subcommands come in their help order.
    let subcommands = bash_completes(&script, &["cam", ""]);
    let position = |name: &str| subcommands.iter().position(|value| value == name);
    assert!(position("add") < position("remove"), "{subcommands:?}");
    assert!(position("use") < position("which"), "{subcommands:?}");
}
