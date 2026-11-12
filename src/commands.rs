//! The subcommands.

use std::convert::Infallible;
use std::env;
use std::ffi::OsString;
use std::fmt::Display;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use toml::value::Date;

use crate::account::Selection;
use crate::config::{self, Config};
use crate::date;
use crate::discover::{self, ENV_CLAUDE_BIN, Search};
use crate::error::Error;
use crate::launch::{self, Credential, ENV_ACTIVE, ENV_DEPTH};
use crate::marker::{self, FILE_NAME};
use crate::redact::redact;
use crate::resolve;
use crate::shim::{self, Installed};
use crate::store::{self, SecretStore};
use crate::token::Token;

/// Prints a line to stdout, redacted like everything else cam prints.
fn say(line: impl Display) {
    println!("{}", redact(&line.to_string()));
}

/// Prints a note to stderr, redacted.
fn note(line: impl Display) {
    eprintln!("note: {}", redact(&line.to_string()));
}

/// Prints a warning to stderr, redacted.
fn warn(line: impl Display) {
    eprintln!("warning: {}", redact(&line.to_string()));
}

/// A name `add` and `remove` accept: valid, and not `none`.
fn account_name(name: &str) -> Result<String, Error> {
    match Selection::parse(name) {
        Some(Selection::Account(name)) => Ok(name),
        Some(Selection::PassThrough) => Err(Error::ReservedName),
        None => Err(Error::InvalidNameArg {
            name: name.to_owned(),
        }),
    }
}

fn is_plausible_email(email: &str) -> bool {
    email
        .split_once('@')
        .is_some_and(|(user, domain)| !user.is_empty() && !domain.is_empty())
        && !email.chars().any(char::is_whitespace)
}

/// `cam add`: stores the token, then records the account in the config.
/// If the config write fails, a newly stored token is deleted again.
pub fn add(
    name: &str,
    email: Option<&str>,
    force: bool,
    config_path: &Path,
    store: &dyn SecretStore,
    read_token: impl FnOnce(&str) -> Result<Token, Error>,
) -> Result<()> {
    let name = account_name(name)?;
    if let Some(email) = email
        && !is_plausible_email(email)
    {
        return Err(Error::InvalidEmail {
            email: email.to_owned(),
        }
        .into());
    }
    // A broken config fails here, before the token prompt.
    Config::load(config_path)?;
    let had_token = store.contains(&name)?;
    if had_token && !force {
        return Err(Error::TokenExists {
            name,
            location: store.location(),
        }
        .into());
    }
    let token = read_token(&name)?;
    store.set(&name, &token)?;
    if let Err(err) = config::add_account(config_path, &name, email, date::today()) {
        if !had_token {
            // Best effort: don't leave a token behind that the config doesn't know.
            let _ = store.delete(&name);
        }
        return Err(err.into());
    }
    let verb = if had_token { "replaced" } else { "added" };
    say(format_args!(
        "{verb} {name}: token saved in {}",
        store.location()
    ));
    Ok(())
}

/// `cam remove`: deletes the token first, so a store failure leaves the
/// config untouched, then the config entry.
pub fn remove(name: &str, config_path: &Path, store: &dyn SecretStore) -> Result<()> {
    let name = account_name(name)?;
    // A broken config fails here, before anything is deleted.
    Config::load(config_path)?;
    let had_token = store.delete(&name)?;
    let removed = config::remove_account(config_path, &name)?;
    let location = store.location();
    match (removed.existed, had_token) {
        (false, false) => return Err(Error::NothingToRemove { name }.into()),
        (true, true) => say(format_args!(
            "removed {name}: config entry and token in {location}"
        )),
        (true, false) => say(format_args!(
            "removed {name}: config entry (no token was stored)"
        )),
        (false, true) => say(format_args!(
            "removed {name}: token in {location} (it had no config entry)"
        )),
    }
    if removed.was_default {
        note(format_args!(
            "{name} was the default, so `default` is now unset"
        ));
    }
    Ok(())
}

/// `cam list`: reads only the config, never the store.
pub fn list(config_path: &Path) -> Result<()> {
    let config = Config::load(config_path)?;
    for line in list_lines(&config, date::today()) {
        say(line);
    }
    Ok(())
}

