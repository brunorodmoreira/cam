# cam

Pick the Claude Code account per folder, the way `.nvmrc` picks a Node version.

Put a `.claude-account` file in a folder. The nearest one up the tree decides which account `claude` starts with. Your settings, plugins, memory and history stay shared across accounts. Only the credential changes.

> **Status: early.** cam stores tokens in the Keychain, works out which account applies where, starts `claude` with that account through `cam exec` or a `claude` shim, checks the setup with `whoami` and `doctor`, and has shell completions. There's no release yet: install from source. See the [roadmap](#roadmap).

## Why

Most Claude Code account switchers point `CLAUDE_CONFIG_DIR` at a different folder per account. That isolates credentials, but it also splits your settings, plugins, hooks, memory and history.

cam swaps only the credential. It takes a long-lived token from `claude setup-token` and passes it as `CLAUDE_CODE_OAUTH_TOKEN`, only to the `claude` process it starts. No `cd` hook exports secrets into your shell.

## How it picks an account

First match wins:

1. The `CAM_ACCOUNT` environment variable: an account name, or `none`.
2. The nearest `.claude-account`, from the current folder up to `/`.
3. `default` in the config file.
4. Nothing: pass-through. `claude` runs untouched and uses your normal `/login`.

```
~/Work/.claude-account        work   ← covers every repo under ~/Work
~/Work/oss/.claude-account    none   ← except these, which use my own login
```

An unknown or invalid account name is an error. cam never quietly falls back to another account.

`cam which` tells you what it picked and why:

```console
$ cd ~/Work/client/api && cam which
work (from /Users/me/Work/.claude-account)
$ cd ~/Work/oss/tool && cam which
none (pass-through, from /Users/me/Work/oss/.claude-account)
```

## Install

From source, with a recent stable Rust:

```sh
cargo install --git https://github.com/brunorodmoreira/cam
```

It isn't on crates.io yet. It will be published as `cam-cli`, because `cam` is taken.

## Usage

Add an account. Run `claude setup-token` while logged in to that account, then paste the token it prints when cam asks. The input is hidden, and the token goes straight into the macOS Keychain:

```sh
cam add work
cam add personal
cam list
```

```console
ACCOUNT   TOKEN ADDED
personal  2026-11-11 (today)
work      2026-11-11 (today)
```

Then pin accounts to folders:

```sh
cam use work                    # write .claude-account here
cam use none --dir ~/Work/oss   # force pass-through for a subtree
cam which                       # what applies here, and why
cam unuse                       # remove the marker in this folder
```

Then make plain `claude` pick the account:

```sh
cam shim install
```

This creates `~/.local/share/cam/shims/claude`, a symlink to cam, and prints the line that puts that folder first on your `PATH`, written for your shell. Add it below the line that adds `~/.local/bin`, where Claude Code installs itself, and open a new terminal. From then on, every `claude` goes through cam. cam never edits your shell files itself, and it never replaces a `claude` that isn't its own shim.

cam finds the real `claude` on your `PATH` and replaces itself with it, so the terminal, signals and exit code are claude's own. Only that process gets the token. In a pass-through folder, `claude` runs exactly as it would without cam.

Without the shim, `cam exec` does the same:

```sh
cam exec                   # claude, with the account that applies here
cam exec -p "summarize"    # arguments go to claude unchanged
cam exec -- --help         # use -- when an argument could be cam's own
```

If cam ever gets in your way, `CAM_ACCOUNT=none claude` runs Claude Code untouched, even with a broken config. `cam shim uninstall` removes the shim.

To check what applies and whether anything will get in the way:

```console
$ cam whoami
work (from /Users/me/Work/.claude-account)
  token:   stored in the macOS Keychain, added 2026-11-11 (3 days ago), good until about 2027-11-11
$ cam doctor
ok    claude: /Users/me/.local/bin/claude
ok    shim: plain `claude` runs through cam (/Users/me/.local/share/cam/shims/claude)
ok    config: /Users/me/.config/cam/config.toml, 2 accounts, no default
ok    account personal: token stored, added 2026-11-02 (12 days ago), good until about 2027-11-02
ok    account work: token stored, added 2026-11-11 (3 days ago), good until about 2027-11-11
ok    here: work (from /Users/me/Work/.claude-account)
ok    environment: nothing outranks cam's tokens
ok    settings: nothing outranks cam's tokens (1 file checked)

no problems found
```

