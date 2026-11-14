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

use crate::account::NONE;
use crate::config::Config;
use crate::redact::redact;

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

/// `cmd` with account names as the values of `use` and `remove`. Only for
/// generating completions: parsing must still accept new names, as `add`
/// does.
fn with_accounts(mut cmd: Command, config: Option<&Config>) -> Command {
    let accounts: Vec<PossibleValue> = config
        .map(|config| {
            config
                .accounts()
                .iter()
                .map(|(name, account)| {
                    let value = PossibleValue::new(name.clone());
                    match account.email.as_deref().filter(|email| is_plain(email)) {
                        Some(email) => value.help(email.to_owned()),
                        None => value,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut with_none = accounts.clone();
    with_none.push(PossibleValue::new(NONE).help("pass-through: Claude Code's own login"));
    // Edited in place: `Command::mut_subcommand` would move the subcommand
    // to the end of the list, and bash shows them in list order.
    for sub in cmd.get_subcommands_mut() {
        let values = match sub.get_name() {
            "use" => with_none.clone(),
            "remove" if !accounts.is_empty() => accounts.clone(),
            _ => continue,
        };
        let taken = std::mem::replace(sub, Command::new(BIN));
        *sub = taken.mut_arg("name", |arg| {
            arg.value_parser(PossibleValuesParser::new(values))
        });
    }
    cmd
}

/// Whether an email is safe to show as help text in any shell's script.
/// clap escapes help text, but the config is hand-editable, so anything
/// unusual is left out rather than trusted to escaping.
fn is_plain(email: &str) -> bool {
    email
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "@._%+-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    fn cli() -> Command {
        Command::new(BIN)
            .subcommand(Command::new("use").arg(clap::Arg::new("name")))
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
    fn fills_in_account_names_and_plain_emails() {
        let tree = Tree::new();
        let path = tree.config(
            "[accounts.work]\nemail = \"you@company.com\"\n\
             [accounts.odd]\nemail = \"a$(b)@c\"\n\
             [accounts.personal]\n",
        );
        let config = Config::load(&path).unwrap();
        let cmd = with_accounts(cli(), Some(&config));
        let accounts = [
            ("odd", None),
            ("personal", None),
            ("work", Some("you@company.com")),
        ];
        assert_eq!(values(&cmd, "remove"), owned(&accounts));
        let mut with_none = accounts.to_vec();
        with_none.push(("none", Some("pass-through: Claude Code's own login")));
        assert_eq!(values(&cmd, "use"), owned(&with_none));
        // The subcommands keep their order.
        let names: Vec<&str> = cmd.get_subcommands().map(Command::get_name).collect();
        assert_eq!(names, ["use", "remove"]);
    }

    #[test]
    fn without_accounts_only_use_gets_none() {
        let cmd = with_accounts(cli(), None);
        assert_eq!(
            values(&cmd, "use"),
            owned(&[("none", Some("pass-through: Claude Code's own login"))])
        );
        assert_eq!(values(&cmd, "remove"), []);
    }

    #[test]
    fn fish_gets_positional_values_quoted() {
        let cmd = Command::new(BIN).subcommand(Command::new("use").arg(
            clap::Arg::new("name").value_parser(PossibleValuesParser::new([
                PossibleValue::new("work").help("you@company.com"),
                PossibleValue::new("none").help("Claude Code's own"),
                PossibleValue::new("plain"),
            ])),
        ));
        assert_eq!(
            fish_values(&cmd),
            "complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'work' -d 'you@company.com'\n\
             complete -c cam -n \"__fish_cam_using_subcommand use\" -f -a 'none' -d 'Claude Code\\'s own'\n\
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