fn list_lines(config: &Config, today: Date) -> Vec<String> {
    if config.accounts().is_empty() {
        return vec!["no accounts yet; add one with `cam add <name>`".to_owned()];
    }
    let default = config.default_selection();
    let mut rows = vec![[
        "ACCOUNT".to_owned(),
        "EMAIL".to_owned(),
        "TOKEN ADDED".to_owned(),
    ]];
    for (name, account) in config.accounts() {
        let is_default = default.as_ref() == Some(&Selection::Account(name.clone()));
        let added = match account.token_created.and_then(|created| created.date) {
            Some(created) => format!("{created} ({})", date::age(created, today)),
            None => "unknown".to_owned(),
        };
        rows.push([
            if is_default {
                format!("{name} (default)")
            } else {
                name.clone()
            },
            account.email.clone().unwrap_or_else(|| "-".to_owned()),
            added,
        ]);
    }
    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (name_width, email_width) = (width(0), width(1));
    let mut lines: Vec<String> = rows
        .iter()
        .map(|[name, email, added]| format!("{name:<name_width$}  {email:<email_width$}  {added}"))
        .collect();
    if default == Some(Selection::PassThrough) {
        lines.push(String::new());
        lines.push("default: none (pass-through)".to_owned());
    }
    lines
}

pub fn which(config_path: &Path) -> Result<()> {
    let env_account = resolve::env_account()?;
    let cwd = current_dir()?;
    let resolution = resolve::resolve(env_account.as_deref(), &cwd, config_path)?;
    say(resolution);
    Ok(())
}

/// `cam exec`: resolves the account, then replaces cam with the real
/// `claude`. Returns only on failure. Pass-through never opens the store, so
/// it can't trigger a Keychain prompt.
pub fn exec(args: &[OsString], config_path: &Path) -> Result<Infallible> {
    // First, so a loop stops before doing any work.
    let depth = launch::child_depth(env::var_os(ENV_DEPTH).as_deref())?;
    let env_account = resolve::env_account()?;
    let resolution = resolve::resolve(env_account.as_deref(), &current_dir()?, config_path)?;

    let program = real_claude(&own_exe()?)?;

    let credential = match resolution.selection {
        Selection::PassThrough => Credential::PassThrough,
        Selection::Account(name) => {
            let store = store::open()?;
            let Some(token) = store.get(&name)? else {
                return Err(Error::NoToken {
                    name,
                    location: store.location(),
                }
                .into());
            };
            for var in launch::outranking(|key| env::var_os(key)) {
                warn(format_args!(
                    "{var} is set and outranks the token for \"{name}\"; unset it to use that account"
                ));
            }
            Credential::Account { name, token }
        }
    };
    let inherited_active = env::var_os(ENV_ACTIVE).is_some();
    let source = launch::command(&program, args, &credential, inherited_active, depth).exec();
    Err(Error::Exec {
        path: program,
        source,
    }
    .into())
}

/// The real `claude`, found the way `exec` finds it.
fn real_claude(own_exe: &Path) -> Result<PathBuf, Error> {
    let shim_dir = shim::default_dir(dirs::home_dir());
    let (claude_bin, path) = (env::var_os(ENV_CLAUDE_BIN), env::var_os("PATH"));
    discover::real_claude(&Search {
        claude_bin: claude_bin.as_deref(),
        path: path.as_deref(),
        own_exe,
        shim_dir: shim_dir.as_deref(),
    })
}

/// cam's own executable, as an absolute path.
fn own_exe() -> Result<PathBuf, Error> {
    let exe = env::current_exe().map_err(Error::CurrentExe)?;
    if exe.is_absolute() {
        Ok(exe)
    } else {
        fs::canonicalize(exe).map_err(Error::CurrentExe)
    }
}

/// `--dir`, made absolute, or the default shim dir.
fn shim_dir(dir: Option<&Path>) -> Result<PathBuf> {
    match dir {
        Some(dir) => {
            std::path::absolute(dir).with_context(|| format!("cannot open {}", dir.display()))
        }
        None => shim::default_dir(dirs::home_dir())
            .ok_or_else(|| anyhow!("cannot find the home directory; pass --dir")),
    }
}

/// `cam shim install`: creates or updates the shim, then says what, if
/// anything, is left to do on PATH. Never edits shell startup files.
pub fn shim_install(dir: Option<&Path>) -> Result<()> {
    let dir = shim_dir(dir)?;
    let own_exe = own_exe()?;
    let path = env::var_os("PATH");
    let target = shim::target(&own_exe, path.as_deref());
    let link = dir.join(shim::NAME);
    let (link_shown, target_shown) = (link.display(), target.display());
    match shim::install(&dir, &target, &own_exe)? {
        Installed::Created => say(format_args!("installed {link_shown} -> {target_shown}")),
        Installed::Replaced { previous } => say(format_args!(
            "replaced {link_shown} -> {target_shown} (it pointed at {})",
            previous.display()
        )),
        Installed::Unchanged => {
            say(format_args!(
                "already installed: {link_shown} -> {target_shown}"
            ));
        }
    }
    let order = shim::path_order(path.as_deref(), &dir, &own_exe);
    let home = dirs::home_dir();
    say("");
    for line in shim::advice(
        &order,
        env::var_os("SHELL").as_deref(),
        &dir,
        home.as_deref(),
    ) {
        say(line);
    }
    if let Err(Error::ClaudeNotFound) = real_claude(&own_exe) {
        note("no real `claude` found on PATH yet; install Claude Code, or set CAM_CLAUDE_BIN");
    }
    Ok(())
}

