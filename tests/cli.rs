//! End-to-end tests for the `cam` binary.
//!
//! Every command runs with `CAM_CONFIG`, `CAM_TEST_STORE` and `HOME` inside a
//! temp dir and with `CAM_ACCOUNT` removed, so the real config, markers and
//! Keychain are never touched. `cargo test` builds the binary with the
//! `test-store` feature (see Cargo.toml).

use std::fs;
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
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cam"));
        cmd.current_dir(cwd)
            .env_remove("CAM_ACCOUNT")
            .env("CAM_CONFIG", &self.config)
            .env("CAM_TEST_STORE", self.store())
            .env("HOME", self.root.join("home"));
        cmd
    }

    fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// The token stored for `name` in the test store, if any.
    fn stored(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.store().join(name)).ok()
    }
}

/// Fake values shaped like tokens. Not real credentials.
const TOKEN: &str = "sk-ant-oat01-FAKE-cli-test-0001";
const TOKEN_2: &str = "sk-ant-oat01-FAKE-cli-test-0002";

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
