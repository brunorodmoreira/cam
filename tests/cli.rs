//! End-to-end tests for the `cam` binary.
//!
//! Every command runs with a cleared environment: only `CAM_CONFIG`,
//! `CAM_TEST_STORE`, `HOME` and `PATH`, all inside a temp dir. So the real
//! config, markers, Keychain and `claude` are never touched, and no real token
//! can leak in. `cargo test` builds the binary with the `test-store` feature
//! (see Cargo.toml).

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

const CONFIG: &str = "[accounts.work]\nemail = \"you@company.com\"\n\n[accounts.personal]\n";

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
            .env("CAM_TEST_STORE", self.store())
            .env("HOME", self.root.join("home"))
            .env("PATH", self.bin());
        cmd
    }

    /// The sandbox's only `PATH` entry. Empty until a test adds a `claude`.
    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Writes [`FAKE_CLAUDE`] as `<rel>/claude` and returns its path.
    fn fake_claude(&self, rel: &str) -> PathBuf {
        self.script(
            rel,
            &FAKE_CLAUDE
                .replace("{TOKEN}", TOKEN)
                .replace("{TOKEN_2}", TOKEN_2),
        )
    }

    /// Writes an executable `<rel>/claude` with `body` and returns its path.
    fn script(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.dir(rel).join("claude");
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Stores `token` for `name` in the test store, as `cam add` would.
    fn store_token(&self, name: &str, token: &str) {
        fs::create_dir_all(self.store()).unwrap();
        fs::write(self.store().join(name), token).unwrap();
    }

    fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// The default shim dir under the sandbox's `HOME`.
    fn shims(&self) -> PathBuf {
        self.root.join("home/.local/share/cam/shims")
    }

    /// `PATH` made of these sandbox dirs, in order.
    fn path(&self, dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// The token stored for `name` in the test store, if any.
    fn stored(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.store().join(name)).ok()
    }
}

const CAM: &str = env!("CARGO_BIN_EXE_cam");

/// Fake values shaped like tokens. Not real credentials.
const TOKEN: &str = "sk-ant-oat01-FAKE-cli-test-0001";
const TOKEN_2: &str = "sk-ant-oat01-FAKE-cli-test-0002";

/// A stand-in for `claude`. It prints its path, its argv and cam's variables.
/// It names the token it got (`TOKEN`, `TOKEN_2` or `other`) instead of
/// printing it, and exits with `$FAKE_EXIT`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "claude: $0"
printf 'argv:'
for arg in "$@"; do printf ' [%s]' "$arg"; done
echo
if [ -z "${CLAUDE_CODE_OAUTH_TOKEN+set}" ]; then
    echo 'token: unset'
else
    case "$CLAUDE_CODE_OAUTH_TOKEN" in
        '{TOKEN}') echo 'token: TOKEN' ;;
        '{TOKEN_2}') echo 'token: TOKEN_2' ;;
        *) echo 'token: other' ;;
    esac
fi
echo "account: ${CAM_ACTIVE_ACCOUNT-unset}"
echo "depth: ${CAM_SHIM_DEPTH-unset}"
exit "${FAKE_EXIT:-0}"
"#;

