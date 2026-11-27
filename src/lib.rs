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
mod herdr;
mod init;
mod launch;
mod marker;
mod platform;
mod redact;
mod resolve;
mod settings;
mod share;
mod shim;
#[cfg(test)]
mod testutil;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueHint};

use crate::error::Error;
use crate::redact::redact;

#[cfg(unix)]
const GETTING_STARTED: &str = "\
Getting started:
  cam add work                log a new account in, with Claude Code's own login
  cam use work --dir ~/Work   use it in ~/Work and every folder below
  eval \"$(cam init zsh)\"      at the end of ~/.zshrc, so plain `claude` picks the account

Where no .claude-account applies, `claude` runs as it always has: that's
`system`, Claude Code's own setup and login.";

#[cfg(windows)]
const GETTING_STARTED: &str = "\
Getting started:
  cam add work                log a new account in, with Claude Code's own login
  cam use work --dir ~\\Work   use it in ~\\Work and every folder below
  Invoke-Expression (& cam init powershell | Out-String)
                              at the end of $PROFILE, so plain `claude` picks the account

Where no .claude-account applies, `claude` runs as it always has: that's
`system`, Claude Code's own setup and login.";

#[derive(Parser)]
#[command(
    name = "cam",
    version,
    about,
    arg_required_else_help = true,
    after_help = GETTING_STARTED
)]
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
    /// you pick the account in the browser, as with `/login`. The account
    /// starts with an empty setup, apart from the parts of your own setup
    /// in ~/.claude it shares: on a terminal, cam asks which, unless
    /// --share says. MCP servers are never shared.
    Add {
        /// Account name: lowercase letters, digits, `-` and `_`, like `work`.
        name: String,
        /// What to share with ~/.claude: settings, instructions, skills,
        /// history, `all` or `none`. Comma-separated or repeated. Without
        /// it, cam asks on a terminal, and shares nothing otherwise.
        #[arg(long, value_name = "GROUPS")]
        share: Vec<String>,
        /// After `--`, arguments for `claude auth login`: `--email <address>`
        /// fills in the email, `--sso` forces single sign-on.
        #[arg(last = true, value_name = "LOGIN_ARGS")]
        login_args: Vec<OsString>,
    },
    /// Log an account in again with Claude Code's own login.
    ///
    /// Without a name, logs in the account that applies here. `system` is
    /// Claude Code's own setup.
    Login {
        /// The account, or `system`. Defaults to the one that applies here.
        name: Option<String>,
        /// After `--`, arguments for `claude auth login`, as for `add`.
        #[arg(last = true, value_name = "LOGIN_ARGS")]
        login_args: Vec<OsString>,
    },
    /// Share parts of your own setup in ~/.claude with an account.
    ///
    /// The groups: settings (settings.json, keybindings.json, plugins/,
    /// hooks/), instructions (CLAUDE.md, rules/), skills (skills/, agents/,
    /// commands/, output-styles/) and history (projects/, history.jsonl,
    /// file-history/, todos/, plans/). A shared group is the same files for
    /// both: a change in either shows up in the other. The account's own
    /// copies are moved aside, never deleted. Without groups, cam shows the
    /// choice on a terminal.
    Share {
        /// The account.
        name: String,
        /// Groups to share: settings, instructions, skills, history, or `all`.
        #[arg(value_name = "GROUP")]
        groups: Vec<String>,
        /// Move the account's own copies aside without asking.
        #[arg(short, long)]
        yes: bool,
    },
    /// Stop sharing parts of ~/.claude with an account.
    ///
    /// Removes the group's links and nothing else: your files stay in
    /// ~/.claude, and the account starts over for these. With --copy, the
    /// account keeps a copy of them instead.
    Unshare {
        /// The account.
        name: String,
        /// Groups to stop sharing, or `all`.
        #[arg(value_name = "GROUP", required = true)]
        groups: Vec<String>,
        /// Replace each link with a copy of what it points to.
        #[arg(long)]
        copy: bool,
    },
    /// Log an account out and delete it, with its transcripts and memory.
    ///
    /// Asks first, unless --yes. Afterwards, a .claude-account that names
    /// the account makes `claude` stop with an error until you change it.
    Remove {
        /// The account to remove.
        name: String,
        /// Don't ask. Needed when there's no terminal to ask on.
        #[arg(short, long)]
        yes: bool,
    },
    /// List `system` and the accounts, with the email each is logged in as.
    List,
    /// Use an account in a folder and every folder below it.
    ///
    /// Writes .claude-account in the folder, replacing one that's there.
    /// The nearest one up the tree decides which account `claude` uses.
    Use {
        /// An account, or `system` for Claude Code's own setup.
        name: String,
        /// The folder, instead of the current one.
        #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Remove the .claude-account marker from a folder.
    Unuse {
        /// The folder, instead of the current one.
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
    /// one could be read as cam's own, e.g. `cam exec -- --help`. With
    /// `cam init` in your shell's startup file, plain `claude` does this.
    Exec {
        /// Arguments for claude.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },
    /// Print the shell code that makes plain `claude` pick the account.
    ///
    /// Add one line to the end of your shell's startup file, after any line
    /// that puts Claude Code's own dir (~/.local/bin) on PATH:
    ///
    ///   zsh, in ~/.zshrc:                     eval "$(cam init zsh)"
    ///   bash, in ~/.bashrc:                   eval "$(cam init bash)"
    ///   fish, in ~/.config/fish/config.fish:  cam init fish | source
    ///
    /// It keeps a `claude` link to cam in ~/.local/share/cam/shims, puts
    /// that dir first on PATH, and loads completions with your account
    /// names. To stop, delete the line.
    #[command(
        verbatim_doc_comment,
        about = "Print the shell code that makes plain `claude` pick the account"
    )]
    Init {
        /// The shell to write the code for.
        shell: init::Shell,
    },
    /// Print a shell completion script.
    ///
    /// `cam init` loads it already. On its own, load it from your shell's
    /// startup file rather than saving it, since account names are filled
    /// in when it's generated:
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

// `cam herdr …` is left out of `Cli`, which completions are made from,
// because clap's scripts list even a hidden command. `parser` adds it.
/// What cam's Herdr plugin runs. See docs/herdr.md.
#[derive(Subcommand)]
enum HerdrCommand {
    /// The plugin's action: opens the accounts popup in the focused pane's
    /// folder.
    Open,
    /// The accounts popup: which account applies here, and a menu to change
    /// it, add one, log one in again or check the setup.
    Accounts,
}

/// What cam parses: [`Cli`], plus `herdr`, hidden.
fn parser() -> clap::Command {
    let herdr = clap::Command::new("herdr")
        .about("What cam's Herdr plugin runs. See docs/herdr.md")
        .hide(true)
        .subcommand_required(true);
    Cli::command().subcommand(HerdrCommand::augment_subcommands(herdr))
}

/// A parsed command line.
enum Parsed {
    Cli(Command),
    Herdr(HerdrCommand),
}

/// Runs the CLI with the process's arguments and environment. Started as
/// `claude`, through the shim, it's `cam exec` with every argument passed on.
pub fn run() -> ExitCode {
    // Before the shim check: the label helper can be started through the
    // shim, which would otherwise run `claude`.
    if let Some(job) = herdr::LabelJob::from_env() {
        job.run();
        return ExitCode::SUCCESS;
    }
    let mut args = env::args_os();
    if args
        .next()
        .is_some_and(|argv0| shim::invoked_as_shim(&argv0))
    {
        let args: Vec<OsString> = args.collect();
        let result = config_path()
            .map_err(Into::into)
            .and_then(|config| match commands::exec(&args, &config)? {});
        return report(result.map_err(escape_hint));
    }
    let parsed = parser()
        .try_get_matches()
        .and_then(|matches| match matches.subcommand() {
            Some(("herdr", herdr)) => HerdrCommand::from_arg_matches(herdr).map(Parsed::Herdr),
            _ => Cli::from_arg_matches(&matches).map(|cli| Parsed::Cli(cli.command)),
        });
    let parsed = match parsed {
        Ok(parsed) => parsed,
        // Usage errors can echo arguments back, so redact them too.
        Err(err) if err.use_stderr() => {
            eprint!("{}", redact(&err.render().to_string()));
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
        Err(err) => err.exit(),
    };
    report(match parsed {
        Parsed::Cli(command) => dispatch(command),
        Parsed::Herdr(HerdrCommand::Open) => commands::herdr_open(),
        Parsed::Herdr(HerdrCommand::Accounts) => config_path()
            .map_err(Into::into)
            .and_then(|config| commands::herdr_accounts(&config)),
    })
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

/// For plain `claude`: when cam fails over the account, adds how to run
/// Claude Code without one.
fn escape_hint(err: anyhow::Error) -> anyhow::Error {
    let about_the_account = matches!(
        err.downcast_ref::<Error>(),
        Some(
            Error::EnvNotUnicode
                | Error::Read { .. }
                | Error::NotUtf8 { .. }
                | Error::ParseConfig { .. }
                | Error::ConfigShape { .. }
                | Error::InvalidConfigAccount { .. }
                | Error::EmptyMarker { .. }
                | Error::InvalidName { .. }
                | Error::UnknownAccount { .. }
                | Error::NoAccountDir { .. }
        )
    );
    let already = env::var(resolve::ENV_ACCOUNT).is_ok_and(|value| value.trim() == account::SYSTEM);
    if about_the_account && !already {
        anyhow::anyhow!(
            "{err:#}\nhint: `CAM_ACCOUNT=system claude` runs Claude Code with its own setup, without cam"
        )
    } else {
        err
    }
}

fn config_path() -> Result<PathBuf, Error> {
    config::path(env::var_os(config::ENV_CONFIG), platform::home_dir())
}

fn dispatch(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Add {
            name,
            share,
            login_args,
        } => commands::add(&name, &share, &login_args, &config_path()?),
        Command::Share { name, groups, yes } => {
            commands::share(&name, &groups, yes, &config_path()?)
        }
        Command::Unshare { name, groups, copy } => {
            commands::unshare(&name, &groups, copy, &config_path()?)
        }
        Command::Login { name, login_args } => {
            commands::login(name.as_deref(), &login_args, &config_path()?)
        }
        Command::Remove { name, yes } => commands::remove(&name, yes, &config_path()?),
        Command::List => commands::list(&config_path()?),
        Command::Use { name, dir } => commands::use_account(&name, dir.as_deref(), &config_path()?),
        Command::Unuse { dir } => commands::unuse(dir.as_deref()),
        Command::Which => commands::which(&config_path()?),
        Command::Whoami => commands::whoami(&config_path()?),
        Command::Doctor => commands::doctor(&config_path()?),
        Command::Exec { args } => match commands::exec(&args, &config_path()?)? {},
        Command::Init { shell } => {
            let config = config_path().ok();
            let script =
                completions::script(shell.completions(), Cli::command(), config.as_deref());
            commands::init(shell, &script)
        }
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
