//! End-to-end tests for the `cam` binary.
//!
//! Every command runs with a cleared environment: only `CAM_CONFIG`, `HOME`
//! (and `USERPROFILE`, its Windows counterpart), `PATH` and `FAKE_LOG`, all
//! inside a temp dir. So the real config, markers, accounts and `claude` are
//! never touched. The `claude` cam finds is the fake in
//! `examples/fake_claude.rs`, which keeps a fake login in the config dir it
//! gets. `cargo test` builds it.
//!
//! Tests of shell code, Unix symlinks and permissions are Unix only.

use std::env::consts::EXE_SUFFIX;
use std::fs;
#[cfg(unix)]
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
        // The physical path, as cam sees it: through /var's symlink on
        // macOS, and past 8.3 names like RUNNER~1 on Windows, which also
        // adds a `\\?\` prefix that cam never prints.
        let root = fs::canonicalize(tmp.path()).unwrap();
        #[cfg(windows)]
        let root = PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
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
        let path = under(&self.root, rel);
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
            .env("USERPROFILE", self.home())
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

    /// Puts the fake `claude` in `rel` (`claude.exe` on Windows) and
    /// returns its path.
    fn fake_claude(&self, rel: &str) -> PathBuf {
        let path = self.dir(rel).join(format!("claude{EXE_SUFFIX}"));
        fs::copy(fake_claude_exe(), &path).unwrap();
        path
    }

    /// Writes an executable `<rel>/claude` with `body` and returns its path.
    #[cfg(unix)]
    fn script(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.dir(rel).join("claude");
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The default shim dir under the sandbox's `HOME`.
    #[cfg(unix)]
    fn shims(&self) -> PathBuf {
        under(&self.home(), ".local/share/cam/shims")
    }

    /// `PATH` made of these sandbox dirs, in order.
    fn path(&self, dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// Where cam keeps account `name`.
    fn account_dir(&self, name: &str) -> PathBuf {
        under(&self.home(), &format!(".local/share/cam/accounts/{name}"))
    }

    /// Creates account `name`'s dir, logged in as `email` the way
    /// the fake `claude` records a login, and returns the dir.
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

    /// What the fake `claude` was asked to do, one line per `auth` call.
    fn log(&self) -> String {
        fs::read_to_string(self.root.join("claude.log")).unwrap_or_default()
    }
}

/// `rel`, written with `/`, joined to `base` one part at a time, as cam
/// builds its paths, so they print the same on Windows.
fn under(base: &Path, rel: &str) -> PathBuf {
    let mut path = base.to_owned();
    path.extend(rel.split('/'));
    path
}

/// The fake `claude` that `cargo test` builds from
/// `examples/fake_claude.rs`, next to cam.
fn fake_claude_exe() -> PathBuf {
    let path = Path::new(CAM)
        .parent()
        .unwrap()
        .join("examples")
        .join(format!("fake_claude{EXE_SUFFIX}"));
    assert!(
        path.is_file(),
        "{} is missing: run the tests with plain `cargo test`, or build it with `cargo build --example fake_claude`",
        path.display()
    );
    path
}

/// Records a login as the fake `claude` does: the email in `.claude.json`,
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

/// What `add work` and `login work` say before the browser opens.
const LOGGING_WORK: &str = "Logging work in with Claude Code. It uses the claude.ai account your browser is signed in to.\n";

/// What the fake `claude` prints when it's started as a session.
fn launched(claude: &Path, argv: &str, config: &str, account: &str, depth: u32) -> String {
    format!(
        "claude: {}\nargv:{argv}\nconfig: {config}\naccount: {account}\ndepth: {depth}\n",
        claude.display()
    )
}

/// Removes a file or a link. Windows removes a link to a folder as a
/// folder.
fn remove_link(path: impl AsRef<Path>) {
    let path = path.as_ref();
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if fs::symlink_metadata(path)
            .unwrap()
            .file_type()
            .is_symlink_dir()
        {
            return fs::remove_dir(path).unwrap();
        }
    }
    fs::remove_file(path).unwrap();
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
fn which_suggests_a_close_account_name() {
    let sb = Sandbox::new();
    let marker = sb.marker("a", "wrok\n");
    sb.cam(&sb.dir("a/b"))
        .arg("which")
        .assert()
        .code(1)
        .stderr(format!(
            "cam: unknown account \"wrok\" (from {})\n\
             hint: did you mean `work`? Fix it with `cam use work --dir {}`, or add \"wrok\" with `cam add wrok`\n",
            marker.display(),
            sb.dir("a").display()
        ));
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
            "cam: invalid account name \"Work\" in {}: names must match ^[a-z0-9][a-z0-9_-]*$\nhint: try `work`\n",
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
    assert!(!under(&sb.root, "elsewhere/.claude-account").exists());
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
        .success()
        .stdout(format!("wrote {} (work, was personal)\n", marker.display()));
    assert_eq!(read(&marker), "work\n");
    sb.cam(&sb.dir("a"))
        .args(["use", "work"])
        .assert()
        .success()
        .stdout(format!("wrote {} (work, as before)\n", marker.display()));
}

#[test]
fn use_expands_a_tilde_in_dir() {
    let sb = Sandbox::new();
    let work = sb.dir("home/Work");
    sb.cam(&sb.root)
        .args(["use", "work", "--dir=~/Work"])
        .assert()
        .success()
        .stdout(format!(
            "wrote {} (work)\n",
            work.join(".claude-account").display()
        ));
    sb.cam(&sb.root)
        .args(["unuse", "--dir=~/Work"])
        .assert()
        .success();
    assert!(!work.join(".claude-account").exists());
}

#[test]
fn use_notes_a_git_repository() {
    let sb = Sandbox::new();
    sb.dir("repo/.git");
    let sub = sb.dir("repo/sub");
    sb.cam(&sub)
        .args(["use", "work"])
        .assert()
        .success()
        .stderr(format!(
            "note: {} is in a git repository; to keep .claude-account out of commits, \
             list it in your global gitignore (~/.config/git/ignore by default)\n",
            sub.display()
        ));
}

#[test]
fn use_suggests_a_close_account_name() {
    let sb = Sandbox::new();
    sb.cam(&sb.root)
        .args(["use", "persona"])
        .assert()
        .code(1)
        .stderr(
            "cam: unknown account \"persona\"\nhint: did you mean `personal`? Or add \"persona\" with `cam add persona`\n",
        );
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
        .stderr(predicate::str::contains("Usage: cam"))
        .stderr(predicate::str::contains(
            "Getting started:\n  cam add work ",
        ));
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
        .env("SHELL", "/bin/zsh")
        .assert()
        .success()
        .stdout(
            "Login successful.\n\
             added work: logged in as work@example.com (Acme, team)\n\
             \n\
             Next:\n  \
               cam use work            use work in this folder and the ones below it (or pass --dir)\n  \
               eval \"$(cam init zsh)\"  at the end of ~/.zshrc, so plain `claude` picks the account\n  \
               cam share work          share your settings, CLAUDE.md, skills or history with it\n\
             \n\
             work starts with an empty Claude Code setup of its own.\n\
             Its first session shows the first-run screens, without your settings, CLAUDE.md, skills or MCP servers.\n",
        )
        .stderr(LOGGING_WORK);
    assert_eq!(read(&sb.config), "[accounts.work]\n");
    let dir = sb.account_dir("work");
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        sb.log(),
        format!(
            "auth login ({dir})\nauth status --json ({dir})\n",
            dir = dir.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn add_skips_the_init_step_once_plain_claude_reaches_cam() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
    let out = stdout_of(
        &sb.cam(&sb.root)
            .args(["add", "work"])
            .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
            .assert()
            .success(),
    );
    assert!(
        out.contains("  cam use work    use work in this folder"),
        "{out}"
    );
    assert!(
        out.contains("  cam share work  share your settings"),
        "{out}"
    );
    assert!(!out.contains("cam init"), "{out}");
}

#[test]
fn add_passes_login_arguments_to_claude() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work", "--", "--email", "you@company.com", "--sso"])
        .assert()
        .success();
    assert!(
        sb.log().starts_with(&format!(
            "auth login --email you@company.com --sso ({})\n",
            sb.account_dir("work").display()
        )),
        "{}",
        sb.log()
    );
}

#[test]
fn add_refuses_a_console_login() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work", "--", "--console"])
        .assert()
        .code(1)
        .stderr(predicate::str::ends_with(
            "cam: work was not added: Claude Code logged in with /login managed key, not a Claude subscription\n\
             hint: log in with claude.ai (Pro, Max, Team or Enterprise). Claude Code keeps a Console login \
             outside the account's dir, so cam can't keep one per account\n",
        ));
    assert!(!sb.account_dir("work").exists());
    assert!(!sb.config.exists());
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
        .stdout(predicate::str::contains(
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
        .stderr(format!(
            "{LOGGING_WORK}Login cancelled.\n\
             cam: work was not added: the login didn't finish (exit status 1)\n\
             hint: run `cam add work` to try again\n"
        ));
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
        .stderr(format!(
            "{LOGGING_WORK}Already in use: me@home.org (system).\n\
             warning: work is logged in as Me@Home.org, the same account as your own login (system): \
             the browser was probably still signed in to it\n\
             hint: if work should be another claude.ai account, switch accounts in the browser, then run `cam login work`\n"
        ));
}

#[test]
fn add_allows_the_same_email_in_another_org() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.system_login("me@home.org");
    sb.cam(&sb.root)
        .args(["add", "work"])
        .env("FAKE_EMAIL", "me@home.org")
        .env("FAKE_ORG", "Other")
        .assert()
        .success()
        .stderr(format!(
            "{LOGGING_WORK}Already in use: me@home.org (system).\n"
        ));
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
        .stderr(format!(
            "{LOGGING_WORK}note: ANTHROPIC_API_KEY is set; if you approve it when Claude Code asks, \
             it's used instead of the login of \"work\" (\"Use custom API key\" in /config)\n"
        ));
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
        .stderr(LOGGING_WORK);
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
fn login_passes_arguments_to_claude() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .args(["login", "--", "--sso"])
        .assert()
        .success();
    assert!(
        sb.log()
            .starts_with(&format!("auth login --sso ({})\n", dir.display())),
        "{}",
        sb.log()
    );
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
        .stderr(format!(
            "{LOGGING_WORK}Login cancelled.\n\
             cam: the login didn't finish (exit status 1)\n\
             hint: run `cam login work` to try again\n"
        ));
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
        .args(["remove", "work", "--yes"])
        .assert()
        .success()
        .stdout(format!(
            "removed work: logged out, deleted {} and its config entry\n",
            dir.display()
        ))
        .stderr(
            "note: a .claude-account that names work now makes `claude` stop with an error; change it with `cam use`\n",
        );
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
        .args(["remove", "work", "-y"])
        .env("FAKE_LOGOUT", "fail")
        .assert()
        .code(1)
        .stderr(
            "cam: cannot log work out, so nothing was removed: `claude auth logout` failed (exit status 1): Logout failed.\n",
        );
    assert!(dir.join(".claude.json").exists());
    assert_eq!(read(&sb.config), CONFIG);
}

#[test]
fn remove_without_claude_keeps_the_dir() {
    let sb = Sandbox::new();
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["remove", "work", "--yes"])
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
        .args(["remove", "stray", "--yes"])
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
        .stderr(
            "cam: nothing to remove: \"stray\" has no config entry and no account dir\n\
             hint: `cam list` shows the accounts\n",
        );
    sb.cam(&sb.root)
        .args(["remove", "wrok"])
        .assert()
        .code(1)
        .stderr(predicate::str::ends_with("hint: did you mean `work`?\n"));
}