/// What [`FAKE_CLAUDE`] prints.
fn launched(claude: &Path, argv: &str, token: &str, account: &str, depth: u32) -> String {
    format!(
        "claude: {}\nargv:{argv}\ntoken: {token}\naccount: {account}\ndepth: {depth}\n",
        claude.display()
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

// --- which ---

#[test]
fn which_pass_through_when_nothing_is_set() {
    let sb = Sandbox::new();
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .success()
        .stdout("none (pass-through: no CAM_ACCOUNT, .claude-account or default)\n");
}

#[test]
fn which_pass_through_without_config_file() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .success()
        .stdout("none (pass-through: no CAM_ACCOUNT, .claude-account or default)\n");
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
fn which_reports_none_marker() {
    let sb = Sandbox::new();
    sb.marker("work", "work\n");
    let none = sb.marker("work/oss", "none # my own login\n");
    sb.cam(&sb.dir("work/oss/repo"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!("none (pass-through, from {})\n", none.display()));
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
fn which_env_none_forces_pass_through_even_with_broken_config() {
    let sb = Sandbox::with_config(Some("not toml at all"));
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("which")
        .env("CAM_ACCOUNT", "none")
        .assert()
        .success()
        .stdout("none (pass-through, from CAM_ACCOUNT)\n");
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
fn which_reports_default() {
    let sb = Sandbox::with_config(Some(&format!("default = \"personal\"\n{CONFIG}")));
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!(
            "personal (from default in {})\n",
            sb.config.display()
        ));
}

#[test]
fn which_unknown_marker_name_fails_with_hint() {
    let sb = Sandbox::with_config(Some(&format!("default = \"personal\"\n{CONFIG}")));
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
    let sb = Sandbox::with_config(Some("defualt = \"work\"\n"));
    sb.cam(&sb.dir("a"))
        .arg("which")
        .assert()
        .failure()
        .stderr(predicate::str::contains(format!(
            "cam: cannot parse {}",
            sb.config.display()
        )))
        .stderr(predicate::str::contains("unknown field `defualt`"));
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
fn use_none_writes_pass_through_marker_without_config() {
    let sb = Sandbox::with_config(None);
    let dir = sb.dir("oss");
    sb.cam(&dir)
        .args(["use", "none"])
        .assert()
        .success()
        .stdout(format!(
            "wrote {} (none)\n",
            dir.join(".claude-account").display()
        ));
    assert_eq!(read(&dir.join(".claude-account")), "none\n");
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
fn add_uses_the_test_store_not_the_keychain() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(format!("{TOKEN}\n"))
        .assert()
        .success()
        .stdout(format!(
            "added work: token saved in the test store at {}\n",
            sb.store().display()
        ));
    assert_eq!(sb.stored("work").as_deref(), Some(TOKEN));
}

#[test]
fn add_records_account_and_email() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work", "--email", "you@company.com"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    let config = read(&sb.config);
    assert!(
        predicate::str::is_match(
            r#"^\[accounts\.work\]\nemail = "you@company.com"\ntoken_created = \d{4}-\d{2}-\d{2}\n$"#
        )
        .unwrap()
        .eval(&config),
        "{config}"
    );

    // The new account resolves.
    let marker = sb.marker("w", "work\n");
    sb.cam(&sb.dir("w"))
        .arg("which")
        .assert()
        .success()
        .stdout(format!("work (from {})\n", marker.display()));
}

#[test]
fn add_keeps_existing_config_comments() {
    let sb = Sandbox::with_config(Some("# my accounts\n[accounts.personal] # home\n"));
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    let config = read(&sb.config);
    assert!(
        config.starts_with("# my accounts\n[accounts.personal] # home\n"),
        "{config}"
    );
    assert!(config.contains("[accounts.work]"), "{config}");
}

#[test]
fn add_twice_needs_force() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work", "--email", "you@company.com"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(TOKEN_2)
        .assert()
        .failure()
        .code(1)
        .stderr(format!(
            "cam: a token for \"work\" is already stored in the test store at {}\nhint: pass --force to replace it\n",
            sb.store().display()
        ));
    assert_eq!(sb.stored("work").as_deref(), Some(TOKEN));

    sb.cam(&sb.root)
        .args(["add", "work", "--force"])
        .write_stdin(TOKEN_2)
        .assert()
        .success()
        .stdout(predicate::str::starts_with("replaced work: token saved"));
    assert_eq!(sb.stored("work").as_deref(), Some(TOKEN_2));
    assert!(read(&sb.config).contains("email = \"you@company.com\""));
}

#[test]
fn add_rejects_bad_tokens_without_echoing_them() {
    let sb = Sandbox::with_config(None);
    for (input, message) in [
        ("", "no token entered"),
        ("hello there", "doesn't look like a token"),
        (
            "sk-ant-oat01-FAKE-secret-part with-space",
            "doesn't look like a token",
        ),
        (
            "sk-ant-api03-FAKE-secret-part",
            "looks like an Anthropic API key",
        ),
    ] {
        sb.cam(&sb.root)
            .args(["add", "work"])
            .write_stdin(input)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains(message))
            .stderr(predicate::str::contains("secret-part").not())
            .stderr(predicate::str::contains("hello").not());
    }
    assert_eq!(sb.stored("work"), None);
    assert!(!sb.config.exists());
}

#[test]
fn add_rejects_bad_names_and_emails() {
    let sb = Sandbox::with_config(None);
    for (args, message) in [
        (vec!["add", "Work"], "invalid account name \"Work\""),
        (vec!["add", "none"], "\"none\" is reserved"),
        (
            vec!["add", "work", "--email", "nope"],
            "invalid email \"nope\"",
        ),
    ] {
        sb.cam(&sb.root)
            .args(&args)
            .write_stdin(TOKEN)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains(message));
    }
    assert_eq!(sb.stored("work"), None);
}

#[test]
fn add_with_broken_config_stores_nothing() {
    let sb = Sandbox::with_config(Some("not toml"));
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(TOKEN)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot parse"));
    assert_eq!(sb.stored("work"), None);
    assert_eq!(read(&sb.config), "not toml");
}

// --- remove ---

#[test]
fn remove_deletes_token_and_config_entry() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .assert()
        .success()
        .stdout(format!(
            "removed work: config entry and token in the test store at {}\n",
            sb.store().display()
        ))
        .stderr("");
    assert_eq!(sb.stored("work"), None);
    assert!(!read(&sb.config).contains("work"));
}

