# cam — Claude Account Manager

Status: redesign to native logins and opt-in sharing decided 2026-11-15 (M7 and M8, not built yet). The code still runs token mode: milestones 1–5 (resolution, config, token store, `add`/`remove`/`list`, `use`/`unuse`/`which`, `exec`, the shim, `whoami`/`doctor`) and shell completions. No release yet. · Written 2026-11-11, redesigned 2026-11-15

Pick the Claude Code account **per folder**, the way `.nvmrc` / `.node-version` pick a Node version with fnm. A `.claude-account` file names an account. The nearest one up the folder tree wins. `claude` starts logged in as that account. Each account starts with a Claude Code setup of its own, and you choose which parts of your usual setup it shares.

## Why another one

Existing tools ([Nemo-Illusionist/claude-code-account-switcher](https://github.com/Nemo-Illusionist/claude-code-account-switcher), [mtxr/claude-switch](https://github.com/mtxr/claude-switch), [realiti4/claude-swap](https://github.com/realiti4/claude-swap), [diananerd/claude-account-switcher](https://github.com/diananerd/claude-account-switcher), [fairy-pitta/cc-account-switcher](https://github.com/fairy-pitta/cc-account-switcher)) switch **`CLAUDE_CONFIG_DIR`** to a separate folder per account. That's Claude Code's documented way to stay logged in to several accounts, but it also splits settings, plugins, hooks, memory and history per account.

cam uses a config dir per account too. What it adds is picking the account per folder, and letting each account share chosen parts of your setup in `~/.claude` through links that stay in sync.

cam's differentiators:

1. **A portable marker file with walk-up lookup**, like fnm's `--version-file-strategy=recursive`. One file at a parent folder covers every nested repo, so nothing is committed into the repos themselves.
2. **Claude Code's own login.** Each account logs in through Claude Code's normal flow: `cam add` and `cam login` run `claude auth login` in the account's dir, and `/login` in a session works too. cam never sees, stores or passes a credential. Sessions get the whole product: the plan in the header, `/usage` limits and claude.ai connectors.
3. **Sharing you choose.** An account starts empty, the way Claude Code's docs set up several accounts. You can share whole groups of your setup with it: settings, instructions, extensions, history. They're symlinked from `~/.claude`, so they stay in sync.
4. **Resolved at launch, scoped to one process.** There's no `cd` hook. `CLAUDE_CONFIG_DIR` goes only into the environment of the `claude` process cam starts.
5. **It explains itself.** `cam which` says which account was chosen and why. `cam whoami` adds who that account is logged in as, according to Claude Code. `cam doctor` checks the setup, the links and anything that would make Claude Code ignore the login.

Docs facts this design relies on:
- "To stay signed in to multiple accounts at once, such as work and personal accounts, give each account its own configuration directory." (https://code.claude.com/docs/en/authentication.md#log-in-with-multiple-accounts)
- `CLAUDE_CONFIG_DIR` moves everything Claude Code keeps: "All settings, session history, and plugins are stored under this path." That includes `.claude.json`, which otherwise sits at `~/.claude.json`, and with it the user-scope MCP servers and per-project trust. Sharing parts across config dirs isn't documented (https://code.claude.com/docs/en/claude-directory.md).
- `CLAUDE_CONFIG_DIR` is **ignored in project/local `.claude/settings.json`**. That's why an external launcher is needed at all (verified 2026-11-11).
- Claude Code warns 3 days before a login expires ("Your login expires in 3 days · run /login to renew"), and asks for `/login` once it has (https://code.claude.com/docs/en/authentication.md#renew-an-expiring-login).
- Cloud-provider switches, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `apiKeyHelper` and `CLAUDE_CODE_OAUTH_TOKEN` all **outrank** a `/login` (https://code.claude.com/docs/en/authentication.md#authentication-precedence).

Observed with CLI 2.1.325 (spike, 2026-11-14):
- In a config dir linked to all four sharing groups (see Sharing), `claude auth login --email <work email>` opened the browser with the email filled in. Claude Code printed "Your organization may apply global configuration...". `auth status` then showed the work email, the org, `subscriptionType: "team"` and a `loginExpiresAt` 29 days out.
- A session in that dir showed "Claude Team" in the header. `/status` showed the org and email, with "User settings" and "Enterprise managed settings (remote)" as setting sources. The claude.ai connectors were connected. A plugin's MCP server needed its own login, and the user-scope MCP servers from `~/.claude.json` were absent.
- The shared parts worked: the user settings and plugins loaded, and the session's transcript went into the shared `~/.claude/projects`.
- Nothing leaked either way: `~/.claude/remote-settings.json` wasn't rewritten, and the system login and `~/.claude.json`'s `oauthAccount` were untouched.
- First-run screens appeared before the session started (which ones wasn't recorded).
- Deleting the account dir after `claude auth logout` left everything in `~/.claude` intact.
- Earlier, in a scratch config dir: `claude mcp add -s user` and `claude plugin install` wrote through a symlinked `.claude.json` and `settings.json`, and the links survived.
- From the CLI's code, not yet seen in the Keychain *(verify)*: the login is stored as a generic password with service `Claude Code-credentials`, plus `-<first 8 hex digits of sha256(CLAUDE_CONFIG_DIR)>` when the variable is set. So the dir's spelling matters, and cam must always pass the same one.
- The system login's `loginExpiresAt` moved from 2026-11-16 (seen 2026-11-12) to 2026-12-11, so it seems to be extended as the login is used *(verify for an idle account)*.

Rejected: the undocumented `CLAUDE_SECURESTORAGE_CONFIG_DIR`, which separates only the credential slot. With a shared `.claude.json`, a refresh merges one account's profile fields into the other's `oauthAccount` without checking `accountUuid`, and so does the org-roles fetch.

### Token mode (M1–M6, dropped 2026-11-15)
cam first passed a long-lived `claude setup-token` token as `CLAUDE_CODE_OAUTH_TOKEN`, with the token stored in the macOS Keychain under service `cam`. It was dropped because:
- Claude Code can't see the plan in token mode. The header said "Claude API" and `/usage` showed a dollar estimate, though usage did count against the plan's limits. claude.ai connectors and Remote Control don't work with the token.
- `auth status` reported no identity (`authMethod: "oauth_token"`, `loginExpiresAt: null`), so cam couldn't tell whose a token was. `--email` only recorded what the user typed, and was removed first.
- Every rebuild of cam brought back a Keychain prompt, and pasting the token into a hidden prompt was easy to get wrong.
- The org's cached managed settings ended up in the shared folder. Token sessions for a work account wrote `remote-settings.json` and `policy-limits.json` (with their `.signature`/`.stamp` files) into `~/.claude`, next to the system login's setup. Whether system sessions apply them is unknown *(verify)*. With per-account dirs these caches stay in the account.

## Accounts

- **`system`** is Claude Code's own setup: `~/.claude`, `~/.claude.json` and its own `/login`. cam never changes it, except to create an empty folder in it that an account shares (see Sharing). It's what runs where no marker applies.
- **A cam account `<name>`** is a folder, `~/.local/share/cam/accounts/<name>/` (mode 0700), that cam passes as `CLAUDE_CONFIG_DIR`.
  - It starts empty. Claude Code fills it on the first login and session, as it does any new config dir, and the first session shows Claude Code's first-run screens.
  - It shares only the groups chosen for it (see Sharing). Nothing, by default.
  - Always its own, shared or not: `.claude.json` (the account's identity, user-scope MCP servers, trusted folders and first-run state), its login, the org's managed settings and policy caches, and run-time state (`sessions`, `session-env`, `shell-snapshots`, `cache`, `stats-cache.json` and the like).
- **Names** match `^[a-z0-9][a-z0-9_-]*$`. `system` is reserved. `none`, the reserved name in token mode, is an ordinary name now, so an old `none` marker fails as an unknown account instead of meaning something else.
- **Migrating from token mode:** before installing M7, run `cam remove <name>` for each account with the old build. It deletes the Keychain item under service `cam`, which the new build no longer knows about. The new build rejects a config that still has `token_created` or `default`.

## Sharing (M8)

- **Off by default.** A new account shares nothing. That's the setup Claude Code's docs describe for several accounts, and it's the safe side for keeping work and personal apart: shared settings run the same hooks in every account, and a work org may not want personal plugins.
- **Groups, not single items,** because Claude Code keeps some things together. Seen on a dev machine with CLI 2.1.325:
  - `settings.json` holds `hooks`, `permissions`, `model`, `theme`, `enabledPlugins` and `extraKnownMarketplaces`. So plugins and hooks go with the settings.
  - Auto memory lives in `projects/<folder>/memory`, next to that folder's transcripts. So memory and history are one choice.
  - `.claude.json` holds `mcpServers` next to `oauthAccount`, trusted folders (`projects`) and first-run state (`hasCompletedOnboarding`). It can't be shared without sharing the login.
- **The groups,** each a set of links to the same names in `~/.claude`:

  | Group | Links | Covers |
  |---|---|---|
  | `settings` | `settings.json`, `keybindings.json`, `plugins/`, `hooks/` | permissions, hooks, model, theme, status line, plugins and the MCP servers they bring |
  | `instructions` | `CLAUDE.md`, `rules/` | your global instructions |
  | `extensions` | `skills/`, `agents/`, `commands/`, `output-styles/` | your own skills, agents and commands |
  | `history` | `projects/`, `history.jsonl`, `file-history/`, `todos/`, `plans/` | transcripts, `/resume`, auto memory, prompt history |

- **Never shared or copied:** MCP servers and first-run state. Copying them out of `.claude.json` would mean cam reading and writing a file of Claude Code's that can hold secrets, since MCP server configs often carry API keys. cam never writes inside Claude Code's files; it only makes links. Per account, MCP servers are added the native way (`CAM_ACCOUNT=work claude mcp add -s user …`), or come from a plugin shared through `settings`. Each account goes through the first-run screens once.
- **Choosing:**
  - `cam add <name>` shows a picker with the four groups, none checked, when stdin and stderr are terminals. Enter shares nothing.
  - `--share <groups>` takes a comma-separated list, `all` or `none`, and skips the picker. With no terminal and no `--share`, nothing is shared.
  - The choice is saved as `share = [...]` in `[accounts.<name>]`, in table order.
  - `cam share <name> <group>…` and `cam unshare <name> <group>…` change it later. `cam share <name>` alone, on a terminal, shows the picker with the current choice checked. Editing `share` by hand works too: the next launch adds the missing links.
- **Links:**
  - Absolute, to `<home>/.claude/<name>`.
  - A shared folder missing from `~/.claude` is created there, empty, then linked. Otherwise the account would make its own, and the two would never merge.
  - A shared file is linked only if it exists in `~/.claude`. cam doesn't create user files: an empty `settings.json` or `keybindings.json` isn't valid JSON. If the account writes its own copy first, that file isn't shared, and `doctor` says so.
  - Created by `add` before the login, and topped up by every launch (`exec`) and `login`, for the groups in `share`.
  - A real file or folder in the account dir is never replaced. `exec` leaves it alone, `doctor` warns and names it, and `share` refuses to share that group until it's moved away.
  - `unshare` removes the group's links and nothing else. The shared files stay in `~/.claude`, and the account starts over for that group.
  - Deleting an account never follows links.
- **File links are the weak spot.** Folder links hold, because whatever is written inside them lands in the shared folder. A file link (`settings.json`, `keybindings.json`, `CLAUDE.md`, `history.jsonl`) turns into a plain file if Claude Code saves by writing a new file and renaming it over the old one, and sharing then stops without an error. Only two write paths were checked (in the spike notes under Why another one). Before building M8, test the rest: `/config`, "always allow" rules, plugin installs and updates, `/memory` edits, keybinding changes and an auto-update. `doctor` warns when a link has become a file.

## Resolution

Order (first match wins):

1. `CAM_ACCOUNT` environment variable: an account name, or `system`. The value is trimmed, and an empty or blank value counts as unset.
2. Nearest `.claude-account`, from the current directory up to `/`. The walk starts from the physical current directory (`getcwd`), not `$PWD`, so inside a symlinked folder it walks the target's parents.
3. Nothing: **`system`**. Run the real `claude` with the environment untouched.

There's no `default` in the config. A marker in the home folder (`cam use work --dir ~`) does the same job for everything under it, and `which` shows where it came from.

The config is read only to check that a name exists. So `system` from `CAM_ACCOUNT` or a marker works even when the config is broken, and `CAM_ACCOUNT=system` is the escape hatch.

Marker file format:
- UTF-8, and a leading BOM is ignored. The first line that isn't empty once comments are stripped is the account name, trimmed. Later lines are ignored.
- `#` starts a comment anywhere on a line, so `work # day job` names `work`.
- `system` selects Claude Code's own setup for that subtree, overriding a parent marker.
- An **unknown account name is a hard error**. cam exits non-zero with a hint (`cam add <name>`) and never silently falls back to another account.
- An **unusable marker is also a hard error, never skipped**. That covers a marker that is unreadable (for example a directory), not UTF-8, names nothing (empty or only comments), or holds an invalid name. Skipping it would silently pick up a parent's account.

`cam which` prints one line. The first word is the account:
```
work (from /Users/me/Work/.claude-account)
work (from CAM_ACCOUNT)
system (from /Users/me/Work/oss/.claude-account)
system (no CAM_ACCOUNT or .claude-account)
```

## Commands

| Command | Does |
|---|---|
| `cam add <name> [--share <groups>]` | Creates the account dir, links the groups it shares (M8), runs Claude Code's login in it, and says who it logged in as. |
| `cam login [<name>]` | Runs Claude Code's login again for an account (by default, the one that applies here), and says who it logged in as. |
| `cam share <name> [<group>…]` / `cam unshare <name> <group>…` | Changes what an account shares (M8, see Sharing). |
| `cam remove <name>` | Logs the account out, then deletes its dir and config entry. The shared files are untouched. |
| `cam list` | `system` and the accounts, with the email each one is logged in as and, from M8, what it shares. |
| `cam use <name> [--dir <d>]` | Writes `.claude-account` in the current directory (or `<d>`, which must exist), containing just `<name>\n` and replacing any existing marker. Validates that the name exists. `cam use system` needs no account. |
| `cam unuse [--dir <d>]` | Removes the marker in that folder only, never a parent's. If that folder has none, it exits non-zero and names the nearest parent marker. |
| `cam which` | Resolved account and its source: env, a file path, or nothing. |
| `cam whoami` | The resolved account and who it's logged in as, from `claude auth status`. |
| `cam doctor` | Health checks (below). |
| `cam exec [--] <args…>` | Resolves, then execs the real `claude` with `<args…>` and the account's config dir. |
| `cam shim install [--dir <d>]` / `cam shim uninstall [--dir <d>]` | Creates or removes the `claude` → `cam` symlink, and says how to put it first on PATH. Never edits shell startup files. |
| `cam completions <shell>` | Prints a completion script for bash, zsh, fish, elvish or PowerShell, with account names filled in. |

### `add`
- **Checks first:** the name is valid and not `system`, the config parses, the account isn't configured and its dir doesn't exist, and the real `claude` is found (as `exec` finds it). Nothing is created before these pass.
- **Asks what to share** (M8): the picker, or `--share` (see Sharing).
- **Creates** the dir and the links of the groups it shares, then runs `claude auth login` with `CLAUDE_CONFIG_DIR` set and the terminal passed through. It passes no flags, so the user sees Claude Code's own choices and picks the account in the browser.
- **Confirms** with `claude auth status --json`: it must say `loggedIn` with an email. Then it records `[accounts.<name>]` (with `share`, if any) in the config and prints `added work: logged in as you@company.com (Acme, team)`.
- **If the login fails or is cancelled,** it runs `claude auth logout` for the dir (best effort), deletes the dir and writes no config entry. So a retry starts clean.
- **Same account twice:** if the email matches the system login's or another account's (read from their `.claude.json`), it warns that the browser may have been signed in to the wrong claude.ai account. The fix: sign the browser in to the right account, then run `cam login <name>`.

### `login`
- **Which account:** `<name>`, or the one that applies here, resolved as `exec` resolves it. cam prints the `which` line first, so logging in from the wrong folder is obvious. `system` is allowed: that's plain `claude auth login` on Claude Code's own setup.
- **Checks first:** the name is configured (or `system`), its dir exists (or it fails with the `cam add <name>` hint, as `exec` does), and the real `claude` is found. Then it tops up the links of the groups the account shares.
- **Runs** `claude auth login` with the environment `exec` would give `claude`, the terminal passed through and no flags. Claude Code replaces the account's login *(verify that it replaces one that's still valid rather than refusing)*.
- **Confirms** with `claude auth status --json` and prints `work: logged in as you@company.com (Acme, team)`, with the same-account warning as `add`.
- **If the login fails or is cancelled,** it exits non-zero and deletes nothing.
- It's the same login as `/login` in a session. `cam login` is for when no session is open: a login about to expire, a `doctor` failure, or the wrong claude.ai account after `add`.

### `remove`
- Checks that the name is valid and the config parses.
- Runs `claude auth logout` with the account's `CLAUDE_CONFIG_DIR`, which deletes its credential slot. If that fails, it stops before deleting anything, so the slot isn't orphaned.
- Then deletes the dir without following links, and then the config entry.
- What's lost is everything the account doesn't share: its MCP servers, trusted folders and org caches, and, unless it shares `history`, its transcripts and memory. Shared files stay in `~/.claude`.
- It works on partial state (only a dir, or only an entry), and fails if there's neither.

### `list`
- One row per account, `system` first: the name, and the email from `oauthAccount.emailAddress` in its `.claude.json` (`~/.claude.json` for `system`), or `-`. From M8, a third column lists the groups it shares.
- It reads files only and never runs `claude`, so it's instant. That's Claude Code's last record, not a live check. `whoami` and `doctor` ask Claude Code.

### Config writes
Via `toml_edit`, so comments and formatting survive. cam refuses to edit a config that doesn't validate and checks the result validates too. It skips the write when nothing changed, writes atomically (temp file + rename), writes through a symlink rather than replacing it, keeps an existing file's permissions, and creates `~/.config/cam/` if needed.

### `exec`
`cam exec` resolves the account for the current directory, then replaces cam with the real `claude` (`std::os::unix::process::CommandExt::exec`), so the TTY, signals and exit codes behave natively. Everything after `exec` goes to `claude` unchanged. `--` is needed only when an argument could be read as cam's own, as in `cam exec -- --help`.

- **Finding the real binary,** on **every** launch and never cached, because `~/.local/bin/claude` is a symlink the auto-updater repoints:
  1. `CAM_CLAUDE_BIN`, if set and non-empty. It must be an absolute path to an executable file and must not be cam itself. Otherwise it's an error, with no fallback to `PATH`.
  2. Otherwise the first executable `claude` on `PATH`. Three kinds of entry are skipped:
     - Relative entries, including empty ones. They mean the current directory, and a `claude` shipped inside a repo must never be what cam runs.
     - The shim dir (`~/.local/share/cam/shims`), however it's spelled.
     - Any `claude` that is cam's own executable. Files are compared by device and inode, so symlinks and hard links both count.
  - The `PATH` spelling is kept, not canonicalized, as a shell does.
- **Account resolved:**
  1. The account dir must exist, or cam fails with a `cam add <name>` hint. Then it tops up the links of the groups the account shares (see Sharing).
  2. Set `CLAUDE_CONFIG_DIR=<account dir>` and `CAM_ACTIVE_ACCOUNT=<name>`, replacing any inherited values. `CAM_ACTIVE_ACCOUNT` is for status lines and debugging.
  3. Warn on stderr for each of `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` that is set and non-empty, since it outranks the login. Cloud-provider variables and `apiKeyHelper` are left to `doctor`.
  - cam doesn't check the login before launching. If it has expired, Claude Code asks for `/login` itself, and the new login lands in the account.
- **`system`:** the environment is unchanged, with one exception. If `CAM_ACTIVE_ACCOUNT` is set, cam is running inside a session a parent cam started, and the `CLAUDE_CONFIG_DIR` it sees is that account's. Both variables are removed, so a `system` folder really gets Claude Code's own setup. A `CLAUDE_CONFIG_DIR` the user exported themselves is left alone.
- **Order:** loop guard, resolution, discovery, then the links.
- **Recursion guard:** cam sets `CAM_SHIM_DEPTH` to one more than it received (a missing or garbled value counts as 0). It refuses to launch once the incoming value reaches 10. A low limit would break legitimate nesting, because claude's Bash tool running `claude` raises the depth too. A real loop, such as a wrapper script on `PATH` that calls cam again, reaches 10 in milliseconds.
- **Exit codes:** `claude`'s own, since cam has been replaced. When cam itself fails, it exits with 127 if `claude` can't be found (including a missing `CAM_CLAUDE_BIN` target), 126 if it can't be run, and 1 otherwise. That's the convention `env` and the shell use.
- Like `execvp`, cam hands a `claude` that the kernel can't run directly (one without a shebang) to `/bin/sh`.

### Shim (busybox style, M4)
- **Shim mode:** if `argv[0]`'s basename is `claude`, cam behaves exactly like `cam exec -- <args…>`. Every argument is passed on unparsed, so `claude --help` and even `claude exec` reach claude. Errors still start with `cam:`, so it's clear which program failed.
- **Install:** `cam shim install [--dir <d>]` creates `<dir>/claude -> cam`.
  - The default dir is `~/.local/share/cam/shims`. A relative `--dir` is resolved against the current directory, and the dir is created if needed.
  - The link is made under a temporary name and renamed into place, so the swap is atomic.
- **Link target:** the `cam` on `PATH` when it's this same binary, else cam's own executable. The `PATH` spelling, such as `/opt/homebrew/bin/cam` or `~/.cargo/bin/cam`, survives package-manager upgrades, where a canonical path into a versioned directory wouldn't.
- **Only cam's own links are touched.** `install` and `uninstall` act only on a `<dir>/claude` that is missing or is a symlink to a cam binary: this one, or any file named `cam`, such as an old build, even if the link is dangling. Anything else is an error, so running with `--dir ~/.local/bin` by mistake can't replace the real `claude`.
- **PATH:** cam never edits shell startup files. After installing, it looks at the current `PATH`:
  - **Shim dir first:** done. It suggests `hash -r` in case the shell remembers the old `claude`.
  - **Not on `PATH`:** it prints the line to add, for `$SHELL`: `~/.zshrc`, `~/.bashrc`, fish's `config.fish`, or a POSIX line for anything else. It says to put that line below the one that adds the real `claude`'s dir, which is usually `~/.local/bin`, where the native installer puts `claude`.
  - **On `PATH` but after another `claude`:** it says to move the line.
  - Paths under the home directory are printed as `$HOME/…`, so the line can go into dotfiles. If no real `claude` is found yet, it adds a note.
- **Uninstall:** `cam shim uninstall [--dir <d>]` removes the link, and notes when the dir is still on `PATH`. If there's no shim, it's an error, as with `unuse`.
- **Loops:** discovery skips the default shim dir and any `claude` that is this cam, so a shim never finds itself. A shim in a custom `--dir` is caught by the same-file check, and the recursion guard covers the rest (see `exec`).
- **Escape hatch:** `CAM_ACCOUNT=system claude` runs Claude Code untouched, even with a broken config, because `system` never reads it.
- *(verify)* Claude Code's own `/doctor` or its updater may notice that the `claude` on `PATH` is a symlink to another program.

### `whoami`
- Prints the `which` line, then what `claude auth status --json` reports, run with the environment `exec` would give `claude`:
  ```
  work (from /Users/me/Work/.claude-account)
    email:   you@company.com
    org:     Acme (team)
    login:   expires 2026-12-13 (in 28 days)
  ```
- Because it asks Claude Code in claude's own environment, an `ANTHROPIC_API_KEY` that outranks the login shows up as what Claude Code would really use. It also warns about it on stderr, after the report.
- Inside a cam session (`CAM_ACTIVE_ACCOUNT` set), it adds which account that session runs as.
- From M8, a `shares:` line lists the account's groups.
- Not logged in, or expired: it prints the report, then fails with the `cam login <name>` hint.

### Completions
- **Static scripts from `clap_complete`,** not its run-time mode. That mode sits behind an `unstable-dynamic` feature whose API can change between minor versions. With static scripts, nothing runs cam while completing.
- **Account names are baked in** when the script is generated, as the values of `use` and `login` (plus `system`), and of `remove`, `share` and `unshare`. The group names after `share` and `unshare` are fixed. That's done on a copy of the command used only for generation, so parsing still accepts any name.
- **Loaded at shell start,** with `eval "$(cam completions zsh)"` and the like, rather than saved to a file. New accounts appear in the next shell. `eval` rather than `source <(…)`, because macOS's bash 3.2 can't source a process substitution.
- **Quiet:** a missing or broken config gives a script without names and no error, since this runs at every shell start.
- **fish:** clap's fish script completes no positional values, so cam appends one `complete` line per value, using the `__fish_cam_using_subcommand` function clap's script defines. Not tried in a real fish *(verify)*.
- Tested: in bash 3.2, by calling the completion function with `COMP_WORDS`. In zsh, through a pseudo-terminal during development (not in the test suite).
- The output is redacted like everything else.

### Doctor checks
`cam doctor` prints one line per check, marked `ok`, `info`, `warn` or `fail`, with `hint:` lines indented below. Then comes a tally (`no problems found`, `1 warning`, `2 problems, 1 warning`). Any `fail` makes it exit 1, with the tally as the error; warnings alone exit 0. It runs `claude auth status` once for `system` and once per account (about 0.4 s each), and never reads a credential. In order:
- **claude:** the real `claude`, found as `exec` finds it.
- **shim:** what plain `claude` runs: the first executable `claude` on `PATH`. `ok` if that's a cam shim or cam itself. `warn` if a shim is installed in the default dir but isn't reached, with a pointer to `cam shim install`, which prints the line to fix. `info` if there's no shim: it's optional, and `cam exec` works without it.
- **config:** parses, with the number of accounts. A missing file or no accounts is `info`.
- **system:** who Claude Code's own setup is logged in as. Not logged in is `info`, since someone may use only cam accounts.
- **account `<name>`,** one per configured account:
  - The dir exists (`fail` if not).
  - From M8, for each group the account shares, each entry is a link to its place in `~/.claude`. A missing link is `warn` (the next launch adds it). A real file or folder in its place is `warn`, naming it: that part isn't shared.
  - The login: `ok` with the email and expiry, `warn` within 3 days of expiry (when Claude Code starts warning too), `fail` if expired or not logged in. The hint is `cam login <name>`.
- **same account:** two accounts, or an account and `system`, logged in as the same email: `warn`.
- **here:** the folder's resolution, as `which` prints it, or its error.
- **environment:** in this shell, the variables that outrank a login: `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` (set and non-empty), and `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` and `CLAUDE_CODE_USE_FOUNDRY` when on (`1`, `true`, `yes` or `on`, the values the docs give for boolean variables). Also `CLAUDE_CONFIG_DIR` exported outside a cam session: `warn`, since `system` then isn't `~/.claude`, while accounts still share from `~/.claude`.
- **settings:** Claude Code's settings files, scanned for `apiKeyHelper` and, in `env`, the same variables. Claude Code writes `env` entries over the inherited environment, so these count as if exported (https://code.claude.com/docs/en/settings.md). The files checked:
  - The user settings of the account that applies here: `~/.claude/settings.json` for `system`, or the account's `settings.json`, which is a link to that same file when it shares `settings`.
  - `.claude/settings.json` in the current directory.
  - `.claude/settings.local.json` at the git root, else in the current directory.
  - A file that can't be read or isn't JSON is a warning. Managed settings aren't checked: remote ones can't be read.
- **Only names are printed, never values:** `env` values and `apiKeyHelper` commands can be secrets.

`claude auth status --json` output shape, observed with CLI 2.1.322 and 2.1.325:
```json
{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
 "configDirectory": "...", "loginExpiresAt": "2026-11-16T15:59:25.959Z",
 "email": "...", "orgId": "...", "orgName": "...",
 "subscriptionType": "max", "planTier": "max_20x", "storedClaudeAiLogin": true}
```

## Storage

- **Config:** `$CAM_CONFIG`, else `~/.config/cam/config.toml`. It contains no secrets.
  ```toml
  [accounts.work]
  share = ["settings", "extensions"]

  [accounts.personal]
  ```
  - An empty `CAM_CONFIG` counts as unset. The fallback really is `~/.config/cam/config.toml` on macOS as well (not `~/Library/Application Support`), and `XDG_CONFIG_HOME` is not consulted.
  - A missing file is an empty config: no accounts.
  - The config is **validated strictly** every time it's read. Unknown keys are errors, so a typo is never silently ignored. Account names follow the name rule and can't be `system`.
  - In M7 an account table has no keys. M8 adds `share`, the groups the account shares (see Sharing). A missing `share` means none, and an unknown group is an error, like any unknown key.
- **Account dirs:** `~/.local/share/cam/accounts/<name>/`, built from the home dir and always spelled the same way, because the credential slot is keyed to the spelling.
- **Credentials:** Claude Code's own. cam never reads, writes or lists them. It only runs `claude auth login`, `logout` and `status`.

## Security rules

- cam handles no credentials. It never reads Claude Code's Keychain items or credential files, and never puts anything secret in argv or the environment.
- It parses `claude auth status --json` and prints only the email, org, plan and expiry, never the raw output.
- Redact anything matching `sk-ant-[A-Za-z0-9_-]+` in any output cam produces. cam's own stdout, stderr and clap usage errors all go through redaction, which covers a token pasted into a marker or into `CAM_ACCOUNT` by mistake.
- Marker files contain only names. A hostile repo can at most pick another one of **your own** accounts, or `system`. A later `doctor` check could flag markers tracked in git.
- Account dirs are 0700: their `.claude.json` holds MCP server configs, which can carry secrets.
- Sharing is opt-in, one group at a time, and a shared group is the same for every account that shares it. With `history`, work transcripts and memory sit in `~/.claude/projects` next to personal ones. With `settings`, the same hooks run in every account. An org's managed settings and policy caches always stay in its own account.
- cam never writes inside Claude Code's files. Sharing only adds links in the account dir, and empty folders in `~/.claude`.

## Crates

`clap` (derive), `clap_complete`, `serde` + `toml` for reading, `toml_edit` for writing, `serde_json` (for `auth status` and `.claude.json`), `dirs`, `anyhow`, `thiserror` (typed errors that tests can match on). Dates use a small built-in civil-date routine instead of a date crate. For tests: `assert_cmd`, `predicates`, `tempfile`. M7 drops `security-framework` and `rpassword`. M8 adds a multi-select picker, such as `dialoguer`.

## Testing

- **Tests never run the real `claude`.** A fake `claude` script on the sandbox `PATH` handles `auth login` (writes a fake `oauthAccount` into `$CLAUDE_CONFIG_DIR/.claude.json`), `auth status --json` (reports from that file) and `auth logout` (removes it). It also prints its argv, `CLAUDE_CONFIG_DIR`, `CAM_ACTIVE_ACCOUNT` and `CAM_SHIM_DEPTH`. Failure modes, like a cancelled login, are switched on by files in the sandbox.
- The sandbox clears the environment and sets its own `HOME` and `PATH`, so the real `~/.claude`, the real `claude` and the real Keychain are out of reach.
- **Resolution unit tests** on temp directory trees: nearest wins, `system` overrides a parent, env beats file, invalid or unknown names fail.
- **Sharing (M8):** only the chosen groups are linked, links are topped up on launch, a real file is never replaced, `unshare` removes only links, and `remove` never follows one, so the shared targets survive. Tests choose through `--share` and a stdin that isn't a terminal. The picker itself isn't driven by tests.
- **PATH discovery** and the **shim end to end,** as before: fake `claude` scripts, a shim symlink earlier on PATH, `argv[0]` really `claude`, and `install`/`uninstall` refusing a real `claude`.
- **`whoami` and `doctor`:** the check logic is unit tested without I/O. End to end, settings files and variables hold fake secrets, and the tests assert that no value is ever printed.
- **Manual acceptance:** after `cam add work`, `/status` in a folder marked `work` shows the work account, and outside it shows the system login. The Keychain has the slot named above. From M8, with every group shared, the links are still links after a session.

## Milestones

1. Resolution, `which`, `use`/`unuse` and the config file.
2. `SecretStore` (Keychain + test store), `add`/`list`/`remove`. *(token mode)*
3. `exec`: real-binary discovery, env injection, conflict warnings.
4. Shim mode, `shim install`/`uninstall`.
5. `whoami` and `doctor`.
6. Shell completions.
7. **Native logins (next):** an account is an empty config dir with its own login, and shares nothing yet. `add`/`login`/`remove` through `claude auth`, `system` in place of `none`, no `default`, `list`/`whoami`/`doctor` from Claude Code's own record. The Keychain store, the token prompt and `token_created` go.
8. **Sharing:** first, test which write paths keep a file link. Then the four groups, the picker and `--share`, `share`/`unshare`, links topped up on launch, and `doctor`'s link checks.
9. A release (cargo-dist / Homebrew tap). Not planned yet.

Later ideas: Linux (the design no longer needs a secret store, so it may just work *(verify)*), a prompt/status-line snippet that reads `CAM_ACTIVE_ACCOUNT`, and a check of IDE extensions (they may bundle their own CLI and bypass the PATH shim *(verify)*).

## Open questions

- **Do file links survive every write?** The first step of M8 (see Sharing). If `settings.json` doesn't, the `settings` group may need another approach, or a warning that it can come apart.
- **Should `remove` ask first** when the account has transcripts and memory of its own, that is, when it doesn't share `history`?
- **Settings without plugins:** sharing `settings` brings `enabledPlugins`, so `plugins/` comes with it. Is a group for plugins alone worth having?
- **Login lifetime:** does an account that isn't used expire after about a month, while a used one keeps renewing *(verify)*?
- **Remote Control** in an account dir *(verify)*.
- **An exported `CLAUDE_CONFIG_DIR`:** should accounts share from it instead of `~/.claude`? For now they share `~/.claude`, and `doctor` warns.
- **The org caches token mode left in `~/.claude`:** do system sessions apply them? If not, they can just be deleted.
- **Logical vs physical cwd:** should the walk-up honour `$PWD` when it points at the current directory, as `pwd -L` does? Then markers above a symlinked folder would apply. M1 uses the physical path.
- Do the VS Code and JetBrains extensions honour a `claude` on PATH?
- **Name (settled 2026-11-11):** renamed from `clam` to `cam`. On crates.io, `clam` is a template engine, and [`clam-cli`](https://crates.io/crates/clam-cli) ([jose2kk/clam](https://github.com/jose2kk/clam)) is also a Claude Code profile manager. The crate `cam` is taken by an unrelated camera library, so the package is `cam-cli` and the binary is `cam`. `Cargo.toml` keeps `publish = false` until a release. When Linux support comes, check for a clash with libcamera's `cam` utility.
- **Decided 2026-11-15:**
  - Native logins replace token mode (see Token mode).
  - `none` becomes `system`, because it's your normal login, not "no account".
  - `default` goes, along with the planned `cam default`: a marker in the home folder covers it.
  - No `--email`: the account name is the label, and the real email comes from Claude Code. `add` doesn't pass `--email` to the login either.
  - `cam login [<name>]` runs Claude Code's login again for an account that already exists. It needs no state of its own, and `/login` in a session does the same.
  - Accounts start empty and share nothing by default. Sharing is by group (`settings`, `instructions`, `extensions`, `history`), chosen in `add` with a picker or `--share`, and changed with `share`/`unshare`. Links keep the groups in sync.
  - MCP servers and first-run state are neither shared nor copied, because cam never writes inside Claude Code's files.
  - M7 ships accounts without sharing, and all link code moves to M8.