#[test]
fn remove_asks_first() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    // The sandbox's stdin isn't a terminal, so cam can't ask.
    sb.cam(&sb.root)
        .args(["remove", "work"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "cam: not removing \"work\" without asking, and there's no terminal to ask on\n\
             hint: pass --yes to remove it\n",
        );
    assert!(dir.join(".claude.json").exists());
    assert_eq!(read(&sb.config), CONFIG);
    assert_eq!(sb.log(), "");
}

#[test]
fn remove_names_the_marker_here_that_now_fails() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    let marker = sb.marker("w", "work\n");
    sb.cam(&sb.dir("w/repo"))
        .args(["remove", "work", "--yes"])
        .assert()
        .success()
        .stderr(format!(
            "note: {} names work, so `claude` stops with an error under it until you change it with `cam use`\n",
            marker.display()
        ));
}

#[cfg(unix)]
#[test]
fn remove_never_follows_links_out_of_the_dir() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    let shared = sb.dir("home/.claude/skills");
    fs::write(shared.join("keep.md"), "mine\n").unwrap();
    symlink(&shared, dir.join("skills")).unwrap();
    sb.cam(&sb.root)
        .args(["remove", "work", "--yes"])
        .assert()
        .success();
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
            "note: ANTHROPIC_API_KEY is set; if you approve it when Claude Code asks, it's used instead of \
             the login of \"work\" (\"Use custom API key\" in /config)\n\
             warning: CLAUDE_CODE_OAUTH_TOKEN is set and outranks the login of \"work\"; unset it to use that account\n",
        );
}

