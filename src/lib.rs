//! cam: pick the Claude Code account per folder via `.claude-account` files.
//! See DESIGN.md for the spec. The `cam` binary only calls [`run`].

mod account;
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
mod store;
#[cfg(test)]
mod testutil;
mod token;

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
    /// Store a token from `claude setup-token` and register the account.
    ///
    /// Prompts for the token with hidden input. If stdin isn't a terminal,
    /// the token is read from stdin instead.
    Add {
        /// Account name, matching ^[a-z0-9][a-z0-9_-]*$.
        name: String,
        /// Replace a token that is already stored.
        #[arg(long)]
        force: bool,
    },
    /// Delete an account's token and config entry.
    Remove {
        /// The account to remove.
        name: String,
    },
    /// List accounts, their token age and the default.
    List,
    /// Pin an account to a folder by writing .claude-account.
    Use {
        /// A configured account, or `none` to force pass-through from this folder down.
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
    /// Show the account that applies here and its token.
    ///
    /// Checks that the token is stored without reading it, so it never
    /// causes a Keychain prompt.
    Whoami,
    /// Check cam's setup, and anything that would keep `claude` from using
    /// the account cam picks.
    ///
    /// Exits non-zero if a check fails. Never reads a token.
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
        Command::Add { name, force } => {
            let store = store::open()?;
            commands::add(&name, force, &config_path()?, store.as_ref(), token::read)
        }
        Command::Remove { name } => {
            let store = store::open()?;
            commands::remove(&name, &config_path()?, store.as_ref())
        }
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