/// `cam shim uninstall`: removes the shim, if it is one.
pub fn shim_uninstall(dir: Option<&Path>) -> Result<()> {
    let dir = shim_dir(dir)?;
    let removed = shim::uninstall(&dir, &own_exe()?)?;
    say(format_args!("removed {}", removed.display()));
    let on_path = env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|entry| discover::same_file(&entry, &dir)));
    if on_path {
        note(format_args!(
            "{} is still on PATH; you can remove that line from your shell's startup file",
            dir.display()
        ));
    }
    Ok(())
}

pub fn use_account(name: &str, dir: Option<&Path>, config_path: &Path) -> Result<()> {
    let selection = Selection::parse(name).ok_or_else(|| Error::InvalidNameArg {
        name: name.to_owned(),
    })?;
    if let Selection::Account(name) = &selection
        && !Config::load(config_path)?.has_account(name)
    {
        return Err(Error::UnknownAccountArg { name: name.clone() }.into());
    }
    let dir = target_dir(dir)?;
    let path = marker::write(&dir, &selection)
        .with_context(|| format!("cannot write {}", dir.join(FILE_NAME).display()))?;
    say(format_args!("wrote {} ({selection})", path.display()));
    Ok(())
}

pub fn unuse(dir: Option<&Path>) -> Result<()> {
    let dir = target_dir(dir)?;
    let path = dir.join(FILE_NAME);
    match fs::remove_file(&path) {
        Ok(()) => {
            say(format_args!("removed {}", path.display()));
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let mut message = format!("no {FILE_NAME} in {}", dir.display());
            if let Some(nearest) = dir.parent().and_then(marker::nearest) {
                let parent = nearest.parent().unwrap_or(&dir);
                message.push_str(&format!(
                    "\nhint: the nearest one is {}; pass --dir {} to remove it",
                    nearest.display(),
                    parent.display()
                ));
            }
            Err(anyhow!(message))
        }
        Err(err) => Err(err).with_context(|| format!("cannot remove {}", path.display())),
    }
}

fn current_dir() -> Result<PathBuf> {
    env::current_dir().context("cannot read the current directory")
}

