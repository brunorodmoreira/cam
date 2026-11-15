//! cam: pick the Claude Code account per folder via `.claude-account` files.
//! See DESIGN.md for the spec. The `cam` binary only calls [`run`].

mod account;
mod auth;
mod commands;
mod completions;
mod config;
mod conflict;
mod date;
mod discover;
mod doctor;
mod error;
mod launch;
mod marker;
mod redact;
mod resolve;
mod settings;
mod shim;
#[cfg(test)]
mod testutil;

#[cfg(not(unix))]
compile_error!("cam supports Unix only (macOS first); see DESIGN.md");

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand, ValueHint};

use crate::error::Error;
use crate::redact::redact;

#[derive(Parser)]
#[command(name = "cam", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create an account and log it in with Claude Code's own login.
    ///
    /// The account gets its own Claude Code config dir, under
    /// ~/.local/share/cam/accounts. cam runs `claude auth login` in it, so
    /// you pick the account in the browser, as with `/login`.
    Add {
        /// Account name, matching ^[a-z0-9][a-z0-9_-]*$.
        name: String,
    },
    /// Log an account in again with Claude Code's own login.
    ///
    /// Without a name, logs in the account that applies here. `system` is
    /// Claude Code's own setup.
    Login {
        /// The account, or `system`. Defaults to the one that applies here.
        name: Option<String>,
    },
    /// Log an account out, then delete its dir and config entry.
    Remove {
        /// The account to remove.
        name: String,
    },
    /// List `system` and the accounts, with the email each is logged in as.
    List,
    /// Pin an account to a folder by writing .claude-account.
    Use {
        /// A configured account, or `system` for Claude Code's own setup from this folder down.
        name: String,
        /// Write the marker in DIR instead of the current directory.
        #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Remove the .claude-account marker from a folder.
    Unuse {
        /// Remove the marker in DIR instead of the current directory.
        #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Show which account applies here, and why.
    Which,
    /// Show the account that applies here, and who it's logged in as.
    ///
    /// Asks `claude auth status`, so it reports what Claude Code would use.
    Whoami,
    /// Check cam's setup, each account's login, and anything that would
    /// keep `claude` from using the account cam picks.
    ///
    /// Exits non-zero if a check fails. Never reads a credential.
    Doctor,
    /// Run the real `claude` with the account that applies here.
    ///
    /// Arguments after `exec` go to claude unchanged. Put them after `--` when
    /// one could be read as cam's own, e.g. `cam exec -- --help`. The shim
    /// (`cam shim install`) does this for plain `claude`.
    Exec {
        /// Arguments for claude.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },
    /// Manage the shim that sends plain `claude` through cam.
    #[command(subcommand)]
    Shim(ShimCommand),
    /// Print a shell completion script.
    ///
    /// Account names are filled in when the script is generated, so load it
    /// from your shell's startup file instead of saving it to a file:
    ///
    ///   zsh, in ~/.zshrc (after compinit):   eval "$(cam completions zsh)"
    ///   bash, in ~/.bashrc:                  eval "$(cam completions bash)"
    ///   fish, in ~/.config/fish/config.fish: cam completions fish | source
    #[command(verbatim_doc_comment, about = "Print a shell completion script")]
    Completions {
        /// The shell to write the script for.
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum ShimCommand {
    /// Create a `claude` -> `cam` symlink, and say how to put it first on PATH.
    ///
    /// Never edits shell startup files, and never replaces a `claude` that
    /// isn't a cam shim.
    Install {
        /// Put the shim in DIR instead of ~/.local/share/cam/shims.
        #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Remove the shim. Leaves anything that isn't a cam shim alone.
    Uninstall {
        /// Remove the shim from DIR instead of ~/.local/share/cam/shims.
        #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
}

/// Runs the CLI with the process's arguments and environment. Started as
/// `claude`, through the shim, it's `cam exec` with every argument passed on.
pub fn run() -> ExitCode {
    let mut args = env::args_os();
    if args
        .next()
        .is_some_and(|argv0| shim::invoked_as_shim(&argv0))
    {
        let args: Vec<OsString> = args.collect();
        return report(
            config_path()
                .map_err(Into::into)
                .and_then(|config| match commands::exec(&args, &config)? {}),
        );
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // Usage errors can echo arguments back, so redact them too.
        Err(err) if err.use_stderr() => {
            eprint!("{}", redact(&err.render().to_string()));
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
        Err(err) => err.exit(),
    };
    report(dispatch(cli.command))
}

/// Prints an error, redacted, and picks the exit code.
fn report(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cam: {}", redact(&format!("{err:#}")));
            ExitCode::from(err.downcast_ref::<Error>().map_or(1, Error::exit_code))
        }
    }
}

fn config_path() -> Result<PathBuf, Error> {
    config::path(env::var_os(config::ENV_CONFIG), dirs::home_dir())
}

fn dispatch(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Add { name } => commands::add(&name, &config_path()?),
        Command::Login { name } => commands::login(name.as_deref(), &config_path()?),
        Command::Remove { name } => commands::remove(&name, &config_path()?),
        Command::List => commands::list(&config_path()?),
        Command::Use { name, dir } => commands::use_account(&name, dir.as_deref(), &config_path()?),
        Command::Unuse { dir } => commands::unuse(dir.as_deref()),
        Command::Which => commands::which(&config_path()?),
        Command::Whoami => commands::whoami(&config_path()?),
        Command::Doctor => commands::doctor(&config_path()?),
        Command::Exec { args } => match commands::exec(&args, &config_path()?)? {},
        Command::Shim(ShimCommand::Install { dir }) => commands::shim_install(dir.as_deref()),
        Command::Shim(ShimCommand::Uninstall { dir }) => commands::shim_uninstall(dir.as_deref()),
        Command::Completions { shell } => {
            let config = config_path().ok();
            print!(
                "{}",
                completions::script(shell, Cli::command(), config.as_deref())
            );
            Ok(())
        }
    }
}
