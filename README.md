# cam

[![CI](https://github.com/brunorodmoreira/cam/actions/workflows/ci.yml/badge.svg)](https://github.com/brunorodmoreira/cam/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cam-cli.svg)](https://crates.io/crates/cam-cli)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/cam-cli.svg)](#license)

Pick the Claude Code account per folder, the way `.nvmrc` picks a Node version.

Put a `.claude-account` file in a folder, and `claude` starts with that account there and in every folder below it. Each account is logged in with Claude Code's own `/login` and stays logged in, so you're signed in to all of them at once.

![cam adds an account that shares your instructions and skills, pins it to ~/Work, and claude then runs as work there and as your own login elsewhere](docs/demo.gif)

## Features

- **Per folder.** One marker at `~/Work` covers every repo under it. Nothing goes into the repos themselves.
- **Claude Code's own login.** cam runs `claude auth login` in the account's own config folder. It never sees, stores or passes a credential, and sessions get your plan, `/usage` limits and claude.ai connectors.
- **Plain `claude`.** One line in your shell's startup file, and `claude` picks the account wherever you start it. No `cd` hook.
- **Share what you want.** An account can share your settings, CLAUDE.md, skills or history with your own setup, or nothing at all.
- **It explains itself.** `cam which` says which account applies and why, `cam whoami` who it's logged in as, and `cam doctor` what could get in the way.

## Install

```sh
brew install brunorodmoreira/tap/cam
```

<details>
<summary>Other ways: install script, Windows, Cargo, or a download</summary>

The install script puts `cam` in `~/.local/bin`, next to Claude Code:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/brunorodmoreira/cam/releases/latest/download/cam-cli-installer.sh | sh
```

On Windows, in PowerShell, the install script puts `cam.exe` in `~\.local\bin`, next to Claude Code:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/brunorodmoreira/cam/releases/latest/download/cam-cli-installer.ps1 | iex"
```

With Rust, from [crates.io](https://crates.io/crates/cam-cli), where it's `cam-cli` because `cam` is taken:

```sh
cargo install cam-cli
```

Or download an archive for your platform from the [releases](https://github.com/brunorodmoreira/cam/releases). The binary is called `cam` either way.

</details>

## Quick start

The login you already have is `system`: Claude Code's own setup in `~/.claude`. cam leaves it alone, and it's what `claude` uses wherever no marker applies. Add another account next to it:

```sh
cam add work                 # Claude Code's login opens in the browser
cam use work --dir ~/Work    # every folder under ~/Work uses it
```

Then add one line at the end of your shell's startup file, after the line that puts Claude Code's `~/.local/bin` on `PATH`:

| Shell | File | Line |
|---|---|---|
| zsh | `~/.zshrc` | `eval "$(cam init zsh)"` |
| bash | `~/.bashrc` | `eval "$(cam init bash)"` |
| fish | `~/.config/fish/config.fish` | `cam init fish \| source` |
| PowerShell (Windows) | `$PROFILE` | `Invoke-Expression (& cam init powershell \| Out-String)` |

Open a new terminal, and `claude` under `~/Work` runs as `work`, anywhere else as `system`.

`cam add` asks which parts of your own setup the new account should share (see [Sharing](docs/sharing.md)). The rest starts empty: its first session shows Claude Code's first-run screens, and your MCP servers aren't in it.

## Commands

| Command | What it does |
|---|---|
| `cam add <name>` | Create an account and log it in with Claude Code's own login |
| `cam login [<name>]` | Log an account in again |
| `cam use <name> [--dir <dir>]` | Use an account in a folder and every folder below it |
| `cam unuse [--dir <dir>]` | Remove a folder's marker |
| `cam share <name> [<group>…]` | Share parts of `~/.claude` with an account |
| `cam unshare <name> <group>…` | Stop sharing them |
| `cam list` | `system` and the accounts, with the email each is logged in as |
| `cam which [--short]` | The account that applies here, and why. `--short` is for your shell prompt |
| `cam whoami` | Who that account is logged in as |
| `cam doctor` | Check the setup, the logins and what could get in the way |
| `cam remove <name>` | Log an account out and delete it |
| `cam exec [<args>…]` | Run `claude` with the account, without the shell line |

`cam <command> --help` has the details.

## Documentation

- [Using cam](docs/usage.md): adding accounts, folders, plain `claude`, logging in again, checking the setup, the status line, the shell prompt (Starship), completions and removing an account.
- [Sharing](docs/sharing.md): which parts of your setup an account can share, and how.
- [Configuration](docs/configuration.md): how cam picks an account, the marker file, the config file and environment variables.
- [Herdr](docs/herdr.md): the account on each `claude` in Herdr's sidebar, and a popup to change it.
- [DESIGN.md](DESIGN.md): the full spec, with what Claude Code was observed to do and the decisions along the way.
- [CHANGELOG.md](CHANGELOG.md): what changed in each release.

## FAQ

**`claude` uses the wrong account.** Run `cam doctor`. It says whether plain `claude` reaches cam (the `cam init` line has to come after the one that adds `~/.local/bin`), which account applies here and why, and whether something such as `ANTHROPIC_API_KEY` outranks the login.

**Why not set `CLAUDE_CONFIG_DIR` myself?** That's what cam does, per folder and only for the `claude` it starts. On top, it runs the login in the right folder, keeps markers out of your repos, shares what you choose, and explains which account applies.

**Does cam see my credentials?** No. Logins are Claude Code's own, kept where Claude Code keeps them. cam only runs `claude auth login`, `auth status` and `auth logout`, and from `auth status` it reads only the email, org, plan and expiry.

**Does it work in VS Code or another editor?** Not checked yet. An editor extension that runs its own bundled `claude` rather than the one on `PATH` would skip cam.

**How do I get cam out of the way?** `CAM_ACCOUNT=system claude` runs Claude Code untouched, even with a broken config. `cam use system` does it for a folder, and deleting the `cam init` line stops it for plain `claude`.

## Upgrading and uninstalling

Upgrade the way you installed: `brew upgrade cam`, the install script again, or `cargo install cam-cli`.

To uninstall:

1. Delete the `cam init` line from your shell's startup file.
2. Run `cam remove <name>` for each account you don't want to keep. It logs the account out, so its login doesn't stay behind.
3. Delete `~/.local/share/cam` (the link for plain `claude`, and the accounts' folders) and `~/.config/cam`.
4. Uninstall the binary: `brew uninstall cam`, `cargo uninstall cam-cli`, or delete `~/.local/bin/cam` (`cam.exe` on Windows).

`.claude-account` files left in folders do nothing without cam.

## Platforms

- **macOS:** the main platform, arm64 and Intel. Claude Code keeps each account's login in the Keychain itself.
- **Linux:** released, and the whole test suite passes there, but cam hasn't been tried in a real Claude Code session on Linux yet. The binaries need glibc 2.35 or newer (Ubuntu 22.04, Debian 12). Reports are welcome.
- **Windows:** new, x64, from PowerShell. Tried in real Claude Code sessions on Windows Server 2025, with two accounts, sharing and plain `claude`, and the test suite runs on Windows in CI. Reports from Windows 11 are welcome. Plain `claude` goes through a copy of cam named `claude.exe` in `~\.local\share\cam\shims`, which `cam init powershell` keeps up to date. Sharing links folders through junctions, which need nothing special, and files through symlinks, which Windows makes only with Developer Mode on: without it, `skills` is the group that can be shared.
- **Shells:** `cam init` writes for bash, zsh, fish and PowerShell, which is the one on Windows. In another shell, put `~/.local/share/cam/shims` first on `PATH` yourself, after running `cam init bash` once to make the link.

## Contributing

Bug reports and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). For a security problem, see [SECURITY.md](SECURITY.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