/// `dir`, or the current directory, made absolute. Must be an existing directory.
fn target_dir(dir: Option<&Path>) -> Result<PathBuf> {
    let dir = match dir {
        Some(dir) => dir.to_owned(),
        None => current_dir()?,
    };
    let canonical =
        fs::canonicalize(&dir).with_context(|| format!("cannot open {}", dir.display()))?;
    ensure!(canonical.is_dir(), "{} is not a directory", dir.display());
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::store::FileStore;
    use crate::testutil::{Tree, fake_token};

    fn store(tree: &Tree) -> FileStore {
        FileStore::new(tree.root.join("store"))
    }

    fn no_prompt(_: &str) -> Result<Token, Error> {
        panic!("must not prompt for a token")
    }

    #[test]
    fn add_stores_token_and_records_account() {
        let tree = Tree::new();
        let config = tree.root.join("config.toml");
        let store = store(&tree);
        add(
            "work",
            Some("you@company.com"),
            false,
            &config,
            &store,
            |name| {
                assert_eq!(name, "work");
                Ok(fake_token(1))
            },
        )
        .unwrap();
        assert!(store.contains("work").unwrap());
        let loaded = Config::load(&config).unwrap();
        let account = &loaded.accounts()["work"];
        assert_eq!(account.email.as_deref(), Some("you@company.com"));
        assert_eq!(
            account.token_created.and_then(|created| created.date),
            Some(date::today())
        );
    }

    #[test]
    fn add_refuses_to_replace_without_force() {
        let tree = Tree::new();
        let config = tree.root.join("config.toml");
        let store = store(&tree);
        add("work", None, false, &config, &store, |_| Ok(fake_token(1))).unwrap();
        let err = add("work", None, false, &config, &store, no_prompt).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");

        add("work", None, true, &config, &store, |_| Ok(fake_token(2))).unwrap();
        let stored = fs::read_to_string(tree.root.join("store/work")).unwrap();
        assert_eq!(stored, fake_token(2).expose());
    }

    #[test]
    fn add_fills_in_a_hand_written_account_without_force() {
        let tree = Tree::new();
        let config = tree.config("[accounts.work]\nemail = \"you@company.com\"\n");
        let store = store(&tree);
        add("work", None, false, &config, &store, |_| Ok(fake_token(1))).unwrap();
        let loaded = Config::load(&config).unwrap();
        assert_eq!(
            loaded.accounts()["work"].email.as_deref(),
            Some("you@company.com")
        );
    }

    #[test]
    fn add_validates_before_prompting() {
        let tree = Tree::new();
        let config = tree.root.join("config.toml");
        let store = store(&tree);
        for (name, email) in [
            ("Work", None),
            ("none", None),
            ("work", Some("not-an-email")),
            ("work", Some("a b@c.d")),
        ] {
            assert!(add(name, email, false, &config, &store, no_prompt).is_err());
        }
        let broken = tree.config("not toml");
        assert!(add("work", None, false, &broken, &store, no_prompt).is_err());
        assert!(!store.contains("work").unwrap());
    }

    #[test]
    fn add_rolls_back_the_token_if_the_config_cannot_be_written() {
        let tree = Tree::new();
        let dir = tree.dir("locked");
        let config = dir.join("config.toml");
        fs::write(&config, "").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        let store = store(&tree);
        let result = add("work", None, false, &config, &store, |_| Ok(fake_token(1)));
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(!store.contains("work").unwrap());
    }

    #[test]
    fn add_with_force_keeps_the_new_token_if_the_config_cannot_be_written() {
        let tree = Tree::new();
        let dir = tree.dir("locked");
        let config = dir.join("config.toml");
        let store = store(&tree);
        add("work", None, false, &config, &store, |_| Ok(fake_token(1))).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        // A new email forces a config write, which the locked dir makes fail.
        let result = add(
            "work",
            Some("new@company.com"),
            true,
            &config,
            &store,
            |_| Ok(fake_token(2)),
        );
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(store.contains("work").unwrap());
    }

    #[test]
    fn remove_deletes_token_and_entry() {
        let tree = Tree::new();
        let config = tree.root.join("config.toml");
        let store = store(&tree);
        add("work", None, false, &config, &store, |_| Ok(fake_token(1))).unwrap();
        remove("work", &config, &store).unwrap();
        assert!(!store.contains("work").unwrap());
        assert!(!Config::load(&config).unwrap().has_account("work"));
    }

    #[test]
    fn remove_handles_partial_state() {
        let tree = Tree::new();
        let store = store(&tree);
        // Config entry without a token.
        let config = tree.config("[accounts.work]\n");
        remove("work", &config, &store).unwrap();
        assert!(!Config::load(&config).unwrap().has_account("work"));
        // A token without a config entry.
        store.set("work", &fake_token(1)).unwrap();
        remove("work", &config, &store).unwrap();
        assert!(!store.contains("work").unwrap());
        // Neither.
        let err = remove("work", &config, &store).unwrap_err();
        assert!(err.to_string().contains("nothing to remove"), "{err}");
    }

    #[test]
    fn remove_with_broken_config_deletes_nothing() {
        let tree = Tree::new();
        let store = store(&tree);
        store.set("work", &fake_token(1)).unwrap();
        let config = tree.config("not toml");
        assert!(remove("work", &config, &store).is_err());
        assert!(store.contains("work").unwrap());
    }

    fn config_from(text: &str) -> Config {
        let tree = Tree::new();
        Config::load(&tree.config(text)).unwrap()
    }

    const TODAY: Date = Date {
        year: 2026,
        month: 11,
        day: 11,
    };

    #[test]
    fn list_empty() {
        assert_eq!(
            list_lines(&config_from(""), TODAY),
            ["no accounts yet; add one with `cam add <name>`"]
        );
    }

    #[test]
    fn list_aligns_columns_and_marks_default() {
        let config = config_from(
            "default = \"work\"\n\
             [accounts.work]\nemail = \"you@company.com\"\ntoken_created = 2026-11-11\n\
             [accounts.personal]\ntoken_created = 2026-11-02\n\
             [accounts.old]\n",
        );
        assert_eq!(
            list_lines(&config, TODAY),
            [
                "ACCOUNT         EMAIL            TOKEN ADDED",
                "old             -                unknown",
                "personal        -                2026-11-02 (9 days ago)",
                "work (default)  you@company.com  2026-11-11 (today)",
            ]
        );
    }

    #[test]
    fn list_shows_pass_through_default() {
        let config = config_from("default = \"none\"\n[accounts.work]\n");
        let lines = list_lines(&config, TODAY);
        assert_eq!(lines.last().unwrap(), "default: none (pass-through)");
    }
}
