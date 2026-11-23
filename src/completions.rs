//! `cam completions <shell>`: clap's static completion scripts, with the
//! configured account names filled in when the script is generated.
//!
//! Static scripts can't look anything up at TAB time, so the names are
//! baked in. Loading the script from the shell's startup file, rather than
//! saving it, keeps them current: an account added later shows up in the
//! next new shell. Nothing runs cam while completing.

use std::path::Path;

use clap::Command;
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap_complete::Shell;

use crate::account::SYSTEM;
use crate::config::Config;
use crate::redact::redact;
use crate::share::Group;

const BIN: &str = "cam";

/// The script for `shell`. A missing or broken config just means no
/// account names: the script runs at every shell start, so it stays quiet.
pub fn script(shell: Shell, cmd: Command, config_path: Option<&Path>) -> String {
    let config = config_path.and_then(|path| Config::load(path).ok());
    let mut cmd = with_accounts(cmd, config.as_ref());
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut cmd, BIN, &mut out);
    let mut script = String::from_utf8_lossy(&out).into_owned();
    if shell == Shell::Fish {
        script.push_str(&fish_values(&cmd));
    }
    redact(&script)
}

/// clap's fish script completes flags and subcommands but no positional
/// values, so this adds them: account names, and shells for `completions`.
/// It uses the `__fish_cam_using_subcommand` function that clap's script
/// defines.
fn fish_values(cmd: &Command) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"));
    let mut lines = String::new();
    for sub in cmd.get_subcommands() {
        for arg in sub.get_positionals() {
            for value in arg
                .get_possible_values()
                .iter()
                .filter(|value| !value.is_hide_set())
            {
                let help = value
                    .get_help()
                    .map(|help| format!(" -d {}", quote(&help.to_string())))
                    .unwrap_or_default();
                lines.push_str(&format!(
                    "complete -c {BIN} -n \"__fish_{BIN}_using_subcommand {}\" -f -a {}{help}\n",
                    sub.get_name(),
                    quote(value.get_name())
                ));
            }
        }
    }
    lines
}

/// `cmd` with account names as the values of `use`, `login`, `remove`,
/// `share` and `unshare`, and group names for sharing. Only for generating
/// completions: parsing must still accept new names, as `add` does, and
/// group lists such as `settings,skills`.
fn with_accounts(mut cmd: Command, config: Option<&Config>) -> Command {
    let accounts: Vec<PossibleValue> = config
        .map(|config| {
            config
                .accounts()
                .keys()
                .map(|name| PossibleValue::new(name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut with_system = accounts.clone();
    with_system.push(PossibleValue::new(SYSTEM).help("Claude Code's own setup"));
    let groups: Vec<PossibleValue> = Group::ALL
        .iter()
        .map(|group| PossibleValue::new(group.name()).help(group.covers()))
        .chain([PossibleValue::new("all").help("every group")])
        .collect();
    let mut groups_or_none = groups.clone();
    groups_or_none.push(PossibleValue::new("none").help("share nothing"));
    let values = |values: Vec<PossibleValue>| PossibleValuesParser::new(values);
    // Edited in place: `Command::mut_subcommand` would move the subcommand
    // to the end of the list, and bash shows them in list order.
    for sub in cmd.get_subcommands_mut() {
        let name = sub.get_name().to_owned();
        let mut taken = std::mem::replace(sub, Command::new(BIN));
        match name.as_str() {
            "use" | "login" => {
                taken = taken.mut_arg("name", |arg| arg.value_parser(values(with_system.clone())));
            }
            "remove" | "share" | "unshare" if !accounts.is_empty() => {
                taken = taken.mut_arg("name", |arg| arg.value_parser(values(accounts.clone())));
            }
            _ => {}
        }
        match name.as_str() {
            "share" | "unshare" => {
                taken = taken.mut_arg("groups", |arg| arg.value_parser(values(groups.clone())));
            }
            "add" => {
                taken = taken.mut_arg("share", |arg| {
                    arg.value_parser(values(groups_or_none.clone()))
                });
            }
            _ => {}
        }
        *sub = taken;
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    fn cli() -> Command {
        Command::new(BIN)
            .subcommand(Command::new("use").arg(clap::Arg::new("name")))
            .subcommand(Command::new("login").arg(clap::Arg::new("name")))
            .subcommand(Command::new("remove").arg(clap::Arg::new("name")))
    }

    fn values(cmd: &Command, sub: &str) -> Vec<(String, Option<String>)> {
        cmd.find_subcommand(sub)
            .unwrap()
            .get_arguments()
            .find(|arg| arg.get_id() == "name")
            .unwrap()
            .get_possible_values()
            .iter()
            .map(|value| {
                (
                    value.get_name().to_owned(),
                    value.get_help().map(ToString::to_string),
                )
            })
            .collect()
    }

    fn owned(pairs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        pairs
            .iter()
            .map(|(name, help)| ((*name).to_owned(), help.map(str::to_owned)))
            .collect()
    }

    #[test]
    fn fills_in_account_names() {
        let tree = Tree::new();
        let path = tree.config("[accounts.work]\n[accounts.personal]\n");
        let config = Config::load(&path).unwrap();
        let cmd = with_accounts(cli(), Some(&config));
        let accounts = [("personal", None), ("work", None)];
        assert_eq!(values(&cmd, "remove"), owned(&accounts));
        let mut with_system = accounts.to_vec();
        with_system.push(("system", Some("Claude Code's own setup")));
        assert_eq!(values(&cmd, "use"), owned(&with_system));
        assert_eq!(values(&cmd, "login"), owned(&with_system));
        // The subcommands keep their order.
        let names: Vec<&str> = cmd.get_subcommands().map(Command::get_name).collect();
        assert_eq!(names, ["use", "login", "remove"]);
    }

    #[test]
    fn without_accounts_use_and_login_get_system() {
        let cmd = with_accounts(cli(), None);
        for sub in ["use", "login"] {
            assert_eq!(
                values(&cmd, sub),
                owned(&[("system", Some("Claude Code's own setup"))])
            );
        }
        assert_eq!(values(&cmd, "remove"), []);
    }

    #[test]
    fn fish_gets_positional_values_quoted() {
        let cmd = Command::new(BIN).subcommand(Command::new("use").arg(
            clap::Arg::new("name").value_parser(PossibleValuesParser::new([
                PossibleValue::new("work").help("some help"),
                PossibleValue::new("system").help("Claude Code's own"),
                PossibleValue::new("plain"),
            ])),
        ));
        assert_eq!(
            fish_values(&cmd),
            "complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'work' -d 'some help'\n\
             complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'system' -d 'Claude Code\\'s own'\n\
             complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'plain'\n"
        );
    }

    #[test]
    fn a_broken_config_still_gives_a_script() {
        let tree = Tree::new();
        let path = tree.config("not toml");
        let script = script(Shell::Bash, cli(), Some(&path));
        assert!(script.contains("complete -F _cam"), "{script}");
    }
}