#[test]
fn exec_warns_when_every_account_shares_one_login_slot() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let work = sb.logged_in("work", "you@company.com");
    // Claude Code reacts to it even when it's empty.
    sb.cam(&sb.root)
        .arg("exec")
        .env("CAM_ACCOUNT", "work")
        .env("CLAUDE_SECURESTORAGE_CONFIG_DIR", "")
        .assert()
        .success()
        .stdout(launched(&claude, "", &work.display().to_string(), "work", 1))
        .stderr(
            "warning: CLAUDE_SECURESTORAGE_CONFIG_DIR is set, so every account keeps its login in the same \
             place, and the last one logged in is used; unset it, so \"work\" keeps its own login\n",
        );
}

#[test]
fn exec_in_print_mode_warns_about_an_api_key() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["exec", "-p", "hi"])
        .env("CAM_ACCOUNT", "work")
        .env("ANTHROPIC_API_KEY", "sk-ant-api03-FAKE")
        .assert()
        .success()
        .stderr(
            "warning: ANTHROPIC_API_KEY is set and outranks the login of \"work\"; unset it to use that account\n",
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(windows)]
#[test]
fn exec_that_cannot_start_claude_exits_126() {
    let sb = Sandbox::new();
    // Not a program Windows can start, though its name says it is.
    let claude = sb.bin().join("claude.exe");
    fs::write(&claude, "not a program\n").unwrap();
    sb.cam(&sb.root)
        .arg("exec")
        .assert()
        .code(126)
        .stderr(predicate::str::starts_with(format!(
            "cam: cannot run {}: ",
            claude.display()
        )));
}

#[cfg(windows)]
#[test]
fn exec_stops_a_loop_back_to_cam() {
    let sb = Sandbox::new();
    // Two copies of cam named `claude`: each skips itself and finds the
    // other, so each launch starts another, waiting for it, until the depth
    // guard stops the last one. Its exit code comes back through them all.
    let (a, b) = (sb.dir("a"), sb.dir("b"));
    fs::copy(CAM, a.join("claude.exe")).unwrap();
    fs::copy(CAM, b.join("claude.exe")).unwrap();
    sb.cam(&sb.root)
        .arg("exec")
        .env("PATH", sb.path(&[&a, &b]))
        .assert()
        .code(1)
        .stderr(
            "cam: `claude` keeps leading back to cam (CAM_SHIM_DEPTH is 10)\nhint: set CAM_CLAUDE_BIN to the real `claude` binary\n",
        );
}

#[cfg(unix)]
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

// --- init and plain `claude` ---

/// What `cam init zsh` prints after the `PATH` line: clap's script, run
/// only once `compinit` has defined `compdef`.
#[cfg(unix)]
fn zsh_completions(code: &str) -> &str {
    let at = code
        .find("if (( $+functions[compdef] )); then\n")
        .unwrap_or_else(|| panic!("no completions in:\n{code}"));
    &code[at..]
}

