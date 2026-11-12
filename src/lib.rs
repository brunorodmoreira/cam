//! cam: pick the Claude Code account per folder via `.claude-account` files.
//! See DESIGN.md for the spec. The `cam` binary only calls [`run`].

mod account;
mod commands;
mod config;
mod date;
mod discover;
mod error;
mod launch;
mod marker;
mod redact;
mod resolve;
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

use clap::{Parser, Subcommand};

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
        /// The email this token is expected to belong to.
        #[arg(long, value_name = "EMAIL")]
        email: Option<String>,
        /// Replace a token that is already stored.
        #[arg(long)]
        force: bool,
    },
    /// Delete an account's token and config entry.
    Remove {
        /// The account to remove.
        name: String,
    },
    /// List accounts, their expected email, token age and the default.
    List,
    /// Pin an account to a folder by writing .claude-account.
    Use {
        /// A configured account, or `none` to force pass-through from this folder down.
        name: String,
        /// Write the marker in DIR instead of the current directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// Remove the .claude-account marker from a folder.
    Unuse {
        /// Remove the marker in DIR instead of the current directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// Show which account applies here, and why.
    Which,
    /// Run the real `claude` with the account that applies here.
    ///
    /// Arguments after `exec` go to claude unchanged. Put them after `--` when
    /// one could be read as cam's own, e.g. `cam exec -- --help`.
    Exec {
        /// Arguments for claude.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },
}

/// Runs the CLI with the process's arguments and environment.
pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // Usage errors can echo arguments back, so redact them too.
        Err(err) if err.use_stderr() => {
            eprint!("{}", redact(&err.render().to_string()));
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
        Err(err) => err.exit(),
    };
    match dispatch(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cam: {}", redact(&format!("{err:#}")));
            ExitCode::from(err.downcast_ref::<Error>().map_or(1, Error::exit_code))
        }
    }
}

fn dispatch(command: Command) -> anyhow::Result<()> {
    let config_path = || config::path(env::var_os(config::ENV_CONFIG), dirs::home_dir());
    match command {
        Command::Add { name, email, force } => {
            let store = store::open()?;
            commands::add(
                &name,
                email.as_deref(),
                force,
                &config_path()?,
                store.as_ref(),
                token::read,
            )
        }
        Command::Remove { name } => {
            let store = store::open()?;
            commands::remove(&name, &config_path()?, store.as_ref())
        }
        Command::List => commands::list(&config_path()?),
        Command::Use { name, dir } => commands::use_account(&name, dir.as_deref(), &config_path()?),
        Command::Unuse { dir } => commands::unuse(dir.as_deref()),
        Command::Which => commands::which(&config_path()?),
        Command::Exec { args } => match commands::exec(&args, &config_path()?)? {},
    }
}
