//! A stand-in for `claude` in cam's integration tests (`tests/cli.rs`). It
//! isn't an example of anything: it lives here because `cargo test` builds
//! examples, which gives the tests a fake `claude` executable on every
//! platform without shipping it in cam's package or releases.
//!
//! `auth login`, `auth status` and `auth logout` act on a fake login in
//! `$CLAUDE_CONFIG_DIR`, or in the home dir for Claude Code's own setup
//! (`HOME`, or `USERPROFILE` on Windows, as cam reads them), and log the
//! call to `$FAKE_LOG`. Anything else prints its path, its argv and cam's
//! variables, and exits with `$FAKE_EXIT`.
//!
//! - `FAKE_LOGIN=fail` makes the login fail; `nothing` makes it succeed
//!   without logging in.
//! - `FAKE_EMAIL` is the email a login gets (default:
//!   `<account>@example.com`).
//! - `FAKE_ORG` is the org a login gets (default: `Acme`).
//! - `auth login --console` makes a Console login, which `auth status`
//!   reports as an API key, not a claude.ai login.
//! - `FAKE_EXPIRES` is the `loginExpiresAt` that `auth status` reports.
//! - `FAKE_LOGOUT=fail` makes the logout fail.
//! - With `ANTHROPIC_API_KEY` set, `auth status` reports the key instead.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A variable's value, set or not, as `${NAME-}` reads it.
fn var(name: &str) -> Option<String> {
    env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

/// A variable's value if it's set and not empty, as `${NAME:-}` reads it.
fn set(name: &str) -> Option<String> {
    var(name).filter(|value| !value.is_empty())
}

/// Where Claude Code's own setup keeps its `.claude.json`.
fn home() -> PathBuf {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(var(name).unwrap_or_default())
}

/// `path` with `suffix` added to its file name: `.claude.json.email`.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// The first line of a file that isn't empty, trimmed, as `read -r` reads
/// it after `[ -s file ]`.
fn first_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path)
        .ok()
        .filter(|text| !text.is_empty())?;
    Some(text.lines().next().unwrap_or_default().trim().to_owned())
}

/// Writes `text` to stdout as it is, bytes and all on Unix.
fn out(text: &OsStr) {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(text).to_vec();
    #[cfg(not(unix))]
    let bytes = text.to_string_lossy().into_owned().into_bytes();
    io::stdout().write_all(&bytes).unwrap();
}

fn main() -> ExitCode {
    let mut argv = env::args_os();
    let program = argv.next().unwrap_or_default();
    let args: Vec<OsString> = argv.collect();
    let word = |at: usize| args.get(at).and_then(|arg| arg.to_str());
    let auth = word(0) == Some("auth");

    if auth && let Some(log) = set("FAKE_LOG") {
        let words: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let dir = var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| "system".to_owned());
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .and_then(|mut file| writeln!(file, "{} ({dir})", words.join(" ")))
            .unwrap();
    }
    let json = match set("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => home().join(".claude.json"),
    };
    match (auth, word(1)) {
        (true, Some("login")) => login(&args, &json),
        (true, Some("status")) => status(&json),
        (true, Some("logout")) => logout(&json),
        _ => session(&program, &args),
    }
}

fn login(args: &[OsString], json: &Path) -> ExitCode {
    match var("FAKE_LOGIN").as_deref() {
        Some("fail") => {
            eprintln!("Login cancelled.");
            return ExitCode::FAILURE;
        }
        Some("nothing") => return ExitCode::SUCCESS,
        _ => {}
    }
    if args.iter().any(|arg| arg == "--console") {
        fs::write(beside(json, ".console"), "").unwrap();
        println!("Login successful.");
        return ExitCode::SUCCESS;
    }
    let email = set("FAKE_EMAIL").unwrap_or_else(|| {
        let account = set("CAM_ACTIVE_ACCOUNT").unwrap_or_else(|| "system".to_owned());
        format!("{account}@example.com")
    });
    fs::write(
        json,
        format!("{{\"oauthAccount\": {{\"emailAddress\": \"{email}\"}}}}\n"),
    )
    .unwrap();
    fs::write(beside(json, ".email"), format!("{email}\n")).unwrap();
    let org = set("FAKE_ORG").unwrap_or_else(|| "Acme".to_owned());
    fs::write(beside(json, ".org"), format!("{org}\n")).unwrap();
    println!("Login successful.");
    ExitCode::SUCCESS
}

fn status(json: &Path) -> ExitCode {
    if set("ANTHROPIC_API_KEY").is_some() {
        println!(
            r#"{{"loggedIn": true, "authMethod": "api_key", "apiKeySource": "ANTHROPIC_API_KEY"}}"#
        );
        return ExitCode::SUCCESS;
    }
    if beside(json, ".console").exists() {
        println!(
            r#"{{"loggedIn": true, "authMethod": "api_key", "apiKeySource": "/login managed key"}}"#
        );
        return ExitCode::SUCCESS;
    }
    if let Some(email) = first_line(&beside(json, ".email")) {
        let org = first_line(&beside(json, ".org")).unwrap_or_else(|| "Acme".to_owned());
        let expires = set("FAKE_EXPIRES").unwrap_or_else(|| "2099-01-01T00:00:00.000Z".to_owned());
        println!(
            r#"{{"loggedIn": true, "authMethod": "claude.ai", "email": "{email}", "orgName": "{org}", "subscriptionType": "team", "loginExpiresAt": "{expires}"}}"#
        );
        return ExitCode::SUCCESS;
    }
    println!(r#"{{"loggedIn": false, "authMethod": "none", "loginExpiresAt": null}}"#);
    ExitCode::FAILURE
}

fn logout(json: &Path) -> ExitCode {
    if var("FAKE_LOGOUT").as_deref() == Some("fail") {
        eprintln!("Logout failed.");
        return ExitCode::FAILURE;
    }
    fs::write(beside(json, ".email"), "").unwrap();
    println!("Successfully logged out from your Anthropic account.");
    ExitCode::SUCCESS
}

/// Started as a session: says how it was started.
fn session(program: &OsStr, args: &[OsString]) -> ExitCode {
    out(OsStr::new("claude: "));
    out(program);
    out(OsStr::new("\nargv:"));
    for arg in args {
        out(OsStr::new(" ["));
        out(arg);
        out(OsStr::new("]"));
    }
    let shown = |name: &str| var(name).unwrap_or_else(|| "unset".to_owned());
    println!();
    println!("config: {}", shown("CLAUDE_CONFIG_DIR"));
    println!("account: {}", shown("CAM_ACTIVE_ACCOUNT"));
    println!("depth: {}", shown("CAM_SHIM_DEPTH"));
    let code = set("FAKE_EXIT").and_then(|code| code.parse::<u8>().ok());
    ExitCode::from(code.unwrap_or(0))
}