#[cfg(unix)]
#[test]
fn init_makes_the_link_and_puts_its_dir_first() {
    let sb = Sandbox::new();
    let assert = sb
        .cam(&sb.root)
        .args(["init", "zsh"])
        .assert()
        .success()
        .stderr("");
    let code = stdout_of(&assert);
    assert!(
        code.starts_with(&format!(
            "export PATH='{}:{}'\nif (( $+functions[compdef] )); then\n",
            sb.shims().display(),
            sb.bin().display()
        )),
        "{code}"
    );
    let completions = zsh_completions(&code);
    assert!(completions.contains("compdef _cam cam"), "{code}");
    assert!(completions.ends_with("fi\n"), "{code}");
    assert_eq!(
        fs::read_link(sb.shims().join("claude")).unwrap(),
        Path::new(CAM)
    );
}

#[cfg(unix)]
#[test]
fn init_moves_the_dir_to_the_front() {
    let sb = Sandbox::new();
    let assert = sb
        .cam(&sb.root)
        .args(["init", "bash"])
        .env("PATH", sb.path(&[&sb.bin(), &sb.shims(), &sb.bin()]))
        .assert()
        .success();
    assert!(stdout_of(&assert).starts_with(&format!(
        "export PATH='{shims}:{bin}:{bin}'\n",
        shims = sb.shims().display(),
        bin = sb.bin().display()
    )));
}

#[cfg(unix)]
#[test]
fn init_for_fish() {
    let sb = Sandbox::new();
    let assert = sb.cam(&sb.root).args(["init", "fish"]).assert().success();
    let code = stdout_of(&assert);
    assert!(
        code.starts_with(&format!(
            "set -gx PATH '{}' '{}'\n",
            sb.shims().display(),
            sb.bin().display()
        )),
        "{code}"
    );
    assert!(
        code.contains("complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'work'"),
        "{code}"
    );
}

#[cfg(unix)]
#[test]
fn init_in_bash_sends_plain_claude_through_cam() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    let work = sb.logged_in("work", "you@company.com");
    sb.marker("w", "work\n");
    sb.command(Path::new("/bin/bash"), &sb.dir("w/repo"))
        .args([
            "--norc",
            "--noprofile",
            "-c",
            r#"eval "$("$CAM" init bash)" && command -v claude && complete -p cam >/dev/null && claude hi"#,
        ])
        .env("CAM", CAM)
        .assert()
        .success()
        .stdout(format!(
            "{}\n{}",
            sb.shims().join("claude").display(),
            launched(&claude, " [hi]", &work.display().to_string(), "work", 1)
        ))
        .stderr("");
}

#[cfg(unix)]
#[test]
fn init_in_zsh_sends_plain_claude_through_cam() {
    let zsh = Path::new("/bin/zsh");
    if !zsh.exists() {
        return;
    }
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    let script = r#"eval "$("$CAM" init zsh)" && command -v claude"#;
    // Without compinit, the completions wait quietly.
    sb.command(zsh, &sb.root)
        .args(["-f", "-c", script])
        .env("CAM", CAM)
        .assert()
        .success()
        .stdout(format!("{}\n", sb.shims().join("claude").display()))
        .stderr("");
    // With it, they load.
    sb.command(zsh, &sb.root)
        .args([
            "-f",
            "-c",
            r#"autoload -Uz compinit && compinit -u -D && eval "$("$CAM" init zsh)" && print -r -- $_comps[cam]"#,
        ])
        .env("CAM", CAM)
        .assert()
        .success()
        .stdout("_cam\n")
        .stderr("");
}

#[cfg(unix)]
#[test]
fn init_replaces_an_old_link_quietly() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.shims()).unwrap();
    let old = under(&sb.root, "old-build/cam");
    symlink(&old, sb.shims().join("claude")).unwrap();
    sb.cam(&sb.root)
        .args(["init", "zsh"])
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        fs::read_link(sb.shims().join("claude")).unwrap(),
        Path::new(CAM)
    );
    // Run again: nothing to change.
    sb.cam(&sb.root)
        .args(["init", "zsh"])
        .assert()
        .success()
        .stderr("");
}

#[cfg(unix)]
#[test]
fn init_points_at_the_path_spelling_of_cam() {
    let sb = Sandbox::new();
    let brew = sb.dir("brew/bin");
    symlink(CAM, brew.join("cam")).unwrap();
    sb.cam(&sb.root)
        .args(["init", "zsh"])
        .env("PATH", sb.path(&[&brew, &sb.bin()]))
        .assert()
        .success();
    assert_eq!(
        fs::read_link(sb.shims().join("claude")).unwrap(),
        brew.join("cam")
    );
}

#[cfg(unix)]
#[test]
fn init_never_replaces_a_claude_that_isnt_cams() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("home/.local/share/cam/shims");
    let before = fs::read(&claude).unwrap();
    let assert = sb
        .cam(&sb.root)
        .args(["init", "zsh"])
        .assert()
        .success()
        .stderr(format!(
            "cam: warning: {} already exists and isn't cam's link, so cam leaves it alone\n\
             hint: plain `claude` can't go through cam until you move it away\n",
            claude.display()
        ));
    // No PATH line, but the completions still load.
    let code = stdout_of(&assert);
    assert!(!code.contains("PATH"), "{code}");
    assert_eq!(zsh_completions(&code), code);
    assert_eq!(fs::read(&claude).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn plain_claude_runs_with_the_account() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
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

#[cfg(unix)]
#[test]
fn plain_claude_system_and_exit_code() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
    sb.command(&sb.shims().join("claude"), &sb.root)
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .env("FAKE_EXIT", "4")
        .assert()
        .code(4)
        .stdout(launched(&claude, "", "unset", "unset", 1));
}

