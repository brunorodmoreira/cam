# cam

Pick the Claude Code account per folder, the way `.nvmrc` picks a Node version.

Put a `.claude-account` file in a folder. The nearest one up the tree decides which account `claude` starts with. Each account is logged in with Claude Code's own `/login` and stays logged in, so you're signed in to all of them at once.

> **Status: early.** Accounts, logins through Claude Code, per-folder markers, a `claude` shim, `whoami`, `doctor` and shell completions all work. Each account has its own Claude Code setup for now: sharing your settings, skills and history with an account comes next. There's no release yet: install from source. See the [roadmap](#roadmap).

## Why

Claude Code's own answer to several accounts is a config folder per account, picked with `CLAUDE_CONFIG_DIR` ([docs](https://code.claude.com/docs/en/authentication.md#log-in-with-multiple-accounts)). cam builds on that:

- **Per folder.** A marker file at `~/Work` covers every repo under it. Nothing goes into the repos themselves.
- **Claude Code's own login.** `cam add` runs `claude auth login` in the account's folder. cam never sees, stores or passes a credential, and sessions get the whole product: your plan, `/usage` limits and claude.ai connectors.
- **Only for `claude`.** cam sets `CLAUDE_CONFIG_DIR` for the `claude` process it starts. No `cd` hook changes your shell.
- **It explains itself.** `cam which` says which account applies and why, `cam whoami` who it's logged in as, and `cam doctor` what could get in the way.

## How it picks an account

First match wins:

1. The `CAM_ACCOUNT` environment variable: an account name, or `system`.
2. The nearest `.claude-account`, from the current folder up to `/`.
3. Nothing: `system`. `claude` runs untouched, with Claude Code's own setup in `~/.claude` and your usual `/login`.

```
~/Work/.claude-account        work     ← covers every repo under ~/Work
~/Work/oss/.claude-account    system   ← except these, which use my own login
```

An unknown or invalid account name is an error. cam never quietly falls back to another account. To use an account almost everywhere, put a marker in your home folder: `cam use personal --dir ~`.

`cam which` tells you what it picked and why:

```console
$ cd ~/Work/client/api && cam which
work (from /Users/me/Work/.claude-account)
$ cd ~/Work/oss/tool && cam which
system (from /Users/me/Work/oss/.claude-account)
```

## Install

From source, with a recent stable Rust:

```sh
cargo install --git https://github.com/brunorodmoreira/cam
```

It isn't on crates.io yet. It will be published as `cam-cli`, because `cam` is taken.

## Usage

Add an account. cam creates its folder, then runs Claude Code's login in it: the browser opens, and you sign in to the claude.ai account you want, as with `/login`.

```console
$ cam add work
...Claude Code's login...
added work: logged in as you@company.com (Acme, team)
$ cam list
ACCOUNT  EMAIL
system   me@example.com
work     you@company.com
```

If the login fails or you cancel it, cam logs the half-made account out and deletes its folder, so you can just try again. If the browser was signed in to the wrong claude.ai account, cam warns that the email matches another account's.

Then pin accounts to folders:

```sh
cam use work                      # write .claude-account here
cam use system --dir ~/Work/oss   # Claude Code's own setup for a subtree
cam which                         # what applies here, and why
cam unuse                         # remove the marker in this folder
```

Then make plain `claude` pick the account:

```sh
cam shim install
```

This creates `~/.local/share/cam/shims/claude`, a symlink to cam, and prints the line that puts that folder first on your `PATH`, written for your shell. Add it below the line that adds `~/.local/bin`, where Claude Code installs itself, and open a new terminal. From then on, every `claude` goes through cam. cam never edits your shell files itself, and it never replaces a `claude` that isn't its own shim.

cam finds the real `claude` on your `PATH` and replaces itself with it, so the terminal, signals and exit code are claude's own. For an account, that process gets `CLAUDE_CONFIG_DIR` set to the account's folder. For `system`, `claude` runs exactly as it would without cam.

Without the shim, `cam exec` does the same:

```sh
cam exec                   # claude, with the account that applies here
cam exec -p "summarize"    # arguments go to claude unchanged
cam exec -- --help         # use -- when an argument could be cam's own
```

If cam ever gets in your way, `CAM_ACCOUNT=system claude` runs Claude Code untouched, even with a broken config. `cam shim uninstall` removes the shim.

### Logging in again

Claude Code warns a few days before a login expires. To log in again, run `/login` in a session, or:

```sh
cam login          # the account that applies here
cam login work     # a named account
cam login system   # Claude Code's own setup
```

### Checking the setup

```console
$ cam whoami
work (from /Users/me/Work/.claude-account)
  email:   you@company.com
  org:     Acme (team)
  login:   expires 2026-12-13 (in 28 days)
$ cam doctor
ok    claude: /Users/me/.local/bin/claude
ok    shim: plain `claude` runs through cam (/Users/me/.local/share/cam/shims/claude)
ok    config: /Users/me/.config/cam/config.toml, 1 account
ok    system: logged in as me@example.com (Example Org, max), login expires 2026-12-11 (in 26 days)
ok    account work: logged in as you@company.com (Acme, team), login expires 2026-12-13 (in 28 days)
ok    here: work (from /Users/me/Work/.claude-account)
ok    environment: nothing outranks the login
ok    settings: nothing outranks the login (1 file checked)

no problems found
```