#[test]
fn remove_default_account_clears_default() {
    let sb = Sandbox::with_config(Some(&format!("default = \"work\"\n{CONFIG}")));
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .assert()
        .success()
        .stdout("removed work: config entry (no token was stored)\n")
        .stderr("note: work was the default, so `default` is now unset\n");
    let config = read(&sb.config);
    assert!(!config.contains("default"), "{config}");
    assert!(config.contains("[accounts.personal]"), "{config}");
}

#[test]
fn remove_unknown_account_fails() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["remove", "ghost"])
        .assert()
        .failure()
        .code(1)
        .stderr("cam: nothing to remove: \"ghost\" has no config entry and no stored token\n");
    assert_eq!(read(&sb.config), CONFIG);
}

// --- list ---

#[test]
fn list_without_accounts() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .arg("list")
        .assert()
        .success()
        .stdout("no accounts yet; add one with `cam add <name>`\n");
}

#[test]
fn list_shows_accounts_and_default() {
    let sb = Sandbox::with_config(Some(&format!("default = \"personal\"\n{CONFIG}")));
    sb.cam(&sb.root)
        .args(["add", "oss"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    let output = sb.cam(&sb.root).arg("list").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{stdout}");
    assert!(lines[0].starts_with("ACCOUNT"), "{stdout}");
    assert!(
        predicate::str::is_match(r"^oss +- +\d{4}-\d{2}-\d{2} \(today\)$")
            .unwrap()
            .eval(lines[1]),
        "{stdout}"
    );
    assert!(
        predicate::str::is_match(r"^personal \(default\) +- +unknown$")
            .unwrap()
            .eval(lines[2]),
        "{stdout}"
    );
    assert!(
        predicate::str::is_match(r"^work +you@company\.com +unknown$")
            .unwrap()
            .eval(lines[3]),
        "{stdout}"
    );
}

#[test]
fn list_never_prints_tokens() {
    let sb = Sandbox::with_config(None);
    sb.cam(&sb.root)
        .args(["add", "work"])
        .write_stdin(TOKEN)
        .assert()
        .success();
    sb.cam(&sb.root)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("FAKE").not());
}

// --- exec ---

#[test]
fn exec_with_account_injects_token() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.store_token("work", TOKEN);
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work/repo"))
        .args(["exec", "-p", "hello world", "--model", "x"])
        .assert()
        .success()
        .stdout(launched(
            &claude,
            " [-p] [hello world] [--model] [x]",
            "TOKEN",
            "work",
            1,
        ))
        .stderr("");
}

#[test]
fn exec_follows_cam_account() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.store_token("work", TOKEN);
    sb.store_token("personal", TOKEN_2);
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .env("CAM_ACCOUNT", "personal")
        .assert()
        .success()
        .stdout(launched(&claude, "", "TOKEN_2", "personal", 1));
}

#[test]
fn exec_pass_through_leaves_the_environment_alone() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    // A token the user exported themselves is theirs to keep.
    sb.cam(&sb.dir("a"))
        .args(["exec", "--", "--help"])
        .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN_2)
        .assert()
        .success()
        .stdout(launched(&claude, " [--help]", "TOKEN_2", "unset", 1));
}