#[cfg(unix)]
#[test]
fn plain_claude_without_a_real_one_fails_instead_of_looping() {
    let sb = Sandbox::new();
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
    sb.command(&sb.shims().join("claude"), &sb.root)
        .env("PATH", sb.path(&[&sb.shims()]))
        .assert()
        .code(127)
        .stderr(predicate::str::starts_with(
            "cam: cannot find the real `claude` on PATH",
        ));
}

#[cfg(unix)]
#[test]
fn plain_claude_says_how_to_run_without_cam() {
    let sb = Sandbox::new();
    sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
    sb.marker("a", "wrok\n");
    let escape =
        "hint: `CAM_ACCOUNT=system claude` runs Claude Code with its own setup, without cam\n";
    sb.command(&sb.shims().join("claude"), &sb.dir("a"))
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("did you mean `work`?"))
        .stderr(predicate::str::ends_with(escape));
    // `cam exec` and `CAM_ACCOUNT=system` itself don't need it.
    sb.cam(&sb.dir("a"))
        .arg("exec")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("CAM_ACCOUNT=system claude").not());
    sb.command(&sb.shims().join("claude"), &sb.dir("a"))
        .env("PATH", sb.path(&[&sb.shims(), &sb.bin()]))
        .env("CAM_ACCOUNT", "system")
        .assert()
        .success();
}

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

#[cfg(unix)]
#[test]
fn doctor_all_good() {
    let sb = Sandbox::new();
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
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
             ok    path: plain `claude` runs through cam ({shim})\n\
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
             hint: if personal should be another claude.ai account, switch accounts in the browser, then run `cam login personal`\n"
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

#[cfg(unix)]
#[test]
fn doctor_flags_plain_claude_skipping_cam() {
    let sb = Sandbox::with_config(None);
    let claude = sb.fake_claude("bin");
    sb.cam(&sb.root).args(["init", "zsh"]).assert().success();
    sb.cam(&sb.root)
        .arg("doctor")
        .env("PATH", sb.path(&[&sb.bin(), &sb.shims()]))
        .env("SHELL", "/bin/zsh")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "warn  path: plain `claude` runs {}, not cam\n      \
             hint: add `eval \"$(cam init zsh)\"` to the end of ~/.zshrc, then open a new terminal; \
             it must come after any line that adds {}. \
             If you'd rather not use cam for plain `claude`, delete {}\n",
            claude.display(),
            sb.bin().display(),
            sb.shims().join("claude").display()
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
#[cfg(unix)]
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

#[cfg(unix)]
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

// --- sharing ---

impl Sandbox {
    /// `<home>/.claude/<name>`, Claude Code's own setup.
    fn claude_dir(&self, name: &str) -> PathBuf {
        under(&self.home().join(".claude"), name)
    }

    /// Writes `<home>/.claude/<rel>`, with its folders.
    fn claude_file(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.claude_dir(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

/// Fails unless each of `names` in account `dir` is a link to its place in
/// the sandbox's `~/.claude`.
fn assert_linked(sb: &Sandbox, dir: &Path, names: &[&str]) {
    for name in names {
        assert_eq!(
            fs::read_link(dir.join(name)).ok(),
            Some(sb.claude_dir(name)),
            "{name} should be a link to ~/.claude/{name}"
        );
    }
}

/// Fails if any of `names` in `dir` is a link.
fn assert_not_linked(dir: &Path, names: &[&str]) {
    for name in names {
        let link =
            fs::symlink_metadata(dir.join(name)).is_ok_and(|meta| meta.file_type().is_symlink());
        assert!(!link, "{name} should not be a link");
    }
}

const SETTINGS_LINKS: [&str; 3] = ["settings.json", "plugins", "hooks"];
const SKILLS_LINKS: [&str; 4] = ["skills", "agents", "commands", "output-styles"];
const HISTORY_LINKS: [&str; 5] = [
    "projects",
    "history.jsonl",
    "file-history",
    "todos",
    "plans",
];

#[test]
fn add_shares_the_chosen_groups() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    let keep = sb.claude_file("skills/keep.md", "mine\n");
    let out = stdout_of(
        &sb.cam(&sb.root)
            .args(["add", "work", "--share", "settings,skills"])
            .assert()
            .success(),
    );
    assert!(out.contains("added work: logged in as work@example.com (Acme, team)\nwork shares settings and skills with ~/.claude\n"), "{out}");
    assert!(
        out.contains("Apart from settings and skills, work starts with an empty Claude Code setup"),
        "{out}"
    );
    assert!(!out.contains("cam share work"), "{out}");
    assert_eq!(
        read(&sb.config),
        "[accounts.work]\nshare = [\"settings\", \"skills\"]\n"
    );
    let dir = sb.account_dir("work");
    assert_linked(&sb, &dir, &SETTINGS_LINKS);
    assert_linked(&sb, &dir, &SKILLS_LINKS);
    // Missing shared files and folders are made, empty; existing ones kept.
    assert_eq!(read(&sb.claude_dir("settings.json")), "{}\n");
    assert!(sb.claude_dir("plugins").is_dir());
    assert_eq!(read(&keep), "mine\n");
    assert_eq!(read(&under(&dir, "skills/keep.md")), "mine\n");
    // No keybindings.json yet, and the other groups stay out.
    assert!(fs::symlink_metadata(dir.join("keybindings.json")).is_err());
    assert!(fs::symlink_metadata(dir.join("CLAUDE.md")).is_err());
    assert!(fs::symlink_metadata(dir.join("projects")).is_err());
}

#[test]
fn every_share_syntax_records_the_same_groups() {
    for args in [
        &["--share", "skills,settings"][..],
        &["--share", "settings", "--share", "skills"],
        &["--share=settings, skills"],
        &["--share", "skills", "--share", "settings,skills"],
    ] {
        let sb = Sandbox::with_config(None);
        sb.fake_claude("bin");
        sb.cam(&sb.root)
            .args(["add", "work"])
            .args(args)
            .assert()
            .success();
        assert_eq!(
            read(&sb.config),
            "[accounts.work]\nshare = [\"settings\", \"skills\"]\n",
            "{args:?}"
        );
    }
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    sb.cam(&sb.root)
        .args(["add", "work", "--share", "all"])
        .assert()
        .success();
    assert_eq!(
        read(&sb.config),
        "[accounts.work]\nshare = [\"settings\", \"instructions\", \"skills\", \"history\"]\n"
    );
    let dir = sb.account_dir("work");
    assert_linked(&sb, &dir, &["CLAUDE.md", "rules"]);
    assert_linked(&sb, &dir, &HISTORY_LINKS);
    assert_eq!(read(&sb.claude_dir("history.jsonl")), "");
    assert_eq!(read(&sb.claude_dir("CLAUDE.md")), "");
}

#[test]
fn add_shares_nothing_with_none_or_without_a_terminal() {
    for args in [&["--share", "none"][..], &[]] {
        let sb = Sandbox::with_config(None);
        sb.fake_claude("bin");
        sb.cam(&sb.root)
            .args(["add", "work"])
            .args(args)
            .assert()
            .success();
        assert_eq!(read(&sb.config), "[accounts.work]\n", "{args:?}");
        let dir = sb.account_dir("work");
        assert_not_linked(&dir, &SETTINGS_LINKS);
        assert_not_linked(&dir, &HISTORY_LINKS);
        assert!(!sb.home().join(".claude").exists(), "{args:?}");
    }
}

#[test]
fn add_rejects_a_bad_share_before_doing_anything() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    for (share, error) in [
        (
            "settings,setings",
            "cam: unknown group \"setings\"\nhint: did you mean `settings`? The groups are settings, instructions, skills and history, or `all`\n",
        ),
        ("none,skills", "cam: `none` can't go with other groups\n"),
        (
            "",
            "cam: no group named\nhint: the groups are settings, instructions, skills and history, or `all`\n",
        ),
    ] {
        sb.cam(&sb.root)
            .args(["add", "work", "--share", share])
            .assert()
            .code(1)
            .stdout("")
            .stderr(error);
    }
    assert!(!sb.account_dir("work").exists());
    assert!(!sb.config.exists());
    assert_eq!(sb.log(), "");
}

#[test]
fn a_failed_add_keeps_the_shared_files() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    let keep = sb.claude_file("skills/keep.md", "mine\n");
    sb.cam(&sb.root)
        .args(["add", "work", "--share", "skills"])
        .env("FAKE_LOGIN", "fail")
        .assert()
        .code(1);
    assert!(!sb.account_dir("work").exists());
    assert_eq!(read(&keep), "mine\n");
}

#[test]
fn exec_tops_up_links_and_leaves_the_accounts_own_files() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nshare = [\"settings\", \"instructions\"]\n",
    ));
    let claude = sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    fs::write(dir.join("settings.json"), "{\"mine\": true}\n").unwrap();
    sb.marker("work", "work\n");
    sb.cam(&sb.dir("work"))
        .arg("exec")
        .assert()
        .success()
        .stdout(launched(&claude, "", &dir.display().to_string(), "work", 1))
        .stderr(
            "warning: work's settings.json is a file, not a link to ~/.claude/settings.json, so work doesn't share it\n\
             hint: run `cam share work settings` to move it aside and link it\n",
        );
    assert_linked(&sb, &dir, &["plugins", "hooks", "CLAUDE.md", "rules"]);
    assert_eq!(read(&dir.join("settings.json")), "{\"mine\": true}\n");
    // keybindings.json waits until Claude Code's own setup has one.
    assert!(fs::symlink_metadata(dir.join("keybindings.json")).is_err());
    sb.claude_file("keybindings.json", "{\"bindings\": []}\n");
    remove_link(dir.join("rules"));
    sb.cam(&sb.dir("work")).arg("exec").assert().success();
    assert_linked(&sb, &dir, &["keybindings.json", "rules"]);
}

