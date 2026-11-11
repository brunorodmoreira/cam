# cam

Pick the Claude Code account per folder, the way `.nvmrc` picks a Node version.

Put a `.claude-account` file in a folder. The nearest one up the tree decides which account `claude` starts with. Your settings, plugins, memory and history stay shared across accounts. Only the credential changes.

> **Status: early.** Milestones 1 and 2 of 5 are done: cam stores tokens in the Keychain and resolves which account applies where. It can't launch `claude` yet, so it doesn't switch accounts yet. See the [roadmap](#roadmap).

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
cam add work --email you@company.com
cam add personal
cam list
```

```console
ACCOUNT   EMAIL            TOKEN ADDED
personal  -                2026-11-11 (today)
work      you@company.com  2026-11-11 (today)
```

Then pin accounts to folders:

```sh
cam use work                    # write .claude-account here
cam use none --dir ~/Work/oss   # force pass-through for a subtree
cam which                       # what applies here, and why
cam unuse                       # remove the marker in this folder
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
email = "you@company.com"
token_created = 2026-11-11
```

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
- **Linux:** resolution and markers should work, but they aren't tested. `add`/`remove` need a secret store, and a Linux one is a v2 idea.
- **Windows:** not supported. The launcher relies on Unix `exec` and a symlink shim.
- **Shells:** any. cam resolves when `claude` starts, not through a shell hook, so bash, zsh and fish behave the same.

## Roadmap

- [x] 1. Resolution, `which`, `use`/`unuse` and the config file
- [x] 2. Token storage in the Keychain, with `add`, `list` and `remove`
- [ ] 3. `exec`: find the real `claude`, inject the token, warn about conflicting variables
- [ ] 4. Shim mode: a `claude` → `cam` symlink on PATH, so plain `claude` picks the account
- [ ] 5. `whoami` and `doctor`: check that each token belongs to the expected email, and warn before it expires

## Trade-offs of token mode

These come from Claude Code itself:

- Sessions on `CLAUDE_CODE_OAUTH_TOKEN` can't use Remote Control or claude.ai connectors.
- The token lasts about a year.
- `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `apiKeyHelper` and cloud-provider variables outrank it.

A per-account config-dir mode is planned for v2 to cover these.

## Design

[DESIGN.md](DESIGN.md) is the full spec.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