Both ask `claude auth status`, so they report what Claude Code itself says. `doctor` checks:
- that the real `claude` is found and the shim comes first on `PATH`;
- that the config parses and every account has its folder;
- each login, with a warning within 3 days of expiry, when Claude Code starts warning too;
- two accounts logged in as the same email;
- the folder's marker;
- anything that would make Claude Code ignore the login, in your environment and in Claude Code's settings files.

It exits non-zero if a check fails. Neither command reads a credential.

### Shell completions

`cam completions <shell>` prints a completion script with your account names filled in, so `cam use <TAB>` offers them. Load it from your shell's startup file rather than saving it, so accounts you add later show up in new shells:

```sh
eval "$(cam completions zsh)"     # ~/.zshrc, after compinit
eval "$(cam completions bash)"    # ~/.bashrc
cam completions fish | source     # ~/.config/fish/config.fish
```

### Removing an account

```sh
cam remove work
```

This logs the account out first, so its login doesn't stay behind in the Keychain, then deletes its folder and its config entry. If the logout fails, nothing is deleted. Everything in the account's folder goes, including its transcripts and memory.

### The config file

Accounts live in `~/.config/cam/config.toml`, or wherever `$CAM_CONFIG` points. It holds account names, never secrets, and you can edit it by hand. cam keeps your comments when it writes to it.

```toml
[accounts.work]

[accounts.personal]
```

Each account's Claude Code folder is `~/.local/share/cam/accounts/<name>/`.

### Environment variables

| Variable | Meaning |
|---|---|
| `CAM_ACCOUNT` | Overrides markers: an account name, or `system`. |
| `CAM_CONFIG` | The config file, instead of `~/.config/cam/config.toml`. |
| `CAM_CLAUDE_BIN` | The absolute path of the real `claude`, if `PATH` lookup isn't what you want. |
| `CAM_ACTIVE_ACCOUNT` | Set by cam in claude's environment: the account in use, for status lines and scripts. |

If `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN` is set, Claude Code ranks it above the login, so `cam exec` warns about it. `cam doctor` also looks for cloud-provider switches and `apiKeyHelper`, in the environment and in Claude Code's settings files.

## What an account shares

Nothing, for now. A new account starts with an empty Claude Code setup and goes through Claude Code's first-run screens once. Its settings, plugins, skills, CLAUDE.md, MCP servers, transcripts and memory are its own.

Next, cam will let each account share groups of your setup in `~/.claude`, kept in sync with links: settings (with plugins and hooks), instructions, extensions such as skills and agents, and history. MCP servers stay per account: add them with `CAM_ACCOUNT=work claude mcp add …`. See [DESIGN.md](DESIGN.md#sharing-m8).

## The marker file

`.claude-account` holds one account name:

```
# comments start with #
work
```

- Names match `^[a-z0-9][a-z0-9_-]*$`. `system` is reserved and means Claude Code's own setup.
- The first line that isn't empty or a comment is the name. The rest of the file is ignored.
- It holds a name, never a secret. A hostile repo with its own marker can at most pick another one of *your* accounts.

## Platforms

- **macOS:** the target for v1. Claude Code keeps each account's login in the Keychain itself.
- **Linux:** nothing in cam is macOS-only any more, so it may just work, but it isn't tested.
- **Windows:** not supported. The launcher relies on Unix `exec` and a symlink shim.
- **Shells:** any. cam resolves when `claude` starts, not through a shell hook, so bash, zsh and fish behave the same.

## Upgrading from token mode

Before milestone 7, cam passed a token from `claude setup-token` and kept it in the Keychain. The new build no longer knows those tokens, and it rejects a config that still has `token_created` or `default`. To move over:

1. With the old build, run `cam remove <name>` for each account. That deletes its token from the Keychain. If you've already upgraded, delete each one yourself with `security delete-generic-password -s cam -a <name>`.
2. Remove any `token_created` and `default` lines left in the config.
3. With the new build, run `cam add <name>` for each account.

A `none` marker from token mode is now an unknown account name. Replace it with `cam use system`.

## Roadmap

- [x] 1. Resolution, `which`, `use`/`unuse` and the config file
- [x] 2. Accounts with `add`, `list` and `remove`
- [x] 3. `exec`: find the real `claude`, set the account, warn about conflicting variables
- [x] 4. Shim mode: a `claude` → `cam` symlink on PATH, so plain `claude` picks the account
- [x] 5. `whoami` and `doctor`
- [x] 6. Shell completions
- [x] 7. Native logins: each account is its own Claude Code config folder with its own `/login`
- [ ] 8. Sharing: pick which parts of your setup each account shares, kept in sync with links
- [ ] 9. A release

## Design

[DESIGN.md](DESIGN.md) is the full spec.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