#[test]
fn login_tops_up_links() {
    let sb = Sandbox::with_config(Some("[accounts.work]\nshare = [\"skills\"]\n"));
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "work@example.com");
    sb.cam(&sb.root).args(["login", "work"]).assert().success();
    assert_linked(&sb, &dir, &SKILLS_LINKS);
}

#[test]
fn share_asks_before_moving_the_accounts_own_copies_aside() {
    let sb = Sandbox::new();
    let dir = sb.logged_in("work", "you@company.com");
    fs::write(dir.join("settings.json"), "{\"mine\": true}\n").unwrap();
    fs::create_dir(dir.join("plugins")).unwrap();
    // No terminal and no --yes: nothing changes.
    sb.cam(&sb.root)
        .args(["share", "work", "settings"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "cam: work has its own settings.json and plugins/, which sharing moves aside, and there's no terminal to ask first\n\
             hint: pass --yes to go ahead; nothing is deleted\n",
        );
    assert_eq!(read(&sb.config), CONFIG);
    assert_eq!(read(&dir.join("settings.json")), "{\"mine\": true}\n");

    let assert = sb
        .cam(&sb.root)
        .args(["share", "work", "settings", "--yes"])
        .assert()
        .success()
        .stderr(
            "note: nothing was deleted: copy what you want to keep from these into ~/.claude, then delete them\n",
        );
    let aside = |name: &str| {
        let found: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|entry| entry.starts_with(&format!("{name}.before-share-")))
            .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        dir.join(&found[0])
    };
    let (settings_aside, plugins_aside) = (aside("settings.json"), aside("plugins"));
    assert_eq!(
        stdout_of(&assert),
        format!(
            "work now shares settings with ~/.claude\n\
             moved work's own settings.json aside, to {}\n\
             moved work's own plugins/ aside, to {}\n",
            settings_aside.display(),
            plugins_aside.display()
        )
    );
    assert_eq!(read(&settings_aside), "{\"mine\": true}\n");
    assert!(plugins_aside.is_dir());
    assert_linked(&sb, &dir, &SETTINGS_LINKS);
    assert_eq!(
        read(&sb.config),
        "[accounts.work]\nshare = [\"settings\"]\n\n[accounts.personal]\n"
    );

    sb.cam(&sb.root)
        .args(["share", "work", "settings"])
        .assert()
        .success()
        .stdout("work already shares settings\n")
        .stderr("");
}

