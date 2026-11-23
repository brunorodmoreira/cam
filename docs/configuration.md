# Configuration

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

## The marker file

`.claude-account` holds one account name:

```
# comments start with #
work
```

- Names match `^[a-z0-9][a-z0-9_-]*$`. `system` is reserved and means Claude Code's own setup.
- The first line that isn't empty or a comment is the name. The rest of the file is ignored.
- It holds a name, never a secret. A hostile repo with its own marker can at most pick another one of *your* accounts.

## The config file

Accounts live in `~/.config/cam/config.toml`, or wherever `$CAM_CONFIG` points. It holds account names, never secrets, and you can edit it by hand. cam keeps your comments when it writes to it.

```toml
[accounts.work]
share = ["settings", "skills"]

[accounts.personal]
```

Each account's Claude Code folder is `~/.local/share/cam/accounts/<name>/`.

## Environment variables

| Variable | Meaning |
|---|---|
| `CAM_ACCOUNT` | Overrides markers: an account name, or `system`. |
| `CAM_CONFIG` | The config file, instead of `~/.config/cam/config.toml`. |
| `CAM_CLAUDE_BIN` | The absolute path of the real `claude`, if `PATH` lookup isn't what you want. |
| `CAM_ACTIVE_ACCOUNT` | Set by cam in claude's environment: the account in use, for status lines and scripts. |

If `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN` is set, Claude Code uses it instead of the login, so `cam exec` warns about it. `ANTHROPIC_API_KEY` is used too once you approve it: Claude Code asks once in an interactive session, and remembers the answer ("Use custom API key" in `/config`), so cam only adds a note. With `-p`, Claude Code uses the key without asking, and cam warns. `cam doctor` also looks for cloud-provider switches and `apiKeyHelper`, in the environment and in Claude Code's settings files.