#[test]
fn exec_pass_through_drops_a_token_from_a_parent_cam() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.marker("oss", "none\n");
    // As if run from inside a session that cam started for `work`.
    sb.cam(&sb.dir("oss"))
        .arg("exec")
        .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)
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
    sb.store_token("personal", TOKEN_2);
    sb.marker("home", "personal\n");
    sb.cam(&sb.dir("home"))
        .arg("exec")
        .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CAM_SHIM_DEPTH", "1")
        .assert()
        .success()
        .stdout(launched(&claude, "", "TOKEN_2", "personal", 2));
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
fn exec_without_a_token_fails_before_running_claude() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .assert()
        .code(1)
        .stdout("")
        .stderr(format!(
            "cam: no token stored for \"work\" in the test store at {}\nhint: add one with `cam add work`\n",
            sb.store().display()
        ));
}

#[test]
fn exec_with_an_invalid_stored_token_fails() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.store_token("work", "junk value");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("the token stored for \"work\""))
        .stderr(predicate::str::contains("cam add work --force"))
        .stderr(predicate::str::contains("junk").not());
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
fn exec_warns_about_variables_that_outrank_the_token() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.store_token("work", TOKEN);
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_ACCOUNT", "work")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .env("ANTHROPIC_AUTH_TOKEN", "")
        .assert()
        .success()
        .stdout(launched(&claude, "", "TOKEN", "work", 1))
        .stderr(
            "warning: ANTHROPIC_API_KEY is set and outranks the token for \"work\"; unset it to use that account\n",
        );
}

#[test]
fn exec_pass_through_does_not_warn() {
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
    sb.store_token("work", TOKEN);
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
            "TOKEN",
            "work",
            1,
        ))
        .stderr("");
}

#[test]
fn shim_pass_through_and_exit_code() {
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

/// Adds `name` with [`TOKEN`] through `cam add`, so it gets today's date.
fn add(sb: &Sandbox, name: &str) {
    sb.cam(&sb.root)
        .args(["add", name])
        .write_stdin(TOKEN)
        .assert()
        .success();
}

#[test]
fn whoami_shows_the_account_email_and_token() {
    let sb = Sandbox::new();
    add(&sb, "work");
    let marker = sb.marker("work", "work\n");
    sb.cam(&sb.dir("work/repo"))
        .arg("whoami")
        .assert()
        .success()
        .stdout(predicate::str::starts_with(format!(
            "work (from {})\n  email:   you@company.com (expected, not verified)\n  token:   stored in the test store at {}, added ",
            marker.display(),
            sb.store().display()
        )))
        .stdout(predicate::str::contains("(today), good until about "))
        .stdout(predicate::str::contains(TOKEN).not())
        .stderr("");
}

#[test]
fn whoami_fails_without_a_token() {
    let sb = Sandbox::new();
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("whoami")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("  email:   you@company.com"))
        .stderr(format!(
            "cam: no token stored for \"work\" in the test store at {}\nhint: add one with `cam add work`\n",
            sb.store().display()
        ));
}

#[test]
fn whoami_pass_through() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("whoami")
        .assert()
        .success()
        .stdout(
            "none (pass-through: no CAM_ACCOUNT, .claude-account or default)\n  \
             login:   Claude Code's own (`claude auth status` shows which)\n",
        )
        .stderr("");
}

#[test]
fn whoami_names_an_exported_token_in_pass_through() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "  login:   the CLAUDE_CODE_OAUTH_TOKEN exported in this shell\n",
        ))
        .stdout(predicate::str::contains(TOKEN).not());
}

#[test]
fn whoami_reports_the_session_and_conflicts() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "none")
        .env("CAM_ACTIVE_ACCOUNT", "work")
        .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)
        .env("ANTHROPIC_API_KEY", TOKEN_2)
        .assert()
        .success()
        .stdout(
            "none (pass-through, from CAM_ACCOUNT)\n  \
             login:   Claude Code's own (`claude auth status` shows which)\n  \
             session: this shell is inside a `claude` that cam started as work\n",
        )
        .stderr(
            "warning: ANTHROPIC_API_KEY is set, so Claude Code uses it instead of cam's token or your /login\n",
        );
}

#[test]
fn whoami_warns_about_an_expired_token() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nemail = \"you@company.com\"\ntoken_created = 2021-03-01\n",
    ));
    sb.store_token("work", TOKEN);
    sb.cam(&sb.root)
        .arg("whoami")
        .env("CAM_ACCOUNT", "work")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "probably expired around 2022-03-01\n",
        ))
        .stderr(
            "warning: the token for \"work\" has probably expired (around 2022-03-01)\n\
             hint: run `claude setup-token` signed in as you@company.com, then `cam add work --force`\n",
        );
}