#[test]
fn share_links_again_what_claude_code_replaced() {
    let sb = Sandbox::with_config(Some("[accounts.work]\nshare = [\"settings\"]\n"));
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["share", "work", "settings"])
        .assert()
        .success()
        .stdout("work shares settings again: linked settings.json, plugins, hooks\n");
    // A link replaced by a file, as a save that renames over it would.
    remove_link(dir.join("settings.json"));
    fs::write(dir.join("settings.json"), "{}\n").unwrap();
    let out = stdout_of(
        &sb.cam(&sb.root)
            .args(["share", "work", "settings", "--yes"])
            .assert()
            .success(),
    );
    assert!(
        out.starts_with("work shares settings again: linked settings.json\nmoved work's own settings.json aside, to "),
        "{out}"
    );
    assert_linked(&sb, &dir, &SETTINGS_LINKS);
}

#[test]
fn share_adds_to_what_the_account_shares() {
    let sb = Sandbox::with_config(Some("[accounts.work]\nshare = [\"skills\"]\n"));
    let dir = sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["share", "work", "history", "instructions"])
        .assert()
        .success()
        .stdout("work now shares instructions and history with ~/.claude\n");
    assert_eq!(
        read(&sb.config),
        "[accounts.work]\nshare = [\"instructions\", \"skills\", \"history\"]\n"
    );
    assert_linked(&sb, &dir, &HISTORY_LINKS);
    assert_linked(&sb, &dir, &["CLAUDE.md", "rules"]);
}

#[test]
fn share_checks_its_arguments() {
    let sb = Sandbox::new();
    sb.logged_in("work", "you@company.com");
    for (args, error) in [
        (
            &["share", "work"][..],
            "cam: no group named, and there's no terminal to show the choice on\nhint: name them, as in `cam share work settings`, or `all`\n",
        ),
        (
            &["share", "work", "none"],
            "cam: `none` is only for `cam add --share`\nhint: to stop sharing, run `cam unshare <name> <group>…`\n",
        ),
        (
            &["share", "work", "mcp"],
            "cam: unknown group \"mcp\"\nhint: the groups are settings, instructions, skills and history, or `all`\n",
        ),
        (
            &["share", "wrok", "skills"],
            "cam: unknown account \"wrok\"\nhint: did you mean `work`? Or add \"wrok\" with `cam add wrok`\n",
        ),
        (
            &["share", "system", "skills"],
            "cam: \"system\" is reserved: it's Claude Code's own setup, not an account\n",
        ),
    ] {
        sb.cam(&sb.root).args(args).assert().code(1).stderr(error);
    }
    sb.cam(&sb.root)
        .args(["share", "personal", "skills"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("account \"personal\" has no dir"));
    assert_eq!(read(&sb.config), CONFIG);
}

#[test]
fn unshare_removes_only_links() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nshare = [\"settings\", \"skills\"]\n",
    ));
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    let keep = sb.claude_file("skills/keep.md", "mine\n");
    sb.cam(&sb.root).args(["login", "work"]).assert().success();
    remove_link(dir.join("agents"));
    fs::create_dir(dir.join("agents")).unwrap();
    sb.cam(&sb.root)
        .args(["unshare", "work", "skills"])
        .assert()
        .success()
        .stdout(
            "work no longer shares skills: removed the links skills, commands, output-styles. \
             Your files stay in ~/.claude, and work starts over for these.\n\
             skipped work's agents: it's a folder of its own, not a link\n",
        )
        .stderr("");
    assert_eq!(
        read(&sb.config),
        "[accounts.work]\nshare = [\"settings\"]\n"
    );
    assert_not_linked(&dir, &SKILLS_LINKS);
    assert!(dir.join("agents").is_dir());
    assert_eq!(read(&keep), "mine\n");
    assert_linked(&sb, &dir, &SETTINGS_LINKS);
}

