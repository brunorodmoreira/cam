# cam

Pick the Claude Code account per folder, the way `.nvmrc` picks a Node version.

Put a `.claude-account` file in a folder. The nearest one up the tree decides which account `claude` starts with. Each account is logged in with Claude Code's own `/login` and stays logged in, so you're signed in to all of them at once.

> **Status: 0.1, the first release.** macOS is the main platform, and Linux builds are new: see [Platforms](#platforms). What changed in each release is in the [changelog](CHANGELOG.md).

## Quick start

The login you already have is `system`: Claude Code's own setup in `~/.claude`. cam leaves it alone, and it's what `claude` uses wherever no marker applies. Add another account next to it:

```sh
brew install brunorodmoreira/tap/cam          # or see Install for other ways
cam add work                                  # Claude Code's login opens in the browser
cam use work --dir ~/Work                     # every folder under ~/Work uses it
echo 'eval "$(cam init zsh)"' >> ~/.zshrc     # plain `claude` picks the account
```

Open a new terminal, and `claude` under `~/Work` runs as `work`, anywhere else as `system`. `cam add` asks which parts of your own setup the new account should share: your settings, CLAUDE.md, skills or history (see [What an account shares](#what-an-account-shares)). The rest starts empty: its first session shows Claude Code's first-run screens, and your MCP servers aren't in it.

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

With [Homebrew](https://brew.sh), on macOS or Linux:

```sh
brew install brunorodmoreira/tap/cam
```

With the install script, which puts `cam` in `~/.local/bin`, next to Claude Code:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/brunorodmoreira/cam/releases/latest/download/cam-cli-installer.sh | sh
```

With Rust, from [crates.io](https://crates.io/crates/cam-cli), where it's `cam-cli` because `cam` is taken:

```sh
cargo install cam-cli
```

Or download an archive for your platform from the [releases](https://github.com/brunorodmoreira/cam/releases). The binary is called `cam` either way.

## Usage

Add an account. cam creates its folder, then runs Claude Code's login in it: the browser opens, and you sign in to the claude.ai account you want, as with `/login`.

```console
$ cam add work
Logging work in with Claude Code. It uses the claude.ai account your browser is signed in to.
Already in use: me@example.com (system).
...Claude Code's login...
added work: logged in as you@company.com (Acme, team)
$ cam list
ACCOUNT  EMAIL
system   me@example.com
work     you@company.com
```

The login uses whichever claude.ai account the browser is signed in to, so switch accounts there first if needed. Arguments after `--` go to `claude auth login`: `cam add work -- --email you@company.com` fills in the email, and `-- --sso` forces single sign-on. Accounts need a Claude subscription (Pro, Max, Team or Enterprise). A Console login is refused, because Claude Code keeps it outside the account's folder.

Before the login, on a terminal, `add` asks what the account should share; `--share settings,skills` or `--share none` answers up front, and without a terminal it shares nothing. After the login, `add` prints the next steps. If the login fails or you cancel it, cam logs the half-made account out and deletes its folder, so you can just try again. If the new account turns out to be the same claude.ai account as `system` or another account (same email, same org), cam warns: the browser was probably still signed in to it.

Then pin accounts to folders:

```sh
cam use work                      # write .claude-account here
cam use system --dir ~/Work/oss   # Claude Code's own setup for a subtree
cam which                         # what applies here, and why
cam unuse                         # remove the marker in this folder
```

`--dir` takes a leading `~`, even as `--dir=~/Work`. `use` says when it replaces a marker naming another account, and notes when the folder is in a git repository, since the marker is yours rather than the repo's.

Then make plain `claude` pick the account, with one line at the end of your shell's startup file:

```sh
eval "$(cam init zsh)"            # ~/.zshrc
eval "$(cam init bash)"           # ~/.bashrc
cam init fish | source            # ~/.config/fish/config.fish
```

It goes at the end, after the line that adds `~/.local/bin`, where Claude Code installs itself. At each shell start, `cam init` makes sure `~/.local/share/cam/shims/claude` is a link to cam, puts that folder first on `PATH` (once, even in nested shells), and loads completions with your account names. Open a new terminal, and every `claude` goes through cam. cam never edits your shell files itself, and it never replaces a `claude` there that isn't its own link.

cam finds the real `claude` on your `PATH` and replaces itself with it, so the terminal, signals and exit code are claude's own. For an account, that process gets `CLAUDE_CONFIG_DIR` set to the account's folder. For `system`, `claude` runs exactly as it would without cam.

Without that line, `cam exec` does the same:

```sh
cam exec                   # claude, with the account that applies here
cam exec -p "summarize"    # arguments go to claude unchanged
cam exec -- --help         # use -- when an argument could be cam's own
```

If cam ever gets in your way, `CAM_ACCOUNT=system claude` runs Claude Code untouched, even with a broken config. When plain `claude` stops because of a marker or the config, its error ends with that hint. To stop using cam for plain `claude`, delete the `cam init` line.

### Logging in again

Claude Code warns a few days before a login expires. To log in again, run `/login` in a session, or:

```sh
cam login          # the account that applies here
cam login work     # a named account
cam login system   # Claude Code's own setup
cam login work -- --sso            # arguments for `claude auth login`
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
ok    path: plain `claude` runs through cam (/Users/me/.local/share/cam/shims/claude)
ok    config: /Users/me/.config/cam/config.toml, 1 account
ok    system: logged in as me@example.com (Example Org, max), login expires 2026-12-11 (in 26 days)
ok    account work: logged in as you@company.com (Acme, team), login expires 2026-12-13 (in 28 days)
ok    here: work (from /Users/me/Work/.claude-account)
ok    environment: nothing outranks the login
ok    settings: nothing outranks the login (1 file checked)

no problems found
```

Both ask `claude auth status`, so they report what Claude Code itself says. `doctor` checks:
- that the real `claude` is found, and that plain `claude` reaches cam;
- that the config parses and every account has its folder;
- each login, with a warning within 3 days of expiry, when Claude Code starts warning too;
- two accounts logged in as the same claude.ai account (same email, same org);
- the folder's marker;
- anything that would make Claude Code ignore the login, in your environment and in Claude Code's settings files.

It exits non-zero if a check fails. Neither command reads a credential.

### Showing the account in Claude Code

cam sets `CAM_ACTIVE_ACCOUNT` in the environment of the `claude` it starts, and Claude Code passes it on to a [status line](https://code.claude.com/docs/en/statusline) command. To see the account under the prompt, add this to the account's `settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "echo \"${CAM_ACTIVE_ACCOUNT:-system}\""
  }
}
```

`system`'s is `~/.claude/settings.json`. An account that shares `settings` uses that same file, so one status line covers them all. Otherwise the account's own is `~/.local/share/cam/accounts/<name>/settings.json`. If you already have a status line script, have it print `${CAM_ACTIVE_ACCOUNT:-system}` too.

### Shell completions

`cam init` loads completions with your account names, so `cam use <TAB>` offers them. Accounts you add later show up in new shells. In zsh they need `compinit`, which frameworks like Oh My Zsh run for you: the `cam init` line has to come after it.

For completions without the rest, `cam completions <shell>` prints only the script:

```sh
eval "$(cam completions zsh)"     # ~/.zshrc, after compinit
eval "$(cam completions bash)"    # ~/.bashrc
cam completions fish | source     # ~/.config/fish/config.fish
```

### Removing an account

```sh
cam remove work
```

It asks first, since everything in the account's folder goes, including its transcripts and memory, unless it shares them: what it shares stays in `~/.claude`. `--yes` skips the question, and scripts need it. Then it logs the account out, so its login doesn't stay behind (in the Keychain, on macOS), and deletes its folder and its config entry. If the logout fails, nothing is deleted. A `.claude-account` that still names the account then makes `claude` stop with an error until you change it.

### The config file

Accounts live in `~/.config/cam/config.toml`, or wherever `$CAM_CONFIG` points. It holds account names, never secrets, and you can edit it by hand. cam keeps your comments when it writes to it.

```toml
[accounts.work]
share = ["settings", "skills"]

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

If `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN` is set, Claude Code uses it instead of the login, so `cam exec` warns about it. `ANTHROPIC_API_KEY` is used too once you approve it: Claude Code asks once in an interactive session, and remembers the answer ("Use custom API key" in `/config`), so cam only adds a note. With `-p`, Claude Code uses the key without asking, and cam warns. `cam doctor` also looks for cloud-provider switches and `apiKeyHelper`, in the environment and in Claude Code's settings files.

## What an account shares

Nothing, unless you choose. An account can share groups of your own setup in `~/.claude`, through links in its folder. A shared group is the same files for both, so a change in either account shows up in the other.

| Group | Links | Covers |
|---|---|---|
| `settings` | `settings.json`, `keybindings.json`, `plugins/`, `hooks/` | permissions, hooks, model, theme, status line, plugins |
| `instructions` | `CLAUDE.md`, `rules/` | your global instructions |
| `skills` | `skills/`, `agents/`, `commands/`, `output-styles/` | your own skills, agents, slash commands and output styles |
| `history` | `projects/`, `history.jsonl`, `file-history/`, `todos/`, `plans/` | transcripts, `/resume`, auto memory, prompt history |

```sh
cam add work --share settings,skills   # or answer when cam asks
cam share work history                 # share more later
cam share work                         # tick and untick, on a terminal
cam unshare work history               # stop: removes the links, your files stay
cam unshare work settings --copy       # stop, and keep a copy of the current files
```

Groups go comma-separated or as separate words, and `all` is every group. `cam list` and `cam whoami` show what each account shares.

Sharing goes both ways, so choose with that in mind. With `settings`, the same hooks and plugins run in both accounts. With `history`, work transcripts and memory sit next to personal ones in `~/.claude/projects`. An org's managed settings and policy caches always stay in its own account.

MCP servers are never shared: Claude Code keeps them in `.claude.json`, next to the login. Add them per account with `CAM_ACCOUNT=work claude mcp add -s user …`, or get them from a plugin in a shared `settings`. Each account also goes through Claude Code's first-run screens once.

When an account already has its own copy of something it starts to share, such as its own `settings.json`, `cam share` asks, then renames it to `settings.json.before-share-<date>`. Nothing is deleted: copy what you want from it into `~/.claude`. `cam doctor` lists these copies until you delete them.

Every launch adds missing links, and never replaces a real file. If something saved a shared file by replacing it, cam says so at launch, and `cam share work settings` links it again. Claude Code's own writes keep the links (checked with Claude Code 2.1.330), and so do saves from vim and Zed. `cam doctor` checks each link.

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

- **macOS:** the main platform, arm64 and Intel. Claude Code keeps each account's login in the Keychain itself.
- **Linux:** released, and the whole test suite passes there, but cam hasn't been tried in a real Claude Code session on Linux yet. The binaries need glibc 2.35 or newer (Ubuntu 22.04, Debian 12). Reports are welcome.
- **Windows:** not supported. The launcher relies on Unix `exec` and a symlink.
- **Shells:** `cam init` writes for bash, zsh and fish. cam resolves when `claude` starts, not through a `cd` hook, so they all behave the same. In another shell, put `~/.local/share/cam/shims` first on `PATH` yourself, after running `cam init bash` once to make the link.

## Design

[DESIGN.md](DESIGN.md) is the full spec: how cam resolves and launches, what Claude Code was observed to do, and the decisions along the way.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
