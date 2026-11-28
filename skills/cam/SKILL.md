---
name: cam
description: cam picks the Claude Code account per folder, with `.claude-account` files. Use when the user asks which Claude Code account, email, org or plan this session or a folder uses, wants a folder or repo to use another account, mentions cam, `.claude-account` or `CAM_ACCOUNT`, wants to add, log in, list or remove Claude Code accounts or have them share settings, skills or history, or when `claude` starts with the wrong account.
---

# cam

cam picks the Claude Code account per folder. A `.claude-account` file names an account, and the nearest one from the current folder up wins. Plain `claude` goes through cam (set up with `cam init <shell>`), which starts the real `claude` with the account's own config folder, `~/.local/share/cam/accounts/<name>/`, as `CLAUDE_CONFIG_DIR`. Where no marker applies, the account is `system`: Claude Code's own `~/.claude` and login, untouched.

Each account is logged in with Claude Code's own login, and cam never sees a credential.

## Find out first

| Question | Command |
|---|---|
| Which account is this session using? | `echo "${CAM_ACTIVE_ACCOUNT:-system}"`: cam sets it for the `claude` it starts |
| Which account would `claude` use in this folder, and why? | `cam which` |
| Who is that account logged in as? | `cam whoami` |
| Which accounts are there, with their emails? | `cam list` |
| Why does `claude` pick the wrong account, or not use the login? | `cam doctor` |

`cam which` answers for a folder, as it is now. A session keeps the account it started with: a marker written during it applies from the next `claude` started there.

## Use an account in a folder

- `cam use <name>` writes `.claude-account` in the current folder, and `--dir <dir>` writes it in another one. It covers that folder and every folder below it, and replaces a marker that's already there.
- `cam use system --dir <dir>` gives a subtree Claude Code's own setup.
- `cam unuse [--dir <dir>]` removes the marker in that folder only. If the folder has none, it fails and names the nearest marker above.
- One marker above many repos, such as in `~/Work`, usually beats one per repo. A marker inside a git repository is the user's file, not the repo's: ask before writing one there, and never commit it unless the user wants that.
- For one run only, `CAM_ACCOUNT=<name> claude …` beats any marker. `CAM_ACCOUNT=system claude` runs Claude Code untouched.

After a change, tell the user to start `claude` again in that folder. A session's transcript stays with the account it ran in, so `claude --continue` or `/resume` in another account finds it only if both share `history`.

## Accounts and sharing

- **Adding or logging in needs the user.** `cam add <name>` and `cam login [<name>]` open a browser, where the user signs in to the claude.ai account they want. Ask the user to run them in a terminal of their own. `add` asks what to share unless `--share <groups>` or `--share none` answers it. Arguments after `--` go to `claude auth login`, such as `-- --email <email>` or `-- --sso`.
- **Sharing.** A new account shares nothing. A shared group links part of `~/.claude` into the account, so a change in either shows up in both:
  - `settings`: `settings.json`, `keybindings.json`, `plugins/`, `hooks/`
  - `instructions`: `CLAUDE.md`, `rules/`
  - `skills`: `skills/`, `agents/`, `commands/`, `output-styles/`
  - `history`: `projects/`, `history.jsonl`, `file-history/`, `todos/`, `plans/`

  `cam share <name> <group>…` starts sharing, and `cam unshare <name> <group>…` stops, removing only the links (`--copy` leaves a copy in each link's place). When the account has its own copy of something it starts to share, `share` renames it to `<name>.before-share-<date>` and deletes nothing. Without a terminal it needs `--yes`: ask the user before passing it. What to share is the user's call: with `settings`, the same hooks and plugins run in both accounts, and with `history`, work transcripts sit next to personal ones.
- **MCP servers are never shared.** Add one to an account with `CAM_ACCOUNT=<name> claude mcp add -s user …`.
- **Removing an account** with `cam remove <name>` logs it out and deletes its folder. Run it only when the user asks for it, and let them confirm. Without a terminal it needs `--yes`.

## When `claude` uses the wrong account

Run `cam doctor` and go by what it reports. The usual causes:
- Plain `claude` doesn't reach cam. The `cam init` line belongs at the end of the shell's startup file, after the line that adds `~/.local/bin`. Tell the user what to add, and don't edit their startup files unless they ask.
- A marker nearer the folder names another account. `cam which` shows which file it is.
- `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN` is set, and Claude Code uses it instead of the login.
- A shared file became a plain file. `cam share <name> <group>` links it again.

## Rules

- Never read, print or copy a `.claude.json`, `.credentials.json` or Keychain item, of any account or of `system`. They hold logins and MCP server secrets. `cam list` and `cam whoami` give the email, org and plan.
- Never set `CLAUDE_CONFIG_DIR` or `CLAUDE_SECURESTORAGE_CONFIG_DIR` to switch accounts. Use a marker or `CAM_ACCOUNT`.
- Don't create, move or delete anything in an account's folder by hand, links included. cam's commands do that, and `cam doctor` checks it.
- The config, `~/.config/cam/config.toml` (or `$CAM_CONFIG`), holds account names and what each shares, never secrets. It can be edited by hand, and an unknown key is an error.

cam links this skill into each account that doesn't share `skills`. `skill = false` at the top of the config turns that off. The docs are at https://github.com/brunorodmoreira/cam/tree/main/docs.
