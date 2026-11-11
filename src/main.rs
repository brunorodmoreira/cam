//! cam: pick the Claude Code account per folder via `.claude-account` files.
//! See DESIGN.md for the spec.

mod account;
mod commands;
mod config;
mod error;
mod marker;
mod redact;
mod resolve;
#[cfg(test)]
mod testutil;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::redact::redact;

#[derive(Parser)]
#[command(name = "cam", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show which account applies here, and why.
    Which,
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
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // Usage errors can echo arguments back, so redact them too.
        Err(err) if err.use_stderr() => {
            eprint!("{}", redact(&err.render().to_string()));
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
        Err(err) => err.exit(),
    };
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cam: {}", redact(&format!("{err:#}")));
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> anyhow::Result<()> {
    let config_path = || config::path(env::var_os(config::ENV_CONFIG), dirs::home_dir());
    match command {
        Command::Which => commands::which(&config_path()?),
        Command::Use { name, dir } => commands::use_account(&name, dir.as_deref(), &config_path()?),
        Command::Unuse { dir } => commands::unuse(dir.as_deref()),
    }
}