// --- doctor ---

#[test]
fn doctor_all_good() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    add(&sb, "work");
    add(&sb, "personal");
    let marker = sb.marker("work", "work\n");
    let output = sb
        .cam(&sb.dir("work/repo"))
        .arg("doctor")
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[0],
        format!("ok    claude: {}/claude", sb.bin().display())
    );
    assert_eq!(
        lines[1],
        format!(
            "ok    shim: plain `claude` runs through cam ({}/claude)",
            sb.shims().display()
        )
    );
    assert_eq!(
        lines[2],
        format!(
            "ok    config: {}, 2 accounts, no default",
            sb.config.display()
        )
    );
    assert!(
        lines[3].starts_with("ok    account personal: token stored, added "),
        "{stdout}"
    );
    assert!(
        lines[4].starts_with("ok    account work: token stored, added "),
        "{stdout}"
    );
    assert_eq!(
        lines[5..],
        [
            format!("ok    here: work (from {})", marker.display()).as_str(),
            "ok    environment: nothing outranks cam's tokens",
            "ok    settings: nothing outranks cam's tokens (no settings files found)",
            "",
            "no problems found",
        ]
    );
    assert!(!stdout.contains(TOKEN));
}

#[test]
fn doctor_reports_problems_and_fails() {
    let sb = Sandbox::new();
    // User settings with a helper, and a repo whose local settings switch
    // to Bedrock. The values must never be printed.
    fs::create_dir_all(sb.root.join("home/.claude")).unwrap();
    fs::write(
        sb.root.join("home/.claude/settings.json"),
        format!(
            r#"{{"apiKeyHelper": "echo {TOKEN}", "env": {{"ANTHROPIC_AUTH_TOKEN": "{TOKEN_2}"}}}}"#
        ),
    )
    .unwrap();
    sb.dir("repo/.git");
    fs::create_dir_all(sb.root.join("repo/.claude")).unwrap();
    fs::write(
        sb.root.join("repo/.claude/settings.local.json"),
        r#"{"env": {"CLAUDE_CODE_USE_BEDROCK": "1"}}"#,
    )
    .unwrap();
    let assert = sb
        .cam(&sb.dir("repo/src"))
        .arg("doctor")
        .env("ANTHROPIC_API_KEY", TOKEN)
        .assert()
        .code(1)
        .stderr("cam: 3 problems, 4 warnings\n");
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let home_settings = sb.root.join("home/.claude/settings.json");
    let local_settings = sb.root.join("repo/.claude/settings.local.json");
    for expected in [
        "fail  claude: cannot find the real `claude` on PATH\n      hint: install Claude Code, or set CAM_CLAUDE_BIN to its full path\n".to_owned(),
        "info  shim: not installed\n      hint: use `cam exec`, or run `cam shim install`\n".to_owned(),
        format!("fail  account personal: no token in the test store at {}\n      hint: add one with `cam add personal`\n", sb.store().display()),
        "fail  account work: no token in ".to_owned(),
        "ok    here: none (pass-through: no CAM_ACCOUNT, .claude-account or default)\n".to_owned(),
        "warn  environment: ANTHROPIC_API_KEY is set, so Claude Code uses it instead of cam's token or your /login\n      hint: unset it to use the account cam picks\n".to_owned(),
        format!("warn  settings: {} sets apiKeyHelper, so Claude Code uses its key instead of cam's token or your /login\n", home_settings.display()),
        format!("warn  settings: {} sets ANTHROPIC_AUTH_TOKEN in `env`, so Claude Code uses it instead of cam's token or your /login\n", home_settings.display()),
        format!("warn  settings: {} sets CLAUDE_CODE_USE_BEDROCK in `env`, so Claude Code uses Amazon Bedrock instead of cam's token or your /login\n", local_settings.display()),
    ] {
        assert!(stdout.contains(&expected), "missing {expected:?} in:\n{stdout}");
    }
    assert!(!stdout.contains("sk-ant-"), "{stdout}");
}

#[test]
fn doctor_flags_a_shim_that_comes_too_late() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["shim", "install"])
        .assert()
        .success();
    sb.cam(&sb.root)
        .arg("doctor")
        .env("PATH", sb.path(&[&sb.bin(), &sb.shims()]))
        .assert()
        .code(1)
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
        .stderr("cam: 2 problems\n");
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
