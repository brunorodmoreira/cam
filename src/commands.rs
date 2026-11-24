//! The subcommands.

use std::convert::Infallible;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs;
use std::io::{self, BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::account::{self, SYSTEM, Selection};
use crate::auth::{self, Expiry};
use crate::config::{self, Config};
use crate::conflict;
use crate::date;
use crate::discover::{self, ENV_CLAUDE_BIN, Search};
use crate::doctor::{self, Login};
use crate::error::{Error, Named, unknown_hint};
use crate::init::{self, Shell};
use crate::launch::{self, ENV_ACTIVE, ENV_CONFIG_DIR, ENV_DEPTH, Target};
use crate::marker::{self, FILE_NAME};
use crate::platform;
use crate::redact::redact;
use crate::resolve;
use crate::settings;
use crate::share::{self, Group, Parsed, State, Unlinked};
use crate::shim::{self, Reaches};

/// Prints a line to stdout, redacted like everything else cam prints.
fn say(line: impl Display) {
    println!("{}", redact(&line.to_string()));
}

/// Prints a line to stderr, redacted: for messages around an interactive
/// step, which stdout would mix into the step's own output.
fn tell(line: impl Display) {
    eprintln!("{}", redact(&line.to_string()));
}

/// Prints a note to stderr, redacted.
fn note(line: impl Display) {
    eprintln!("note: {}", redact(&line.to_string()));
}

/// Prints a warning to stderr, redacted.
fn warn(line: impl Display) {
    eprintln!("warning: {}", redact(&line.to_string()));
}

/// Whether cam can ask the user: stdin and stderr are both a terminal.
fn can_ask() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// Asks a yes-or-no question on the terminal. Only `y` or `yes` is yes.
fn ask_yes(question: &str) -> Result<bool> {
    eprint!("{} [y/N] ", redact(question));
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .context("cannot read the answer")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// The groups a command line names. `none` only for `--share`.
fn named_groups(args: &[String], none_allowed: bool) -> Result<Vec<Group>, Error> {
    match share::parse_groups(args)? {
        Parsed::Groups(groups) => Ok(groups),
        Parsed::Nothing if none_allowed => Ok(Vec::new()),
        Parsed::Nothing => Err(Error::NoneNotHere),
    }
}

/// Shows the groups on the terminal, `checked` ticked, each with what it
/// would share today, and returns the ones ticked. `None` if cancelled.
fn pick_groups(name: &str, home: &Path, checked: &[Group]) -> Result<Option<Vec<Group>>> {
    let width = Group::ALL
        .iter()
        .map(|group| group.name().len())
        .max()
        .unwrap_or(0);
    let items: Vec<String> = Group::ALL
        .iter()
        .map(|&group| format!("{:<width$}  {}", group.name(), share::preview(home, group)))
        .collect();
    let defaults: Vec<bool> = Group::ALL
        .iter()
        .map(|group| checked.contains(group))
        .collect();
    tell(format_args!(
        "What should {name} share with your own setup in ~/.claude?\n\
         A shared group is the same files for both: a change in either account shows up in the other.\n\
         MCP servers are never shared."
    ));
    let picked = dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Space ticks, Enter confirms; nothing ticked shares nothing")
        .items(&items)
        .defaults(&defaults)
        // What was picked is said afterwards, in cam's words.
        .report(false)
        .interact_on_opt(&dialoguer::console::Term::stderr())
        .context("cannot show the choice")?;
    Ok(picked.map(|indexes| indexes.into_iter().map(|index| Group::ALL[index]).collect()))
}

/// Links `groups` into account `dir`. Fails on the first link that can't
/// be made.
fn link_groups(home: &Path, dir: &Path, groups: &[Group]) -> Result<share::TopUp> {
    let mut result = share::top_up(home, dir, groups);
    if let Some((entry, source)) = result.failed.pop() {
        return Err(Error::Link {
            path: dir.join(entry.name),
            target: share::shared_path(home, entry),
            source,
        }
        .into());
    }
    Ok(result)
}

/// Before a launch or a login: links whatever `groups` are missing, and
/// warns about what the account can't share, since the account's own files
/// are never replaced. Problems are warnings, so the launch goes on.
fn top_up_links(name: &str, home: &Path, dir: &Path, groups: &[Group]) {
    if groups.is_empty() {
        return;
    }
    let result = share::top_up(home, dir, groups);
    for (group, entry, own) in result.own {
        warn(format_args!(
            "{name}'s {} is {}, not a link to {}, so {name} doesn't share it\n\
             hint: run `cam share {name} {group}` to move it aside and link it",
            entry.name,
            own.describe(),
            entry.shared_shown()
        ));
    }
    for (entry, err) in result.failed {
        warn(format_args!(
            "cannot link {name}'s {} to {}: {err}",
            entry.name,
            entry.shared_shown()
        ));
    }
}

/// A name `add` and `remove` accept: valid, and not `system`.
fn account_name(name: &str) -> Result<String, Error> {
    match Selection::parse(name) {
        Some(Selection::Account(name)) => Ok(name),
        Some(Selection::System) => Err(Error::ReservedName),
        None => Err(Error::InvalidNameArg {
            name: name.to_owned(),
        }),
    }
}

fn home() -> Result<PathBuf, Error> {
    platform::home_dir().ok_or(Error::NoHome)
}

/// A variable that is set and non-empty.
fn env_set(key: &str) -> Option<OsString> {
    env::var_os(key).filter(|value| !value.is_empty())
}

/// A `CLAUDE_CONFIG_DIR` the user exported, which moves `system`'s setup.
/// Inside a cam session the variable is that session's account, not theirs.
fn user_config_dir() -> Option<PathBuf> {
    env_set(ENV_ACTIVE)
        .is_none()
        .then(|| env_set(ENV_CONFIG_DIR).map(PathBuf::from))
        .flatten()
}

/// The `CAM_SHIM_DEPTH` to give `claude`.
fn depth() -> Result<u32, Error> {
    launch::child_depth(env::var_os(ENV_DEPTH).as_deref())
}

/// `claude` with `args`, in the environment `exec` would give it.
fn claude(program: &Path, args: &[impl AsRef<OsStr>], target: Target<'_>, depth: u32) -> Command {
    let inherited_active = env::var_os(ENV_ACTIVE).is_some();
    launch::command(program, args, target, inherited_active, depth)
}

/// What Claude Code says about the login itself: `claude auth status` with
/// whatever would outrank the login left out.
fn login_status(program: &Path, target: Target<'_>, depth: u32) -> Result<auth::Status, Error> {
    let mut command = claude(program, &["auth", "status", "--json"], target, depth);
    launch::without_outranking(&mut command);
    auth::status(command, program)
}

/// Runs Claude Code's login for `target`, with `login_args` passed on
/// (`--email`, `--sso`), then checks with Claude Code that it worked.
/// `retry` is the command that tries again.
fn log_in(
    program: &Path,
    target: Target<'_>,
    depth: u32,
    login_args: &[OsString],
    retry: &str,
) -> Result<auth::Status> {
    let args: Vec<&OsStr> = [OsStr::new("auth"), OsStr::new("login")]
        .into_iter()
        .chain(login_args.iter().map(OsString::as_os_str))
        .collect();
    let exit = auth::login(claude(program, &args, target, depth), program)?;
    if !exit.success() {
        return Err(Error::LoginFailed {
            status: auth::describe_exit(exit),
            retry: retry.to_owned(),
        }
        .into());
    }
    let status = login_status(program, target, depth)?;
    if let Some(method) = status.other_method() {
        return Err(Error::NoSubscription { method }.into());
    }
    if !status.is_login() {
        bail!("Claude Code reports no login after `claude auth login`");
    }
    Ok(status)
}

/// Says, before the browser opens, which claude.ai account the login will
/// use, and which emails `system` and the other accounts already use.
fn before_login(name: &str, config: Option<&Config>, home: &Path) {
    tell(format_args!(
        "Logging {name} in with Claude Code. It uses the claude.ai account your browser is signed in to."
    ));
    let others: Vec<String> = config
        .map(|config| recorded_emails(config, home))
        .unwrap_or_default()
        .into_iter()
        .filter(|(other, _)| other != name)
        .map(|(other, email)| format!("{email} ({other})"))
        .collect();
    if !others.is_empty() {
        tell(format_args!("Already in use: {}.", others.join(", ")));
    }
}

/// The emails Claude Code last recorded for `system` and each account, as
/// (name, email). Read from `.claude.json` files, so nothing runs.
fn recorded_emails(config: &Config, home: &Path) -> Vec<(String, String)> {
    let system = auth::recorded_email(&auth::claude_json(user_config_dir().as_deref(), home))
        .map(|email| (SYSTEM.to_owned(), email));
    let accounts = config.accounts().keys().filter_map(|name| {
        let dir = account::dir(home, name);
        auth::recorded_email(&auth::claude_json(Some(&dir), home))
            .map(|email| (name.clone(), email))
    });
    system.into_iter().chain(accounts).collect()
}

/// Warns when `name` just logged in as the account `system` or another
/// account already uses: the browser was probably still signed in to it.
/// The emails come from each `.claude.json`. On a match, Claude Code is
/// asked about the other login too, since one email can belong to several
/// orgs, and a different org is no mistake.
fn warn_same_account(
    name: &str,
    status: &auth::Status,
    config: &Config,
    home: &Path,
    program: &Path,
    depth: u32,
) {
    let Some(email) = &status.email else {
        return;
    };
    for (other, other_email) in recorded_emails(config, home) {
        if other == name || !other_email.eq_ignore_ascii_case(email) {
            continue;
        }
        let dir = account::dir(home, &other);
        let target = match other.as_str() {
            SYSTEM => Target::System,
            _ => Target::Account {
                name: &other,
                dir: &dir,
            },
        };
        if let Ok(other_status) = login_status(program, target, depth)
            && other_status.is_login()
            && other_status.org_name.is_some()
            && other_status.org_name != status.org_name
        {
            continue;
        }
        let like = match other.as_str() {
            SYSTEM => "your own login (system)".to_owned(),
            _ => other.clone(),
        };
        warn(format_args!(
            "{name} is logged in as {email}, the same account as {like}: the browser was probably still signed in to it\n\
             hint: if {name} should be another claude.ai account, switch accounts in the browser, then run `cam login {name}`"
        ));
    }
}

/// Whether `args` ask `claude` for print mode (`-p`), where Claude Code
/// always uses `ANTHROPIC_API_KEY` without asking.
fn print_mode(args: &[OsString]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "-p" || arg == "--print")
}

/// Tells about each variable set in this shell that outranks a login.
/// Claude Code asks once whether to use `ANTHROPIC_API_KEY` in an
/// interactive session, so that one is only a note there; with `-p` it's
/// used without asking. The others are always used. Also warns about
/// [`conflict::SHARED_STORE`], which puts every account's login in one place.
fn warn_outranking(name: &str, print_mode: bool) {
    if env::var_os(conflict::SHARED_STORE).is_some() {
        warn(format_args!(
            "{} is set, so every account keeps its login in the same place, and the last one logged in is used; unset it, so \"{name}\" keeps its own login",
            conflict::SHARED_STORE
        ));
    }
    for var in launch::outranking(|key| env::var_os(key)) {
        if var == launch::ENV_API_KEY && !print_mode {
            note(format_args!(
                "{var} is set; if you approve it when Claude Code asks, it's used instead of the login of \"{name}\" (\"Use custom API key\" in /config)"
            ));
        } else {
            warn(format_args!(
                "{var} is set and outranks the login of \"{name}\"; unset it to use that account"
            ));
        }
    }
}

/// `cam add`: creates the account dir with the links of the groups it
/// shares, runs Claude Code's login in it, and records the account once
/// Claude Code confirms the login. If anything fails, it logs out and
/// deletes the dir without following links, so a retry starts clean.
pub fn add(
    name: &str,
    share_args: &[String],
    login_args: &[OsString],
    config_path: &Path,
) -> Result<()> {
    let name = account_name(name)?;
    let named = match share_args {
        [] => None,
        args => Some(named_groups(args, true)?),
    };
    // A broken config fails here, before anything is created.
    let config = Config::load(config_path)?;
    let home = home()?;
    let dir = account::dir(&home, &name);
    if account::exists(&dir) {
        return Err(Error::AccountExists { name, dir }.into());
    }
    let depth = depth()?;
    let own_exe = own_exe()?;
    let program = real_claude(&own_exe)?;
    let groups = match named {
        Some(groups) => groups,
        None if can_ask() => pick_groups(&name, &home, &[])?
            .ok_or_else(|| anyhow!("cancelled, so {name} was not added"))?,
        None => Vec::new(),
    };
    account::create_dir(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    if let Err(err) = link_groups(&home, &dir, &groups) {
        let _ = fs::remove_dir_all(&dir);
        return Err(err.context(format!("{name} was not added")));
    }
    let target = Target::Account {
        name: &name,
        dir: &dir,
    };
    before_login(&name, Some(&config), &home);
    let retry = format!("cam add {name}");
    let added = log_in(&program, target, depth, login_args, &retry).and_then(|status| {
        config::add_account(config_path, &name)?;
        config::set_share(config_path, &name, &groups)?;
        Ok(status)
    });
    match added {
        Ok(status) => {
            say(format_args!(
                "added {name}: logged in as {}",
                status.identity()
            ));
            if !groups.is_empty() {
                say(format_args!(
                    "{name} shares {} with ~/.claude",
                    share::describe(&groups)
                ));
            }
            warn_same_account(&name, &status, &config, &home, &program, depth);
            warn_outranking(&name, false);
            let reaches = shim::first_on_path(env::var_os("PATH").as_deref(), &own_exe);
            let shell = Shell::from_env(env::var_os("SHELL").as_deref());
            for line in next_steps(&name, matches!(reaches, Reaches::Shim(_)), shell, &groups) {
                say(line);
            }
            Ok(())
        }
        Err(err) => {
            // Best effort: leave nothing behind.
            let logout = claude(&program, &["auth", "logout"], target, depth);
            let _ = auth::logout(logout, &program);
            let _ = fs::remove_dir_all(&dir);
            Err(err.context(format!("{name} was not added")))
        }
    }
}

/// What to do after `cam add`: pin the account to a folder, send plain
/// `claude` through cam unless it already goes (`reached`), share parts of
/// `~/.claude` if it shares nothing yet, and expect an empty setup for the
/// rest.
fn next_steps(name: &str, reached: bool, shell: Option<Shell>, shares: &[Group]) -> Vec<String> {
    let mut steps = vec![(
        format!("cam use {name}"),
        format!("use {name} in this folder and the ones below it (or pass --dir)"),
    )];
    if !reached {
        let (file, line) = init::setup(shell);
        steps.push((
            line.to_owned(),
            format!("at the end of {file}, so plain `claude` picks the account"),
        ));
    }
    if shares.is_empty() {
        steps.push((
            format!("cam share {name}"),
            "share your settings, CLAUDE.md, skills or history with it".to_owned(),
        ));
    }
    let width = steps
        .iter()
        .map(|(command, _)| command.len())
        .max()
        .unwrap_or(0);
    let mut lines = vec![String::new(), "Next:".to_owned()];
    lines.extend(
        steps
            .iter()
            .map(|(command, what)| format!("  {command:<width$}  {what}")),
    );
    lines.push(String::new());
    lines.push(match shares {
        [] => format!(
            "{name} starts with an empty Claude Code setup of its own.\n\
             Its first session shows the first-run screens, without your settings, CLAUDE.md, skills or MCP servers."
        ),
        _ => format!(
            "Apart from {}, {name} starts with an empty Claude Code setup of its own.\n\
             Its first session shows the first-run screens, without your MCP servers.",
            share::describe(shares)
        ),
    });
    lines
}

/// `cam login`: runs Claude Code's login again for an account, by default
/// the one that applies here. Deletes nothing if it fails.
pub fn login(name: Option<&str>, login_args: &[OsString], config_path: &Path) -> Result<()> {
    let selection = match name {
        Some(name) => {
            let selection = Selection::parse(name).ok_or_else(|| Error::InvalidNameArg {
                name: name.to_owned(),
            })?;
            if let Selection::Account(name) = &selection {
                known_account(name, &Config::load(config_path)?)?;
            }
            selection
        }
        None => {
            let env_account = resolve::env_account()?;
            let resolution =
                resolve::resolve(env_account.as_deref(), &current_dir()?, config_path)?;
            say(&resolution);
            resolution.selection
        }
    };
    let home = home()?;
    let depth = depth()?;
    let dir = match &selection {
        Selection::Account(name) => {
            let dir = account::dir(&home, name);
            if !dir.is_dir() {
                return Err(Error::NoAccountDir {
                    name: name.clone(),
                    dir,
                }
                .into());
            }
            Some(dir)
        }
        Selection::System => None,
    };
    let program = real_claude(&own_exe()?)?;
    let target = match (&selection, &dir) {
        (Selection::Account(name), Some(dir)) => Target::Account { name, dir },
        _ => Target::System,
    };
    // Past this point the config only matters for the links and for naming
    // the other accounts.
    let config = Config::load(config_path).ok();
    if let (Selection::Account(name), Some(dir), Some(config)) = (&selection, &dir, &config) {
        top_up_links(name, &home, dir, &config.shares(name));
    }
    before_login(&selection.to_string(), config.as_ref(), &home);
    let retry = format!("cam login {selection}");
    let status = log_in(&program, target, depth, login_args, &retry)?;
    say(format_args!(
        "{selection}: logged in as {}",
        status.identity()
    ));
    if let Some(config) = &config {
        warn_same_account(
            &selection.to_string(),
            &status,
            config,
            &home,
            &program,
            depth,
        );
    }
    warn_outranking(&selection.to_string(), false);
    Ok(())
}

/// The configured account `name` and its dir, which must exist.
fn existing_account(name: &str, config: &Config, home: &Path) -> Result<PathBuf> {
    known_account(name, config)?;
    let dir = account::dir(home, name);
    if !dir.is_dir() {
        return Err(Error::NoAccountDir {
            name: name.to_owned(),
            dir,
        }
        .into());
    }
    Ok(dir)
}

/// `cam share`: links the groups into the account's dir and records them.
/// The account's own copies where the links go are moved aside, after
/// asking, and never deleted. Without groups, shows the choice on the
/// terminal, where unticking a group stops sharing it.
pub fn share(name: &str, group_args: &[String], yes: bool, config_path: &Path) -> Result<()> {
    let name = account_name(name)?;
    let named = match group_args {
        [] => None,
        args => Some(named_groups(args, false)?),
    };
    let config = Config::load(config_path)?;
    let home = home()?;
    let dir = existing_account(&name, &config, &home)?;
    let current = config.shares(&name);
    // What it will share, and the groups whose links to make.
    let (wanted, to_link) = match named {
        Some(groups) => (
            share::normalize([current.clone(), groups.clone()].concat()),
            groups,
        ),
        None if can_ask() => {
            let picked = pick_groups(&name, &home, &current)?
                .ok_or_else(|| anyhow!("cancelled, so nothing changed"))?;
            (picked.clone(), picked)
        }
        None => return Err(Error::ShareNeedsGroups { name }.into()),
    };
    let dropped: Vec<Group> = current
        .iter()
        .copied()
        .filter(|group| !wanted.contains(group))
        .collect();

    let own: Vec<share::Entry> = share::states(&home, &dir, &to_link)
        .into_iter()
        .filter_map(|(_, entry, state)| matches!(state, State::Own(_)).then_some(entry))
        .collect();
    let today = date::today().to_string();
    if !own.is_empty() && !yes {
        let shown: Vec<String> = own.iter().map(|entry| entry.shown()).collect();
        let entries = share::and_list(&shown.iter().map(String::as_str).collect::<Vec<_>>());
        if !can_ask() {
            return Err(Error::ShareUnconfirmed { name, entries }.into());
        }
        let (it, its) = match own.len() {
            1 => ("it", "its name"),
            _ => ("them", "their names"),
        };
        tell(format_args!(
            "{name} has its own {entries}, so sharing moves {it} aside: it adds .before-share-{today} to {its}, in {name}'s dir. Nothing is deleted."
        ));
        ensure!(
            ask_yes(&format!("Move {it} aside and share?"))?,
            "nothing changed"
        );
    }
    let mut moved = Vec::new();
    for entry in own {
        let to = share::move_aside(&dir, entry, &today)
            .with_context(|| format!("cannot move {name}'s {} aside", entry.name))?;
        moved.push((entry, to));
    }
    let linked = link_groups(&home, &dir, &to_link)?.linked;
    let mut unshared = Vec::new();
    for &group in &dropped {
        unshared.extend(unlink_group(&name, &home, &dir, group, false)?);
    }
    if wanted != current {
        config::set_share(config_path, &name, &wanted)?;
    }

    let added: Vec<Group> = wanted
        .iter()
        .copied()
        .filter(|group| !current.contains(group))
        .collect();
    if !added.is_empty() {
        say(format_args!(
            "{name} now shares {} with ~/.claude",
            share::describe(&added)
        ));
    } else if dropped.is_empty() && moved.is_empty() && linked.is_empty() {
        match to_link.as_slice() {
            [] => say(format_args!("{name} shares nothing, as before")),
            _ => say(format_args!(
                "{name} already shares {}",
                share::describe(&to_link)
            )),
        }
    } else if !linked.is_empty() {
        let names: Vec<&str> = linked.iter().map(|entry| entry.name).collect();
        say(format_args!(
            "{name} shares {} again: linked {}",
            share::describe(&to_link),
            names.join(", ")
        ));
    }
    for (entry, to) in &moved {
        say(format_args!(
            "moved {name}'s own {} aside, to {}",
            entry.shown(),
            to.display()
        ));
    }
    if !moved.is_empty() {
        note(
            "nothing was deleted: copy what you want to keep from these into ~/.claude, then delete them",
        );
    }
    for line in unshared {
        say(line);
    }
    Ok(())
}

/// `cam unshare`: removes the groups' links from the account's dir, and
/// nothing else, then records that it no longer shares them. With `copy`,
/// each link is replaced with a copy of what it pointed to.
pub fn unshare(name: &str, group_args: &[String], copy: bool, config_path: &Path) -> Result<()> {
    let name = account_name(name)?;
    let groups = named_groups(group_args, false)?;
    let config = Config::load(config_path)?;
    known_account(&name, &config)?;
    let current = config.shares(&name);
    let (shared, not_shared): (Vec<Group>, Vec<Group>) =
        groups.iter().partition(|group| current.contains(group));
    if shared.is_empty() {
        return Err(Error::NotShared {
            groups: share::describe(&groups),
            shares: share::describe(&current),
            name,
        }
        .into());
    }
    let home = home()?;
    let dir = account::dir(&home, &name);
    let has_dir = dir.is_dir();
    if copy && has_dir && shared.contains(&Group::History) {
        let bytes: u64 = Group::History
            .entries()
            .iter()
            .filter(|&&entry| share::state(&home, &dir, entry) == State::Linked)
            .map(|&entry| share::size(&share::shared_path(&home, entry)))
            .sum();
        tell(format_args!(
            "Copying {} of history into {}...",
            share::human_size(bytes),
            dir.display()
        ));
    }
    let mut lines = Vec::new();
    for &group in &shared {
        match has_dir {
            true => lines.extend(unlink_group(&name, &home, &dir, group, copy)?),
            false => lines.push(format!(
                "{name} no longer shares {group} (it has no dir, so there were no links)"
            )),
        }
    }
    let left: Vec<Group> = current
        .iter()
        .copied()
        .filter(|group| !shared.contains(group))
        .collect();
    config::set_share(config_path, &name, &left)?;
    for line in lines {
        say(line);
    }
    for group in not_shared {
        note(format_args!("{name} didn't share {group}"));
    }
    Ok(())
}

/// Unlinks every entry of `group` in account `dir`, with a copy in each
/// link's place if `copy`, and returns the lines that say what happened.
fn unlink_group(
    name: &str,
    home: &Path,
    dir: &Path,
    group: Group,
    copy: bool,
) -> Result<Vec<String>> {
    let (mut done, mut skipped) = (Vec::new(), Vec::new());
    for &entry in group.entries() {
        let unlinked = share::unlink(home, dir, entry, copy)
            .with_context(|| format!("cannot unlink {name}'s {}", entry.name))?;
        match unlinked {
            Unlinked::Removed | Unlinked::Copied => done.push(entry.name),
            Unlinked::Skipped(State::Own(own)) => skipped.push(format!(
                "skipped {name}'s {}: it's {} of its own, not a link",
                entry.name,
                own.describe()
            )),
            Unlinked::Skipped(_) => {}
        }
    }
    let mut lines = vec![match (done.is_empty(), copy) {
        (true, _) => format!("{name} no longer shares {group}: it had no links to remove"),
        (false, false) => format!(
            "{name} no longer shares {group}: removed the links {}. Your files stay in ~/.claude, and {name} starts over for these.",
            done.join(", ")
        ),
        (false, true) => format!(
            "{name} no longer shares {group}: replaced the links {} with copies of what they pointed to. ~/.claude is unchanged, and {name} goes its own way from here.",
            done.join(", ")
        ),
    }];
    lines.extend(skipped);
    Ok(lines)
}

/// Fails unless `name` is configured, with a hint that names a close
/// account if it's a typo.
fn known_account(name: &str, config: &Config) -> Result<(), Error> {
    if config.has_account(name) {
        return Ok(());
    }
    let known = config.accounts().keys().map(String::as_str);
    Err(Error::UnknownAccountArg {
        name: name.to_owned(),
        hint: unknown_hint(name, known, Named::Arg),
    })
}

/// `cam remove`: asks first when there's an account dir to lose, unless
/// `yes`. Then logs the account out, and stops if that fails, so its login
/// is never orphaned. Then deletes its dir without following links, then
/// its config entry.
pub fn remove(name: &str, yes: bool, config_path: &Path) -> Result<()> {
    let name = account_name(name)?;
    // A broken config fails here, before anything is deleted.
    let config = Config::load(config_path)?;
    let configured = config.has_account(&name);
    let dir = account::dir(&home()?, &name);
    let has_dir = account::exists(&dir);
    if !configured && !has_dir {
        let hint = match account::closest(&name, config.accounts().keys().map(String::as_str)) {
            Some(close) => format!("did you mean `{close}`?"),
            None => "`cam list` shows the accounts".to_owned(),
        };
        return Err(Error::NothingToRemove { name, hint }.into());
    }
    if has_dir && !yes {
        confirm_remove(&name, &dir, &config.shares(&name))?;
    }
    if has_dir {
        let logged_out = depth().map_err(anyhow::Error::from).and_then(|depth| {
            let program = real_claude(&own_exe()?)?;
            let target = Target::Account {
                name: &name,
                dir: &dir,
            };
            auth::logout(
                claude(&program, &["auth", "logout"], target, depth),
                &program,
            )?;
            Ok(())
        });
        logged_out.with_context(|| format!("cannot log {name} out, so nothing was removed"))?;
        fs::remove_dir_all(&dir)
            .with_context(|| format!("logged {name} out, but cannot delete {}", dir.display()))?;
    }
    if configured {
        config::remove_account(config_path, &name)?;
    }
    let shown = dir.display();
    match (has_dir, configured) {
        (true, true) => say(format_args!(
            "removed {name}: logged out, deleted {shown} and its config entry"
        )),
        (true, false) => say(format_args!(
            "removed {name}: logged out and deleted {shown} (it had no config entry)"
        )),
        _ => say(format_args!(
            "removed {name}: its config entry (it had no account dir)"
        )),
    }
    let here = current_dir()
        .ok()
        .and_then(|cwd| marker::find(&cwd).ok().flatten());
    match here {
        Some((path, marked)) if marked == name => note(format_args!(
            "{} names {name}, so `claude` stops with an error under it until you change it with `cam use`",
            path.display()
        )),
        _ => note(format_args!(
            "a .claude-account that names {name} now makes `claude` stop with an error; change it with `cam use`"
        )),
    }
    Ok(())
}

/// Asks on the terminal before deleting an account dir. Without a
/// terminal, fails: `--yes` is how a script says yes.
fn confirm_remove(name: &str, dir: &Path, shares: &[Group]) -> Result<()> {
    if !can_ask() {
        return Err(Error::Unconfirmed {
            name: name.to_owned(),
        }
        .into());
    }
    let what = match shares {
        [] => "with its transcripts, memory, settings and MCP servers".to_owned(),
        _ => format!(
            "with all it doesn't share, such as its MCP servers. What it shares ({}) stays in ~/.claude",
            share::describe(shares)
        ),
    };
    tell(format_args!(
        "This logs {name} out and deletes {}, {what}.",
        dir.display()
    ));
    ensure!(ask_yes(&format!("Remove {name}?"))?, "nothing was removed");
    Ok(())
}

/// `cam list`: reads the config and each `.claude.json`, and never runs
/// `claude`.
pub fn list(config_path: &Path) -> Result<()> {
    let config = Config::load(config_path)?;
    let home = home()?;
    let emails = recorded_emails(&config, &home);
    let email = |name: &str| {
        emails
            .iter()
            .find(|(other, _)| other == name)
            .map(|(_, email)| email.clone())
    };
    let accounts: Vec<(String, Option<String>, Vec<Group>)> = config
        .accounts()
        .keys()
        .map(|name| (name.clone(), email(name), config.shares(name)))
        .collect();
    for line in list_lines(email(SYSTEM), &accounts) {
        say(line);
    }
    Ok(())
}

/// The rows of `cam list`. The SHARES column shows only once an account
/// shares something.
fn list_lines(
    system: Option<String>,
    accounts: &[(String, Option<String>, Vec<Group>)],
) -> Vec<String> {
    let dash = || "-".to_owned();
    let mut rows = vec![(
        "ACCOUNT".to_owned(),
        "EMAIL".to_owned(),
        "SHARES".to_owned(),
    )];
    rows.push((SYSTEM.to_owned(), system.unwrap_or_else(dash), dash()));
    for (name, email, shares) in accounts {
        let shares: Vec<&str> = shares.iter().map(|group| group.name()).collect();
        rows.push((
            name.clone(),
            email.clone().unwrap_or_else(dash),
            match shares.as_slice() {
                [] => dash(),
                shares => shares.join(", "),
            },
        ));
    }
    let sharing = accounts.iter().any(|(_, _, shares)| !shares.is_empty());
    let width = |column: fn(&(String, String, String)) -> &String| {
        rows.iter()
            .map(|row| column(row).chars().count())
            .max()
            .unwrap_or(0)
    };
    let (name_width, email_width) = (width(|row| &row.0), width(|row| &row.1));
    let mut lines: Vec<String> = rows
        .iter()
        .map(|(name, email, shares)| match sharing {
            true => format!("{name:<name_width$}  {email:<email_width$}  {shares}"),
            false => format!("{name:<name_width$}  {email}"),
        })
        .collect();
    if accounts.is_empty() {
        lines.push(String::new());
        lines.push("no accounts yet; add one with `cam add <name>`".to_owned());
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

/// `cam whoami`: the account for this folder, and who Claude Code says it
/// is logged in as. Asks in the environment `claude` would get, so what
/// outranks the login shows up as what Claude Code would really use.
/// Fails if the account isn't logged in, or its login has expired.
pub fn whoami(config_path: &Path) -> Result<()> {
    let env_account = resolve::env_account()?;
    let resolution = resolve::resolve(env_account.as_deref(), &current_dir()?, config_path)?;
    say(&resolution);
    let depth = depth()?;
    let dir;
    let target = match &resolution.selection {
        Selection::System => Target::System,
        Selection::Account(name) => {
            dir = account::dir(&home()?, name);
            if !dir.is_dir() {
                return Err(Error::NoAccountDir {
                    name: name.clone(),
                    dir,
                }
                .into());
            }
            Target::Account { name, dir: &dir }
        }
    };
    let program = real_claude(&own_exe()?)?;
    let command = claude(&program, &["auth", "status", "--json"], target, depth);
    let status = auth::status(command, &program)?;
    let name = resolution.selection.to_string();
    let today = date::today();
    let mut failed = false;
    // Printed after the report, so they don't land in the middle of it.
    let mut warnings = Vec::new();
    if status.is_login() {
        say(format_args!(
            "  email:   {}",
            status.email.as_deref().unwrap_or("unknown")
        ));
        match (&status.org_name, &status.subscription_type) {
            (Some(org), Some(plan)) => say(format_args!("  org:     {org} ({plan})")),
            (Some(org), None) => say(format_args!("  org:     {org}")),
            (None, Some(plan)) => say(format_args!("  plan:    {plan}")),
            (None, None) => {}
        }
        let expiry = auth::expiry(&status, today);
        if let Some(expires) = expiry.describe(today) {
            say(format_args!("  login:   {expires}"));
        }
        match expiry {
            Expiry::Soon(_) => warnings.push(format!(
                "the login of \"{name}\" {}\nhint: run `cam login {name}` to renew it",
                expiry.describe(today).unwrap_or_default()
            )),
            Expiry::Expired(_) => failed = true,
            Expiry::Good(_) | Expiry::Unknown => {}
        }
    } else if let Some(method) = status.other_method() {
        say(format_args!(
            "  login:   {method}, which Claude Code uses instead of a login"
        ));
    } else {
        say("  login:   not logged in");
        failed = true;
    }
    if let Selection::Account(account) = &resolution.selection {
        let shares = Config::load(config_path)
            .map(|config| config.shares(account))
            .unwrap_or_default();
        if !shares.is_empty() {
            say(format_args!("  shares:  {}", share::describe(&shares)));
        }
    }
    if let Some(active) = env_set(ENV_ACTIVE) {
        say(format_args!(
            "  session: this shell is inside a `claude` that cam started as {}",
            active.to_string_lossy()
        ));
    }
    for conflict in conflict::in_env(|key| env::var_os(key)) {
        warnings.push(format!(
            "{} is set, so {}",
            conflict.name(),
            conflict.effect()
        ));
    }
    for warning in warnings {
        warn(warning);
    }
    ensure!(!failed, Error::NotLoggedIn { name });
    Ok(())
}

/// `cam doctor`: checks cam's setup and what could keep `claude` from using
/// the account cam picks. Asks Claude Code about each login, and never
/// reads a credential. Fails if any check fails; warnings alone don't.
pub fn doctor(config_path: &Path) -> Result<()> {
    let cwd = current_dir()?;
    let home = platform::home_dir();
    let own_exe = own_exe()?;
    let found = real_claude(&own_exe);
    let program = found.as_ref().ok().cloned();
    let mut checks = vec![doctor::claude(found)];

    let path = env::var_os("PATH");
    let shim_dir = shim::default_dir(home.clone());
    let installed = shim_dir
        .as_deref()
        .filter(|dir| shim::is_shim(&dir.join(shim::NAME), &own_exe));
    checks.push(doctor::path(
        &shim::first_on_path(path.as_deref(), &own_exe),
        installed,
        Shell::from_env(env::var_os("SHELL").as_deref()),
    ));

    let exists = config_path.try_exists().unwrap_or(true);
    let loaded = Config::load(config_path);
    checks.push(doctor::config(config_path, exists, &loaded));

    // A depth that is already too deep only matters to `exec`.
    let depth = depth().unwrap_or(1);
    let ask = |target: Target<'_>| match &program {
        Some(program) => Login::Checked(login_status(program, target, depth)),
        None => Login::NoClaude,
    };
    let today = date::today();
    let mut emails = Vec::new();
    let mut note_email = |name: &str, login: &Login| {
        if let Login::Checked(Ok(status)) = login
            && status.is_login()
            && let Some(email) = &status.email
        {
            emails.push((name.to_owned(), email.clone(), status.org_name.clone()));
        }
    };
    let system = ask(Target::System);
    note_email(SYSTEM, &system);
    checks.push(doctor::system(&system, today));
    if let (Ok(config), Some(home)) = (&loaded, &home) {
        for name in config.accounts().keys() {
            let dir = account::dir(home, name);
            let dir_exists = dir.is_dir();
            let login = if dir_exists {
                ask(Target::Account { name, dir: &dir })
            } else {
                Login::NoClaude
            };
            note_email(name, &login);
            checks.push(doctor::account(name, &dir, dir_exists, &login, today));
            if dir_exists {
                let states = share::states(home, &dir, &config.shares(name));
                checks.extend(doctor::links(name, &states, &share::set_aside(&dir)));
            }
        }
    }
    checks.extend(doctor::same_account(&emails));

    let resolved = resolve::env_account()
        .and_then(|account| resolve::resolve(account.as_deref(), &cwd, config_path));
    // The user settings Claude Code would read here: the account's own,
    // or `system`'s.
    let config_dir = match (&resolved, &home) {
        (Ok(resolution), Some(home)) => match &resolution.selection {
            Selection::Account(name) => Some(account::dir(home, name)),
            Selection::System => user_config_dir(),
        },
        _ => user_config_dir(),
    };
    checks.push(doctor::here(resolved));
    checks.extend(doctor::environment(|key| env::var_os(key)));
    let inspected: Vec<_> = settings::files(
        config_dir.as_deref().map(Path::as_os_str),
        home.as_deref(),
        &cwd,
    )
    .into_iter()
    .map(|file| {
        let inspection = settings::inspect(&file);
        (file, inspection)
    })
    .collect();
    checks.extend(doctor::settings(&inspected));

    for check in &checks {
        for line in doctor::render(check) {
            say(line);
        }
    }
    say("");
    let tally = doctor::tally(&checks);
    ensure!(tally.problems == 0, "{tally}");
    say(tally);
    Ok(())
}

/// `cam exec`: resolves the account, then runs the real `claude` in cam's
/// place (see [`platform::run_instead`]). Returns only on failure.
pub fn exec(args: &[OsString], config_path: &Path) -> Result<Infallible> {
    // First, so a loop stops before doing any work.
    let depth = depth()?;
    let env_account = resolve::env_account()?;
    let resolution = resolve::resolve(env_account.as_deref(), &current_dir()?, config_path)?;

    let program = real_claude(&own_exe()?)?;

    let dir;
    let target = match &resolution.selection {
        Selection::System => Target::System,
        Selection::Account(name) => {
            dir = account::dir(&home()?, name);
            if !dir.is_dir() {
                return Err(Error::NoAccountDir {
                    name: name.clone(),
                    dir,
                }
                .into());
            }
            let shares = Config::load(config_path)
                .map(|config| config.shares(name))
                .unwrap_or_default();
            top_up_links(name, &home()?, &dir, &shares);
            warn_outranking(name, print_mode(args));
            Target::Account { name, dir: &dir }
        }
    };
    let source = platform::run_instead(claude(&program, args, target, depth));
    Err(Error::Exec {
        path: program,
        source,
    }
    .into())
}

/// The real `claude`, found the way `exec` finds it.
fn real_claude(own_exe: &Path) -> Result<PathBuf, Error> {
    let shim_dir = shim::default_dir(platform::home_dir());
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
        platform::canonicalize(&exe).map_err(Error::CurrentExe)
    }
}

/// `cam init`: keeps the `claude` → `cam` link in the shim dir, then
/// prints the code that puts that dir first on `PATH` and loads
/// `completions`. It runs at every shell start, so a problem is a warning
/// on stderr, and the rest of the code still loads.
pub fn init(shell: Shell, completions: &str) -> Result<()> {
    let warn = |message: &dyn Display| {
        eprintln!("cam: warning: {}", redact(&message.to_string()));
    };
    let path = env::var_os("PATH");
    let mut code = String::new();
    match (shim::default_dir(platform::home_dir()), own_exe()) {
        (None, _) => warn(&"cannot find the home directory, so plain `claude` skips cam; set HOME"),
        (_, Err(err)) => warn(&err),
        (Some(dir), Ok(own_exe)) => {
            let target = shim::target(&own_exe, path.as_deref());
            match (shim::install(&dir, &target, &own_exe), dir.to_str()) {
                (Ok(_), Some(dir)) => code.push_str(&init::path_code(shell, dir, path.as_deref())),
                (Ok(_), None) => warn(&format_args!(
                    "{} isn't valid UTF-8, so cam can't put it on PATH",
                    dir.display()
                )),
                (Err(err), _) => warn(&err),
            }
        }
    }
    code.push_str(&init::completions_code(shell, completions));
    print!("{}", redact(&code));
    Ok(())
}

/// `cam use`: writes the marker, and says what it replaced.
pub fn use_account(name: &str, dir: Option<&Path>, config_path: &Path) -> Result<()> {
    let selection = Selection::parse(name).ok_or_else(|| Error::InvalidNameArg {
        name: name.to_owned(),
    })?;
    if let Selection::Account(name) = &selection {
        known_account(name, &Config::load(config_path)?)?;
    }
    let dir = target_dir(dir)?;
    // An unreadable marker is just replaced.
    let previous = marker::read(&dir.join(FILE_NAME)).ok().flatten();
    let path = marker::write(&dir, &selection)
        .with_context(|| format!("cannot write {}", dir.join(FILE_NAME).display()))?;
    let shown = path.display();
    match previous {
        Some(previous) if previous != selection.to_string() => {
            say(format_args!("wrote {shown} ({selection}, was {previous})"));
        }
        Some(_) => say(format_args!("wrote {shown} ({selection}, as before)")),
        None => say(format_args!("wrote {shown} ({selection})")),
    }
    if dir.ancestors().any(|dir| dir.join(".git").exists()) {
        note(format_args!(
            "{} is in a git repository; to keep {FILE_NAME} out of commits, list it in your global gitignore (~/.config/git/ignore by default)",
            dir.display()
        ));
    }
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

/// `dir`, or the current directory, made absolute. Must be an existing
/// directory. A leading `~` is the home directory, since a shell leaves it
/// alone in `--dir=~/Work`.
fn target_dir(dir: Option<&Path>) -> Result<PathBuf> {
    let dir = match dir {
        Some(dir) => expand_home(dir, platform::home_dir().as_deref()),
        None => current_dir()?,
    };
    let canonical =
        platform::canonicalize(&dir).with_context(|| format!("cannot open {}", dir.display()))?;
    ensure!(canonical.is_dir(), "{} is not a directory", dir.display());
    Ok(canonical)
}

/// `path` with a leading `~` or `~/` replaced by `home`.
fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts(list: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>, Vec<Group>)> {
        list.iter()
            .map(|(name, email)| ((*name).to_owned(), email.map(str::to_owned), Vec::new()))
            .collect()
    }

    #[test]
    fn list_without_accounts_shows_system() {
        assert_eq!(
            list_lines(Some("me@home.org".to_owned()), &[]),
            [
                "ACCOUNT  EMAIL",
                "system   me@home.org",
                "",
                "no accounts yet; add one with `cam add <name>`",
            ]
        );
    }

    #[test]
    fn list_aligns_columns() {
        assert_eq!(
            list_lines(
                None,
                &accounts(&[
                    ("personal", Some("me@home.org")),
                    ("work", Some("you@company.com")),
                    ("x", None),
                ])
            ),
            [
                "ACCOUNT   EMAIL",
                "system    -",
                "personal  me@home.org",
                "work      you@company.com",
                "x         -",
            ]
        );
    }

    #[test]
    fn list_shows_shares_once_an_account_shares_something() {
        let mut list = accounts(&[("personal", Some("me@home.org")), ("work", None)]);
        list[1].2 = vec![Group::Settings, Group::Skills];
        assert_eq!(
            list_lines(None, &list),
            [
                "ACCOUNT   EMAIL        SHARES",
                "system    -            -",
                "personal  me@home.org  -",
                "work      -            settings, skills",
            ]
        );
    }

    #[test]
    fn next_steps_offer_sharing_until_the_account_shares() {
        let none = next_steps("work", true, None, &[]).join("\n");
        assert!(
            none.contains(
                "  cam share work  share your settings, CLAUDE.md, skills or history with it"
            ),
            "{none}"
        );
        assert!(
            none.contains("without your settings, CLAUDE.md, skills or MCP servers"),
            "{none}"
        );
        let some = next_steps("work", true, None, &[Group::Settings, Group::Skills]).join("\n");
        assert!(!some.contains("cam share"), "{some}");
        assert!(some.contains("Apart from settings and skills, work starts with an empty Claude Code setup of its own"), "{some}");
    }

    #[test]
    fn account_names_exclude_system() {
        assert_eq!(account_name("work").unwrap(), "work");
        assert!(matches!(account_name("system"), Err(Error::ReservedName)));
        assert!(matches!(
            account_name("Work"),
            Err(Error::InvalidNameArg { .. })
        ));
    }
}