#[test]
fn unshare_with_copy_keeps_a_copy_of_each() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nshare = [\"instructions\", \"history\"]\n",
    ));
    sb.fake_claude("bin");
    let dir = sb.logged_in("work", "you@company.com");
    sb.claude_file("CLAUDE.md", "be brief\n");
    sb.claude_file("projects/repo/session.jsonl", "0123456789\n");
    sb.cam(&sb.root).args(["login", "work"]).assert().success();
    sb.cam(&sb.root)
        .args(["unshare", "work", "all", "--copy"])
        .assert()
        .success()
        .stdout(
            "work no longer shares instructions: replaced the links CLAUDE.md, rules with copies of what they pointed to. \
             ~/.claude is unchanged, and work goes its own way from here.\n\
             work no longer shares history: replaced the links projects, history.jsonl, file-history, todos, plans with copies of what they pointed to. \
             ~/.claude is unchanged, and work goes its own way from here.\n",
        )
        .stderr(format!(
            "Copying 11 B of history into {}...\nnote: work didn't share settings\nnote: work didn't share skills\n",
            dir.display()
        ));
    assert_eq!(read(&sb.config), "[accounts.work]\n");
    assert_not_linked(&dir, &["CLAUDE.md", "projects"]);
    assert_eq!(read(&dir.join("CLAUDE.md")), "be brief\n");
    assert_eq!(
        read(&under(&dir, "projects/repo/session.jsonl")),
        "0123456789\n"
    );
    assert_eq!(read(&sb.claude_dir("CLAUDE.md")), "be brief\n");
}

#[test]
fn unshare_what_is_not_shared_fails() {
    let sb = Sandbox::new();
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root)
        .args(["unshare", "work", "settings"])
        .assert()
        .code(1)
        .stderr("cam: work doesn't share settings\nhint: work shares nothing\n");
    sb.cam(&sb.root)
        .args(["unshare", "work"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("<GROUP>"));
}

#[test]
fn remove_keeps_what_the_account_shared() {
    let sb = Sandbox::with_config(None);
    sb.fake_claude("bin");
    let keep = sb.claude_file("projects/repo/session.jsonl", "keep\n");
    sb.cam(&sb.root)
        .args(["add", "work", "--share", "all"])
        .assert()
        .success();
    sb.cam(&sb.root)
        .args(["remove", "work", "--yes"])
        .assert()
        .success();
    assert!(!sb.account_dir("work").exists());
    assert_eq!(read(&keep), "keep\n");
    assert_eq!(read(&sb.claude_dir("settings.json")), "{}\n");
    assert!(sb.claude_dir("skills").is_dir());
}

#[test]
fn list_and_whoami_show_what_an_account_shares() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nshare = [\"skills\", \"settings\"]\n\n[accounts.personal]\n",
    ));
    sb.fake_claude("bin");
    sb.logged_in("work", "you@company.com");
    sb.cam(&sb.root).arg("list").assert().success().stdout(
        "ACCOUNT   EMAIL            SHARES\n\
             system    -                -\n\
             personal  -                -\n\
             work      you@company.com  settings, skills\n",
    );
    let out = stdout_of(
        &sb.cam(&sb.root)
            .arg("whoami")
            .env("CAM_ACCOUNT", "work")
            .assert()
            .success(),
    );
    assert!(
        any_days(&out).ends_with(
            "  login:   expires 2099-01-01 (in N days)\n  shares:  settings and skills\n"
        ),
        "{out}"
    );
}

#[test]
fn doctor_checks_the_links() {
    let sb = Sandbox::with_config(Some(
        "[accounts.work]\nshare = [\"settings\", \"skills\"]\n\n[accounts.personal]\nshare = [\"instructions\"]\n",
    ));
    sb.fake_claude("bin");
    let work = sb.logged_in("work", "you@company.com");
    let personal = sb.logged_in("personal", "me@home.org");
    sb.cam(&sb.root).args(["login", "work"]).assert().success();
    sb.cam(&sb.root)
        .args(["login", "personal"])
        .assert()
        .success();
    remove_link(work.join("settings.json"));
    fs::write(work.join("settings.json"), "{}\n").unwrap();
    remove_link(work.join("agents"));
    fs::write(work.join("settings.json.before-share-2026-11-20"), "{}\n").unwrap();
    assert_linked(&sb, &personal, &["CLAUDE.md", "rules"]);
    let out = any_days(&stdout_of(
        &sb.cam(&sb.root).arg("doctor").assert().success(),
    ));
    for expected in [
        "ok    account personal: logged in as personal@example.com (Acme, team), login expires 2099-01-01 (in N days)\n\
         ok    account personal: shares instructions, linked to ~/.claude\n\
         ok    account work: logged in as work@example.com (Acme, team), login expires 2099-01-01 (in N days)\n",
        "warn  account work: its settings.json is a file, not a link to ~/.claude/settings.json, so work doesn't share it\n      \
         hint: run `cam share work settings` to move it aside and link it\n\
         info  account work: ~/.claude/keybindings.json doesn't exist yet; once it does, the next `claude` in work links it\n\
         warn  account work: agents isn't linked to ~/.claude/agents/\n      \
         hint: the next `claude` in work links it\n\
         info  account work: settings.json.before-share-2026-11-20 is work's own copy from before it shared\n      \
         hint: copy what you want to keep from it into ~/.claude, then delete it\n",
    ] {
        assert!(out.contains(expected), "{out}");
    }
}

#[test]
fn completions_offer_the_groups() {
    let sb = Sandbox::new();
    let fish = String::from_utf8(
        sb.cam(&sb.root)
            .args(["completions", "fish"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    for expected in [
        "complete -c cam -n \"__fish_cam_using_subcommand share\" -f -a 'work'\n",
        "complete -c cam -n \"__fish_cam_using_subcommand share\" -f -a 'settings' -d 'permissions, hooks, model, theme, status line, plugins'\n",
        "complete -c cam -n \"__fish_cam_using_subcommand unshare\" -f -a 'all' -d 'every group'\n",
    ] {
        assert!(fish.contains(expected), "{expected}\n---\n{fish}");
    }
}