`doctor` checks:
- that the real `claude` is found and the shim comes first on `PATH`;
- that the config parses and every account has a stored token;
- token age, with a warning from 330 days on (tokens last about a year);
- the folder's marker;
- anything that would make Claude Code ignore cam's token, in your environment and in Claude Code's settings files.

It exits non-zero if a check fails. Neither command reads a token, so neither can cause a Keychain prompt. Neither can tell whose a token is, either: in token mode, Claude Code doesn't report the account's email.

### Shell completions

`cam completions <shell>` prints a completion script with your account names filled in, so `cam use <TAB>` offers them. Load it from your shell's startup file rather than saving it, so accounts you add later show up in new shells:

```sh
eval "$(cam completions zsh)"     # ~/.zshrc, after compinit
eval "$(cam completions bash)"    # ~/.bashrc
cam completions fish | source     # ~/.config/fish/config.fish
```

Other commands:

```sh
cam add work --force    # replace a token, say when it's close to expiring
cam remove work         # delete the token and the account
```

Accounts live in `~/.config/cam/config.toml`, or wherever `$CAM_CONFIG` points. The file never holds secrets, and you can edit it by hand. cam keeps your comments when it writes to it. To use an account wherever no marker applies, set `default`:

```toml
default = "personal"

[accounts.work]
token_created = 2026-11-11
```

### Environment variables

| Variable | Meaning |
|---|---|
| `CAM_ACCOUNT` | Overrides markers and the default: an account name, or `none`. |
| `CAM_CONFIG` | The config file, instead of `~/.config/cam/config.toml`. |
| `CAM_CLAUDE_BIN` | The absolute path of the real `claude`, if `PATH` lookup isn't what you want. |
| `CAM_ACTIVE_ACCOUNT` | Set by cam in claude's environment: the account in use, for status lines and scripts. |

If `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN` is set, Claude Code ranks it above the account token, so `cam exec` warns about it. `cam doctor` also looks for cloud-provider switches and `apiKeyHelper`, in the environment and in Claude Code's settings files.

## The marker file

`.claude-account` holds one account name:

```
# comments start with #
work
```

- Names match `^[a-z0-9][a-z0-9_-]*$`. `none` is reserved and means pass-through.
- The first line that isn't empty or a comment is the name. The rest of the file is ignored.
- It holds a name, never a secret. A hostile repo with its own marker can at most pick another one of *your* accounts.

## Platforms

- **macOS:** the target for v1. Tokens live in the Keychain, under the service name `cam`.
- **Linux:** resolution, markers and pass-through `exec` should work, but they aren't tested. Accounts need a secret store, and a Linux one is a v2 idea.
- **Windows:** not supported. The launcher relies on Unix `exec` and a symlink shim.
- **Shells:** any. cam resolves when `claude` starts, not through a shell hook, so bash, zsh and fish behave the same.

## Roadmap

- [x] 1. Resolution, `which`, `use`/`unuse` and the config file
- [x] 2. Token storage in the Keychain, with `add`, `list` and `remove`
- [x] 3. `exec`: find the real `claude`, inject the token, warn about conflicting variables
- [x] 4. Shim mode: a `claude` → `cam` symlink on PATH, so plain `claude` picks the account
- [x] 5. `whoami` and `doctor`: what applies here, stored tokens and their age, shim order, and anything that outranks the token
- [ ] 6. Shell completions (done) and a release

Checking whose a token is, and that the server still accepts it, needs a source Claude Code doesn't offer in token mode. It's an open question in DESIGN.md.

## Trade-offs of token mode

These come from Claude Code itself:

- Sessions on `CLAUDE_CODE_OAUTH_TOKEN` can't use Remote Control or claude.ai connectors.
- The token lasts about a year.
- Claude Code can't see your plan in token mode. The startup header says "Claude API", and `/usage` shows a dollar estimate instead of your plan's limits. Usage still counts against your subscription. You can watch the plan meters move in Settings → Usage on claude.ai.
- `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `apiKeyHelper` and cloud-provider variables outrank it.

A per-account config-dir mode is planned for v2 to cover these.

## Design

[DESIGN.md](DESIGN.md) is the full spec.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
