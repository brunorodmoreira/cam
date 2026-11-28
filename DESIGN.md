# cam — Claude Account Manager

Status: milestones 1–8 built. Accounts log in with Claude Code's own login (M7, 2026-11-15) and share parts of `~/.claude` on request (M8, 2026-11-23). No release yet. · Written 2026-11-11, redesigned 2026-11-15

Pick the Claude Code account **per folder**, the way `.nvmrc` / `.node-version` pick a Node version with fnm. A `.claude-account` file names an account. The nearest one up the folder tree wins. `claude` starts logged in as that account. Each account starts with a Claude Code setup of its own, and you choose which parts of your usual setup it shares.

## Why another one

Existing tools ([Nemo-Illusionist/claude-code-account-switcher](https://github.com/Nemo-Illusionist/claude-code-account-switcher), [mtxr/claude-switch](https://github.com/mtxr/claude-switch), [realiti4/claude-swap](https://github.com/realiti4/claude-swap), [diananerd/claude-account-switcher](https://github.com/diananerd/claude-account-switcher), [fairy-pitta/cc-account-switcher](https://github.com/fairy-pitta/cc-account-switcher)) switch **`CLAUDE_CONFIG_DIR`** to a separate folder per account. That's Claude Code's documented way to stay logged in to several accounts, but it also splits settings, plugins, hooks, memory and history per account.

cam uses a config dir per account too. What it adds is picking the account per folder, and letting each account share chosen parts of your setup in `~/.claude` through links that stay in sync.

cam's differentiators:

1. **A portable marker file with walk-up lookup**, like fnm's `--version-file-strategy=recursive`. One file at a parent folder covers every nested repo, so nothing is committed into the repos themselves.
2. **Claude Code's own login.** Each account logs in through Claude Code's normal flow: `cam add` and `cam login` run `claude auth login` in the account's dir, and `/login` in a session works too. cam never sees, stores or passes a credential. Sessions get the whole product: the plan in the header, `/usage` limits and claude.ai connectors.
3. **Sharing you choose.** An account starts empty, the way Claude Code's docs set up several accounts. You can share whole groups of your setup with it: settings, instructions, skills, history. They're symlinked from `~/.claude`, so they stay in sync.
4. **Resolved at launch, scoped to one process.** There's no `cd` hook. `CLAUDE_CONFIG_DIR` goes only into the environment of the `claude` process cam starts.
5. **It explains itself.** `cam which` says which account was chosen and why. `cam whoami` adds who that account is logged in as, according to Claude Code. `cam doctor` checks the setup, the links and anything that would make Claude Code ignore the login.

Docs facts this design relies on:
- "To stay signed in to multiple accounts at once, such as work and personal accounts, give each account its own configuration directory." (https://code.claude.com/docs/en/authentication.md#log-in-with-multiple-accounts)
- `CLAUDE_CONFIG_DIR` moves everything Claude Code keeps: "All settings, session history, and plugins are stored under this path." That includes `.claude.json`, which otherwise sits at `~/.claude.json`, and with it the user-scope MCP servers and per-project trust. Sharing parts across config dirs isn't documented (https://code.claude.com/docs/en/claude-directory.md).
- `CLAUDE_CONFIG_DIR` is **ignored in project/local `.claude/settings.json`**. That's why an external launcher is needed at all (verified 2026-11-11).
- Claude Code warns 3 days before a login expires ("Your login expires in 3 days · run /login to renew"), and asks for `/login` once it has (https://code.claude.com/docs/en/authentication.md#renew-an-expiring-login).
- Cloud-provider switches, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `apiKeyHelper` and `CLAUDE_CODE_OAUTH_TOKEN` all **outrank** a `/login` (https://code.claude.com/docs/en/authentication.md#authentication-precedence).
- For `ANTHROPIC_API_KEY` only: "In interactive mode, you are prompted once to approve or decline the key, and your choice is remembered. To change it later, use the "Use custom API key" toggle in `/config`. ... In non-interactive mode (`-p`), the key is always used when present." The docs don't say where the choice is kept. (same page, checked 2026-11-15)
- "Separate directories don't keep two Claude Console sign-ins [without an API key] apart, because Claude Code stores that kind of sign-in outside the configuration directory." (https://code.claude.com/docs/en/authentication.md#log-in-with-multiple-accounts) So a cam account can't hold a Console login.
- `claude auth login` takes `--email <email>` (fills in the login page), `--sso`, `--console` and `--claudeai` (the default). `auth status`'s `authMethod` is one of `none`, `claude.ai`, `oauth_token`, `api_key`, `api_key_helper` or `third_party` (https://code.claude.com/docs/en/cli-reference.md).

Observed with CLI 2.1.325 (spike, 2026-11-14):
- In a config dir linked to all four sharing groups (see Sharing), `claude auth login --email <work email>` opened the browser with the email filled in. Claude Code printed "Your organization may apply global configuration...". `auth status` then showed the work email, the org, `subscriptionType: "team"` and a `loginExpiresAt` 29 days out.
- A session in that dir showed "Claude Team" in the header. `/status` showed the org and email, with "User settings" and "Enterprise managed settings (remote)" as setting sources. The claude.ai connectors were connected. A plugin's MCP server needed its own login, and the user-scope MCP servers from `~/.claude.json` were absent.
- The shared parts worked: the user settings and plugins loaded, and the session's transcript went into the shared `~/.claude/projects`.
- Nothing leaked either way: `~/.claude/remote-settings.json` wasn't rewritten, and the system login and `~/.claude.json`'s `oauthAccount` were untouched.
- First-run screens appeared before the session started (which ones wasn't recorded).
- Deleting the account dir after `claude auth logout` left everything in `~/.claude` intact.
- Earlier, in a scratch config dir: `claude mcp add -s user` and `claude plugin install` wrote through a symlinked `.claude.json` and `settings.json`, and the links survived.
- The login is stored as a generic password with service `Claude Code-credentials`, plus `-<first 8 hex digits of sha256(CLAUDE_CONFIG_DIR)>` when the variable is set. Seen in the Keychain for two accounts added with `cam add` (2026-11-15). So the dir's spelling matters, and cam must always pass the same one.
- The system login's `loginExpiresAt` moved from 2026-11-16 (seen 2026-11-12) to 2026-12-11, so it seems to be extended as the login is used *(verify for an idle account)*.

Observed with CLI 2.1.325 in an empty config dir (2026-11-15):
- `claude auth status --json` exits 1 when not logged in, and still prints its JSON: `loggedIn: false`, `authMethod: "none"`. So cam reads the JSON whatever the exit code.
- With `ANTHROPIC_API_KEY` set, it reports `loggedIn: true`, `authMethod: "api_key"` and `apiKeySource: "ANTHROPIC_API_KEY"`, with no email.
- `claude auth logout` in a dir that never logged in succeeds. Running these leaves `.claude.json`, `.claude.json.lock` and `backups/` in the dir.
- A `SessionStart` hook saw `CAM_ACTIVE_ACCOUNT`, `CAM_SHIM_DEPTH` and `CLAUDE_CONFIG_DIR` from claude's environment (a logged-out `claude -p` in a scratch config dir). In the CLI's code, the status line command runs through the same runner as hooks, whose environment starts from `process.env`, minus a few of Claude Code's own variables (and, in a scrubbing mode, names and values that look like secrets), plus `CLAUDE_PROJECT_DIR`, `COLUMNS` and `LINES`. So a status line can show `$CAM_ACTIVE_ACCOUNT`; `docs/usage.md` has the snippet. Seen working in a session on 2026-11-23 (below).

Read in the code of CLI 2.1.330 (2026-11-23), for the file links of the sharing groups:
- **`settings.json`:** changes made in a session (`/config`, `/model`, permission rules, plugin installs and toggles) go through one update function. For user settings it calls the atomic-write helper with `allowSymlink`: the helper reads the link, writes a temp file and renames it onto the link's target, so the link stays. Without `allowSymlink` the helper refuses a link with an error ("Refusing to write through symlink") instead of replacing it. A newer storage backend writes user settings with a `followAtomic` discipline, whose code wasn't found. It's used only when `CLAUDE_CODE_HOVER_REST` is set, which happens in remote sessions and some background ones *(verify)*.
- **`keybindings.json` and `CLAUDE.md`:** Claude Code never rewrites these. `/keybindings` and `/memory` create the file only when nothing is at the path (`flag: "wx"`, which also fails on a link), then open `$VISUAL` or `$EDITOR`. So the editor decides whether the link survives. Stock vim (`-u NONE`) wrote through a link and kept it.
- **`history.jsonl`:** prompts are appended, and appending follows the link. The retention prune skips a file that isn't a regular one ("History retention prune skipped: history.jsonl is not a regular file"), so only sessions of the system login prune the shared history. `claude project purge` rewrites the file, and that path wasn't checked.
- **`.claude.json`:** its writes pass `allowSymlink` too, though cam doesn't share it.

Observed with CLI 2.1.330 in a session (2026-11-23). A throwaway account was added with `cam add`. Each of the four groups' 15 entries was linked by hand to a scratch copy, not to `~/.claude`.
- After a session with `/model`, `/config` (theme), `/permissions` (an allow rule saved to user settings) and a `/plugin` install, all 15 entries were still links. The shared `settings.json` had gained `model`, `theme`, `permissions` and `enabledPlugins`, written through the link. Prompts went into the shared `history.jsonl`. The plugin went into the shared `plugins/` and the transcript into the shared `projects/`. The account dir itself gained only `cache/` and `sessions/`.
- The status line snippet from the docs showed the account's name.
- Zed 1.29 saved `CLAUDE.md` and `keybindings.json` through their links, and kept them.
- `cam remove` logged the account out, which deleted its Keychain slot. It deleted the dir without following a link: the scratch copies were intact afterwards.
- Not exercised: an "always allow" answer to a permission prompt (auto mode didn't ask), a plugin update, an auto-update, `claude project purge`, and sessions on the storage backend.

Rejected: the undocumented `CLAUDE_SECURESTORAGE_CONFIG_DIR`, which separates only the credential slot. With a shared `.claude.json`, a refresh merges one account's profile fields into the other's `oauthAccount` without checking `accountUuid`, and so does the org-roles fetch. It is also a hazard when exported: in Claude Code 2.1.331 it takes the place of `CLAUDE_CONFIG_DIR` in where the login is stored (the Keychain entry's name on macOS, the folder for `.credentials.json` elsewhere), and any value, even an empty one, applies. So every account would use one login slot. `exec`, `add`, `login`, `whoami` and `doctor` warn about it.

Read in Claude Code 2.1.331 and its VS Code extension (2026-11-24), for the IDE and Linux questions below:

- **IDEs:** the extension writes its lock file to `$CLAUDE_CONFIG_DIR/ide/` of the editor's own environment, which is `~/.claude/ide/` unless the editor was started with that variable. With `CLAUDE_CONFIG_DIR` set, the CLI looks in both `<config dir>/ide/` and `~/.claude/ide/`, so `/ide` finds the editor from an account's session in an editor's terminal. cam needs no `ide` link.
- **Linux:** there is no keyring support: the login is a plaintext `.credentials.json` (Claude Code warns "Storing credentials in plaintext") in the same folder that names the macOS Keychain entry, `CLAUDE_CONFIG_DIR` unless `CLAUDE_SECURESTORAGE_CONFIG_DIR` is set. So accounts keep separate logins there too. A report from April 2026 (anthropics/claude-code#30031) said otherwise, likely on an older version. Not yet tried with two real logins on Linux.
- **The extension's own panel:** it runs its bundled `claude` directly, not the one on `PATH`, so cam doesn't reach it. Its machine-scoped `claudeCode.claudeProcessWrapper` setting names an executable to run instead, with the bundled binary as the first argument and the workspace folder as the working directory. That could put the panel's agent on the folder's account, but the extension's own side (its session list, the MCP servers it adds, its plugins) still uses `~/.claude`. Not built yet *(verify by hand first)*.

### Token mode (M1–M6, dropped 2026-11-15)
cam first passed a long-lived `claude setup-token` token as `CLAUDE_CODE_OAUTH_TOKEN`, with the token stored in the macOS Keychain under service `cam`. It was dropped because:
- Claude Code can't see the plan in token mode. The header said "Claude API" and `/usage` showed a dollar estimate, though usage did count against the plan's limits. claude.ai connectors and Remote Control don't work with the token.
- `auth status` reported no identity (`authMethod: "oauth_token"`, `loginExpiresAt: null`), so cam couldn't tell whose a token was. `--email` only recorded what the user typed, and was removed first.
- Every rebuild of cam brought back a Keychain prompt, and pasting the token into a hidden prompt was easy to get wrong.
- The org's cached managed settings ended up in the shared folder. Token sessions for a work account wrote `remote-settings.json` and `policy-limits.json` (with their `.signature`/`.stamp` files) into `~/.claude`, next to the system login's setup. Sessions of the system login never refreshed them afterwards, so they were leftovers. With per-account dirs these caches stay in the account.

## Accounts

- **`system`** is Claude Code's own setup: `~/.claude`, `~/.claude.json` and its own `/login`. cam never changes it, except to create an empty folder in it that an account shares (see Sharing). It's what runs where no marker applies.
- **A cam account `<name>`** is a folder, `~/.local/share/cam/accounts/<name>/` (mode 0700), that cam passes as `CLAUDE_CONFIG_DIR`.
  - It starts empty. Claude Code fills it on the first login and session, as it does any new config dir, and the first session shows Claude Code's first-run screens.
  - It shares only the groups chosen for it (see Sharing). Nothing, by default.
  - Always its own, shared or not: `.claude.json` (the account's identity, user-scope MCP servers, trusted folders and first-run state), its login, the org's managed settings and policy caches, and run-time state (`sessions`, `session-env`, `shell-snapshots`, `cache`, `stats-cache.json` and the like).
- **Names** match `^[a-z0-9][a-z0-9_-]*$`. `system` is reserved.

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
  | `skills` | `skills/`, `agents/`, `commands/`, `output-styles/` | your own skills, agents, slash commands and output styles |
  | `history` | `projects/`, `history.jsonl`, `file-history/`, `todos/`, `plans/` | transcripts, `/resume`, auto memory, prompt history |

- **Group names** say what's inside. `skills` was `extensions` in the first draft, which read like plugins, and plugins are in `settings`.
- **Never shared or copied:** MCP servers and first-run state. Copying them out of `.claude.json` would mean cam reading and writing a file of Claude Code's that can hold secrets, since MCP server configs often carry API keys. cam never writes inside an existing file of Claude Code's; it only makes links, and the empty files and folders described under Links. Per account, MCP servers are added the native way (`CAM_ACCOUNT=work claude mcp add -s user …`), or come from a plugin shared through `settings`. Each account goes through the first-run screens once.
- **Choosing:**
  - `cam add <name>` shows a picker with the four groups, none checked, when stdin and stderr are terminals. Enter shares nothing.
  - The picker says what each group would share today, from a look at `~/.claude`: which of its files exist, and how many entries each folder holds (`settings: settings.json, plugins/ (4), hooks/ (2)`). Above the list, one line says what sharing means both ways: a shared group is the same files for both, so a change in either account shows up in the other. Space ticks, Enter confirms, and Esc cancels: `add` then creates nothing.
  - `--share <groups>` skips the picker. With no terminal and no `--share`, nothing is shared.
  - The choice is saved as `share = [...]` in `[accounts.<name>]`, in table order.
  - `cam share <name> <group>…` and `cam unshare <name> <group>…` change it later. `cam share <name>` alone, on a terminal, shows the picker with the current choice checked. Unticking a group there stops sharing it, as `unshare` does without `--copy`. Editing `share` by hand works too: the next launch adds the missing links.
  - **One syntax everywhere:** groups can be comma-separated, separate words or repeated flags, and the forms mix: `--share settings,skills`, `--share settings --share skills`, `cam share work settings skills`, `cam share work settings,skills`. `all` means the four groups in each of them, `cam unshare work all` included. `none` is only for `--share`, where it skips the picker.
- **Links:**
  - Absolute, to `<home>/.claude/<name>`.
  - A shared folder missing from `~/.claude` is created there, empty, then linked. Otherwise the account would make its own, and the two would never merge.
  - A shared file missing from `~/.claude` is created there when its empty form is valid, then linked: `settings.json` as `{}`, `CLAUDE.md` and `history.jsonl` empty. Otherwise the account would write its own copy first, and sharing would never start. `keybindings.json` is linked only if it exists, since cam doesn't know its empty form; until then, `doctor` says it isn't shared.
  - Created by `add` before the login, and topped up by every launch (`exec`) and `login`, for the groups in `share`.
  - A real file or folder in the account dir is never replaced at launch. `exec` leaves it alone and warns once per launch, naming it: `work's settings.json is a file, not a link to ~/.claude/settings.json, so work doesn't share it`, with `cam share work settings` as the fix. That's also how a file link that Claude Code replaced by renaming over it shows up. `doctor` warns the same way.
- **When the account has its own copy,** `share` moves it aside instead of refusing, since an account that has been used always has some (its `settings.json` at least):
  - It lists what it would move, and asks on a terminal. `--yes` skips the question; with no terminal and no `--yes`, it fails and names the entries.
  - Each entry is renamed in the account dir to `<name>.before-share-<date>`, such as `settings.json.before-share-2026-11-20`, with `-2`, `-3` and so on if that's taken. Then the link takes its place.
  - Nothing is deleted. It prints where each entry went, so the user can merge by hand: copy a setting across, or move a folder's contents into `~/.claude`.
  - `doctor` lists `*.before-share-*` entries as `info`, so they're not forgotten.
- **`unshare`** removes the group's links and nothing else, and says exactly what it removed: `work no longer shares settings: removed the links settings.json, plugins, hooks. Your files stay in ~/.claude, and work starts over for these.` A link that was never made, or a real file in its place, is named as skipped.
  - `--copy` replaces each link with a copy of what it pointed to, so the account keeps the current setup and goes its own way from there. Folders are copied recursively without following links inside them. For `history`, it says how much it's about to copy first, since `projects/` can be large.
- Deleting an account never follows links.
- **File links are the weak spot.** Folder links hold, because whatever is written inside them lands in the shared folder. A file link (`settings.json`, `keybindings.json`, `CLAUDE.md`, `history.jsonl`) turns into a plain file if something saves it by writing a new file and renaming it over the old one, and sharing then stops without an error. Claude Code's own writes keep the links, from a reading of its code and a session with every group linked (in the notes under Why another one). That leaves the user's editor, which `/memory` and `/keybindings` open on `CLAUDE.md` and `keybindings.json` (vim and Zed keep links), and the paths not yet exercised, such as plugin updates and auto-updates. `exec` and `doctor` warn when a link has become a file, so a break shows up at the next launch.

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
- An **unknown account name is a hard error**. cam exits non-zero and never silently falls back to another account. The hint depends on the name:
  - a typo of a configured account (at most 2 edits, and fewer edits than the name has letters): `did you mean \`work\`?`, with the fix: `cam use work --dir <marker's dir>` for a marker, `CAM_ACCOUNT=work` for the variable;
  - anything else: `cam add <name>`.
- An invalid name gets a valid one close to it as a hint, when there is one: `Work` → `work`, `my account` → `my-account`.
- An **unusable marker is also a hard error, never skipped**. That covers a marker that is unreadable (for example a directory), not UTF-8, names nothing (empty or only comments), or holds an invalid name. Skipping it would silently pick up a parent's account.

`cam which` prints one line. The first word is the account:
```
work (from /Users/me/Work/.claude-account)
work (from CAM_ACCOUNT)
system (from /Users/me/Work/oss/.claude-account)
system (no CAM_ACCOUNT or .claude-account)
```

`cam which --short` (2026-11-27) is for a shell prompt, which runs it every time: just the account's name, nothing for `system`, and `cam?` when the resolution fails or the config path can't be found, so a broken marker shows before `claude` fails on it. It always exits 0 and never writes to stderr. docs/usage.md has a Starship custom module for it, which Starship 1.27 rendered as `as ✻ work` in about 6 ms. Starship's `detect_files` looks only in the current directory, while a marker usually sits higher up, so the module runs the command at every prompt (`when = true`) and hides itself on empty output.

## Commands

| Command | Does |
|---|---|
| `cam add <name> [--share <groups>] [-- <login args>]` | Creates the account dir, links the groups it shares (M8), runs Claude Code's login in it, and says who it logged in as. |
| `cam login [<name>] [-- <login args>]` | Runs Claude Code's login again for an account (by default, the one that applies here), and says who it logged in as. |
| `cam share <name> [<group>…] [--yes]` / `cam unshare <name> <group>… [--copy]` | Changes what an account shares (see Sharing). `share` adds to what it shares, and also links again a group it already shares. |
| `cam remove <name> [--yes]` | Asks, then logs the account out and deletes its dir and config entry. The shared files are untouched. |
| `cam list` | `system` and the accounts, with the email each one is logged in as and, from M8, what it shares. |
| `cam use <name> [--dir <d>]` | Writes `.claude-account` in the current directory (or `<d>`, which must exist), containing just `<name>\n` and replacing any existing marker. Validates that the name exists. `cam use system` needs no account. Says what it replaced (`(work, was personal)`), and notes when the folder is in a git repository. A leading `~` in `<d>` is the home dir, since shells leave it alone in `--dir=~/x`. |
| `cam unuse [--dir <d>]` | Removes the marker in that folder only, never a parent's. If that folder has none, it exits non-zero and names the nearest parent marker. `~` as for `use`. |
| `cam which [--short]` | Resolved account and its source: env, a file path, or nothing. `--short` is for a shell prompt (see Resolution). |
| `cam whoami` | The resolved account and who it's logged in as, from `claude auth status`. |
| `cam doctor` | Health checks (below). |
| `cam exec [--] <args…>` | Resolves, then execs the real `claude` with `<args…>` and the account's config dir. |
| `cam init <bash\|zsh\|fish>` | Prints the shell code for the end of the startup file: it keeps the `claude` → `cam` link, puts its dir first on `PATH` and loads completions (see Plain `claude`). |
| `cam completions <shell>` | Prints a completion script for bash, zsh, fish, elvish or PowerShell, with account names filled in. |

### `add`
- **Checks first:** the name is valid and not `system`, the config parses, nothing is at the account's dir yet, and the real `claude` is found (as `exec` finds it). Nothing is created before these pass. A config entry without a dir is fine: `add` sets that account up again, which is what the hint for a missing dir says to do.
- **Asks what to share** (M8): the picker, or `--share` (see Sharing).
- **Creates** the dir and the links of the groups it shares.
- **Says which account the login will use,** on stderr: `Logging work in with Claude Code. It uses the claude.ai account your browser is signed in to.`, then `Already in use: me@example.com (system), you@company.com (personal).` from each `.claude.json`, so a browser still signed in to one of those is caught before the login rather than after.
- **Runs** `claude auth login` with `CLAUDE_CONFIG_DIR` set and the terminal passed through. Arguments after `--` go to it unchanged, such as `--email <address>` or `--sso`, so the user sees Claude Code's own choices and picks the account in the browser.
- **While the login runs,** cam ignores Ctrl-C and Ctrl-\\, as `system(3)` does. They still reach `claude`, which stops, and cam is left to clean up.
- **Confirms** with `claude auth status --json`, minus the variables that outrank a login, so it checks the login itself: it must say `loggedIn` with `authMethod: "claude.ai"`. Then it records `[accounts.<name>]` (with `share`, if any) in the config and prints `added work: logged in as you@company.com (Acme, team)`.
- **Any other login is refused,** such as a Console login from `-- --console`: Claude Code keeps those outside the config dir (see the docs facts), so the account couldn't keep its own. The error names the method `auth status` reports, and the account is cleaned up as for a failed login.
- **If the login fails or is cancelled,** it runs `claude auth logout` for the dir (best effort), deletes the dir and writes no config entry. So a retry starts clean. The hint is `cam add <name>` again.
- **Same account twice:** if the email matches the system login's or another account's (read from their `.claude.json`), cam asks Claude Code about that other login too. One email can belong to several orgs, so it warns only when the org matches as well: the browser was probably still signed in to that account. The warning names `system` as "your own login (system)". The fix: switch accounts in the browser, then run `cam login <name>`.
- **Next steps,** printed after the result: `cam use <name>`, the `cam init` line for `$SHELL` unless plain `claude` already reaches cam, and that the account starts with an empty setup, which shows the first-run screens and none of `~/.claude`.

### `login`
- **Which account:** `<name>`, or the one that applies here, resolved as `exec` resolves it. Without a name, cam prints the `which` line first, so logging in from the wrong folder is obvious. `system` is allowed: that's plain `claude auth login` on Claude Code's own setup.
- **Checks first:** the name is configured (or `system`), its dir exists (or it fails with the `cam add <name>` hint, as `exec` does), and the real `claude` is found. Then it tops up the links of the groups the account shares.
- **Runs** `claude auth login` with the environment `exec` would give `claude`, the terminal passed through, and the arguments after `--`, after the same "which account" lines as `add`. Claude Code replaces the account's login, even one that's still valid: it goes through the browser again, and the expiry moves forward (checked 2026-11-15).
- **Confirms** with `claude auth status --json` and prints `work: logged in as you@company.com (Acme, team)`, with the same-account warning as `add`. A login other than claude.ai is an error, as for `add`.
- **If the login fails or is cancelled,** it exits non-zero, with `cam login <name>` as the hint, and deletes nothing.
- It's the same login as `/login` in a session. `cam login` is for when no session is open: a login about to expire, a `doctor` failure, or the wrong claude.ai account after `add`.

### `remove`
- Checks that the name is valid and the config parses.
- **Asks first** when the account has a dir: it names the dir and what goes with it, and only `y` or `yes` goes on. `--yes` (`-y`) skips the question. With stdin or stderr not a terminal and no `--yes`, it fails before doing anything, so a script never deletes an account by accident.
- Runs `claude auth logout` with the account's `CLAUDE_CONFIG_DIR`, which deletes its credential slot. If that fails, it stops before deleting anything, so the slot isn't orphaned. The error includes what `claude` printed on stderr, redacted.
- Then deletes the dir without following links, and then the config entry.
- What's lost is everything the account doesn't share: its MCP servers, trusted folders and org caches, and, unless it shares `history`, its transcripts and memory. Shared files stay in `~/.claude`.
- **Afterwards,** a note says that markers naming the account now make `claude` stop with an error. If the marker that applies in the current folder names it, the note gives its path.
- It works on partial state (only a dir, or only an entry), and fails if there's neither, with a close name or `cam list` as the hint.

### `list`
- One row per account, `system` first: the name, and the email from `oauthAccount.emailAddress` in its `.claude.json`, or `-`. For `system` that's `~/.claude.json`, or the one in a `CLAUDE_CONFIG_DIR` the user exported. Once an account shares something, a third column, SHARES, lists each account's groups.
- It reads files only and never runs `claude`, so it's instant. That's Claude Code's last record, not a live check. `whoami` and `doctor` ask Claude Code.

### Config writes
Via `toml_edit`, so comments and formatting survive. cam refuses to edit a config that doesn't validate and checks the result validates too. It skips the write when nothing changed, writes atomically (temp file + rename), writes through a symlink rather than replacing it, keeps an existing file's permissions, and creates `~/.config/cam/` if needed.

### `exec`
`cam exec` resolves the account for the current directory, then replaces cam with the real `claude` (`std::os::unix::process::CommandExt::exec`), so the TTY, signals and exit codes behave natively. Windows can't, so there cam waits for `claude` (see Windows). Everything after `exec` goes to `claude` unchanged. `--` is needed only when an argument could be read as cam's own, as in `cam exec -- --help`.

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
  3. For each of `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` that is set and non-empty, tell on stderr, since it outranks the login. `ANTHROPIC_API_KEY` in an interactive session is only a note: Claude Code asks once whether to use it, and its "Use custom API key" toggle in `/config` changes the answer. With `-p` or `--print` (before any `--`), Claude Code uses the key without asking, so it's a warning, as the other two always are. Cloud-provider variables and `apiKeyHelper` are left to `doctor`. `CLAUDE_SECURESTORAGE_CONFIG_DIR`, set even to an empty value, is a warning too (see Rejected, under Why another one): `add` and `login` warn about all of these as well.
  - cam doesn't check the login before launching. If it has expired, Claude Code asks for `/login` itself, and the new login lands in the account.
- **`system`:** the environment is unchanged, with one exception. If `CAM_ACTIVE_ACCOUNT` is set, cam is running inside a session a parent cam started, and the `CLAUDE_CONFIG_DIR` it sees is that account's. Both variables are removed, so a `system` folder really gets Claude Code's own setup. A `CLAUDE_CONFIG_DIR` the user exported themselves is left alone.
- **Order:** loop guard, resolution, discovery, then the links.
- **Recursion guard:** cam sets `CAM_SHIM_DEPTH` to one more than it received (a missing or garbled value counts as 0). It refuses to launch once the incoming value reaches 10. A low limit would break legitimate nesting, because claude's Bash tool running `claude` raises the depth too. A real loop, such as a wrapper script on `PATH` that calls cam again, reaches 10 in milliseconds.
- **Exit codes:** `claude`'s own, since cam has been replaced. When cam itself fails, it exits with 127 if `claude` can't be found (including a missing `CAM_CLAUDE_BIN` target), 126 if it can't be run, and 1 otherwise. That's the convention `env` and the shell use.
- Like `execvp`, cam hands a `claude` that the kernel can't run directly (one without a shebang) to `/bin/sh`.

### Plain `claude` (M4; `cam init` since 2026-11-15)
- **The shim:** if `argv[0]`'s basename is `claude`, cam behaves exactly like `cam exec -- <args…>`. Every argument is passed on unparsed, so `claude --help` and even `claude exec` reach claude. Errors still start with `cam:`, so it's clear which program failed. When the error is about the account (a marker, `CAM_ACCOUNT`, the config or a missing account dir), it ends with a hint for `CAM_ACCOUNT=system claude`, unless that's already set.
- **`cam init <shell>`,** for bash, zsh or fish, prints code for the end of the shell's startup file: `eval "$(cam init zsh)"`, `eval "$(cam init bash)"` or `cam init fish | source`. cam never edits startup files itself. Each time it runs, that is at each shell start:
  - **The link:** it makes sure `~/.local/share/cam/shims/claude` is a symlink to cam. It's made under a temporary name and renamed into place, so the swap is atomic. Its target is the `cam` on `PATH` when that's this same binary, else cam's own executable. The `PATH` spelling, such as `/opt/homebrew/bin/cam` or `~/.cargo/bin/cam`, survives package-manager upgrades, where a canonical path into a versioned directory wouldn't.
  - **Only cam's own link is touched:** a `claude` there that is missing, or a symlink to a cam binary (this one, or any file named `cam`, such as an old build, even if dangling), is created or replaced quietly. Anything else is left alone, with a warning on stderr, and then the code doesn't touch `PATH`.
  - **`PATH`:** the code sets `PATH` to the shim dir followed by the current `PATH` minus any entry for that dir (give or take trailing slashes). cam computes the value, since it sees the shell's `PATH`, and prints it quoted for the shell. So the dir is first exactly once, even in a nested shell whose startup file put `~/.local/bin` in front again. A `PATH` that isn't UTF-8 is left as it is, with the dir added in front.
  - **Completions** follow (see Completions). zsh's need `compdef`, so they're wrapped in `if (( $+functions[compdef] ))` and skipped quietly before `compinit`.
  - Problems are warnings on stderr, `cam: warning: …`, and the rest of the code still loads: an error at every shell start would be worse than a missing link.
- **Why not a command that installs once** (`cam shim install`, M4–M7): it printed a line that the user then had to add by hand, the word "shim" was jargon, and a second line was needed for completions. One line that does all three is what zoxide, fnm and mise do. To stop, the user deletes the line. The link left behind does nothing off `PATH`, and `doctor` names it.
- **Loops:** discovery skips the shim dir and any `claude` that is this cam, so the link never finds itself. The recursion guard covers the rest (see `exec`).
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
- For an account that shares something, a `shares:` line lists its groups.
- Within 3 days of expiry, it warns with the `cam login <name>` hint.
- Not logged in, or expired: it prints the report, then fails with the `cam login <name>` hint.
- If something outranks the login, the login line names it (`ANTHROPIC_API_KEY, which Claude Code uses instead of a login`), and it doesn't fail.

### Completions
- **Static scripts from `clap_complete`,** not its run-time mode. That mode sits behind an `unstable-dynamic` feature whose API can change between minor versions. With static scripts, nothing runs cam while completing.
- **Account names are baked in** when the script is generated, as the values of `use` and `login` (plus `system`), and of `remove`, `share` and `unshare`. The group names after `share` and `unshare` are fixed. That's done on a copy of the command used only for generation, so parsing still accepts any name.
- **Loaded at shell start** by `cam init`, or on their own with `eval "$(cam completions zsh)"` and the like, rather than saved to a file. New accounts appear in the next shell. `eval` rather than `source <(…)`, because macOS's bash 3.2 can't source a process substitution.
- **Quiet:** a missing or broken config gives a script without names and no error, since this runs at every shell start.
- **fish:** clap's fish script completes no positional values, so cam appends one `complete` line per value, using the `__fish_cam_using_subcommand` function clap's script defines. Tried in fish 3.6 on Debian (2026-11-23): `cam init fish | source` put the shim dir first, `complete -C` offered the account and group names with their help, and plain `claude` ran as the folder's account. Since these lines don't know positions, `cam share work <TAB>` offers the account names next to the groups.
- Tested: in bash 3.2, by calling the completion function with `COMP_WORDS`. In zsh, `cam init zsh` registers `_cam` after `compinit` (in the test suite), and completing was tried through a pseudo-terminal during development.
- The output is redacted like everything else.

### Doctor checks
`cam doctor` prints one line per check, marked `ok`, `info`, `warn` or `fail`, with `hint:` lines indented below. Then comes a tally (`no problems found`, `1 warning`, `2 problems, 1 warning`). Any `fail` makes it exit 1, with the tally as the error; warnings alone exit 0. It runs `claude auth status` once for `system` and once per account (about 0.4 s each), minus the variables that outrank a login, so it checks each login itself. It never reads a credential. In order:
- **claude:** the real `claude`, found as `exec` finds it.
- **path:** what plain `claude` runs: the first executable `claude` on `PATH`. `ok` if that's cam's link or cam itself. `warn` if `cam init` has made the link but it isn't reached: the hint gives the `cam init` line for `$SHELL`, says it must come after the line that adds the other `claude`'s dir, and names the link to delete for someone who'd rather not use cam for plain `claude`. `info` if there's no link: it's optional, and `cam exec` works without it.
- **config:** parses, with the number of accounts. A missing file or no accounts is `info`.
- **system:** who Claude Code's own setup is logged in as. Not logged in is `info`, since someone may use only cam accounts, and an expiring or expired login is `warn`.
- **account `<name>`,** one per configured account:
  - The dir exists (`fail` if not).
  - For each group the account shares, each entry is a link to its place in `~/.claude`: `ok` once for all of them. A missing link is `warn` (the next launch adds it). A real file or folder in its place, or a link elsewhere, is `warn`, naming it: that part isn't shared, and `cam share <name> <group>` is the fix. A `keybindings.json` that `~/.claude` doesn't have yet is `info`.
  - Each `*.before-share-*` entry in its dir is `info`, so it isn't forgotten.
  - The login: `ok` with the email and expiry, `warn` within 3 days of expiry (when Claude Code starts warning too), `fail` if expired or not logged in. The hint is `cam login <name>`.
- **same account:** two accounts, or an account and `system`, logged in as the same email in the same org: `warn`. The same email in different orgs is fine.
- **here:** the folder's resolution, as `which` prints it, or its error.
- **environment:** in this shell, the variables that outrank a login: `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` (set and non-empty), and `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` and `CLAUDE_CODE_USE_FOUNDRY` when on (`1`, `true`, `yes` or `on`, the values the docs give for boolean variables). Also `CLAUDE_SECURESTORAGE_CONFIG_DIR`, set even to an empty value, which gives every account the same login slot, and `CLAUDE_CONFIG_DIR` exported outside a cam session: `warn`, since `system` then isn't `~/.claude`, while accounts still share from `~/.claude`.
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

## Windows (M10, done 2026-11-24)

**What Claude Code does there,** from a reading of CLI 2.1.331. That was its macOS build, whose shared code keeps the Windows branches, so each point wants a look at the Windows build *(verify)*:
- The config dir is `%USERPROFILE%\.claude`, or `CLAUDE_CONFIG_DIR`, and `.claude.json` sits in the home dir or in `CLAUDE_CONFIG_DIR`, as on Unix. Nothing goes in `%APPDATA%`.
- The login is a `.credentials.json` in the config dir, as on Linux, so each account keeps its own. A Windows Credential Manager store is being rolled out behind a flag (`CLAUDE_CODE_FORCE_WINDOWS_CREDMAN=1`, or a server-side gate read from each config dir's `.claude.json`). Whether its entry is named per config dir, as the Keychain's is, isn't known. It wasn't in use in the acceptance run below: each config dir had its own `.credentials.json`, and Credential Manager held no entry for Claude. If the rollout reaches Windows users, run the check again *(verify)*, since accounts sharing one entry would share one login.
- The native installer puts a real `claude.exe`, a copy rather than a link, in `%USERPROFILE%\.local\bin`. An update renames it aside and puts a new copy in its place, without checking that the file there is its own. npm installs `claude.cmd`.
- It saves `settings.json`, `.claude.json` and the files its Write and Edit tools change by writing a temp file and renaming it over the target, after following a symlink. So a symlink survives a save, and a hard link silently turns into a separate file.
- `CLAUDE_CODE_PROCESS_WRAPPER` is ignored on Windows ("the launcher must exec into Claude Code, which Windows can't do"). Claude Code's own relaunch there ignores Ctrl-C, waits for its child and exits with its code.

**What cam does.** Everything that differs is in `src/platform/` (`unix.rs`, `windows.rs`). The rest is the same code on both.
- **The home dir** is Claude Code's: `USERPROFILE` if set and not empty, else the profile folder Windows knows, as Node's `os.homedir()` does. `HOME`, which Git Bash sets, is ignored, as Claude Code ignores it for its config dir. The tests set `USERPROFILE` to the sandbox's home.
- **Paths:** as on Unix, under the home dir: `~\.config\cam\config.toml`, `~\.local\share\cam\accounts\<name>` and `~\.local\share\cam\shims`, each built one component at a time so they're spelled with `\`. Claude Code itself uses `~\.claude` and `~\.local\bin` there.
- **Permissions:** no mode bits. What cam creates in the user profile inherits its ACL, which already keeps other users out.
- **Finding `claude`:** each `PATH` dir is tried with the extensions in `PATHEXT`, in its order, as `cmd.exe` does, so `claude.exe` comes before `claude.cmd`. A file without one of those extensions can't be started, such as the shell script npm puts beside `claude.cmd` for Git Bash. File identity, for cam itself and the shim dir, is the volume and file index, which also catches hard links, and names compare without case.
- **Launching:** Windows can't replace a process with another, so `exec` starts `claude` with the console passed through and exits with its exit code. While it runs, cam ignores Ctrl-C and Ctrl-Break through a handler, since the console's ignore flag would be inherited by `claude`, which handles them itself. `claude` runs in a job object that ends it if cam ends first, so a program that stops the `claude` it started, which is cam, doesn't leave the real one running. What `claude` starts breaks away from the job, as it would outlive `claude` on Unix. Rust runs a `.cmd` through `cmd.exe` and refuses arguments it can't quote safely for it. The rest of `exec` (resolution, links, warnings, `CAM_SHIM_DEPTH`) is unchanged, and so are the exit codes cam uses for its own failures.
- **The shim:** `claude.exe` in the shim dir, a copy of `cam.exe`. Not a symlink, which needs Developer Mode, and not a hard link: that's the same file as `cam.exe`, and Windows locks a running program's file, so while any session started through plain `claude` ran, cam couldn't be upgraded. cam knows it's been started as the shim from the file Windows started (`current_exe`), whatever `argv[0]` says. It never goes in `~\.local\bin`, which Claude Code's updater overwrites.
  - `cam init powershell`, at each PowerShell start, keeps the copy current. A copy keeps cam's size and write time, so a current one is known without reading it. Otherwise it's replaced if it's cam's, which every cam binary marks with a fixed string, and left alone with a warning if it's any other `claude.exe`, as on Unix. The new copy goes in under a temporary name. The old one is renamed aside first, since a running session keeps it open and Windows renames a running program but won't replace it, and it's deleted then or at a later `cam init`, once nothing runs it.
  - The line for `$PROFILE` is `Invoke-Expression (& cam init powershell | Out-String)`. Its output starts with clap's PowerShell completions, whose `using namespace` lines a script allows only before any other statement: in 0.1.3 they came after the `PATH` line, and the whole `Invoke-Expression` failed to parse, which a Windows Server 2025 VM showed on 2026-11-24. Then it sets `$env:PATH`, split and joined on `;`, with the shim dir first and once, compared without case or a trailing `\`. PowerShell's single quotes are doubled, curly ones too. A Windows test runs the profile line in Windows PowerShell itself.
  - PowerShell is the only shell on Windows: `$SHELL` is ignored there, and `cam init bash`, `zsh` or `fish` print only completions and a warning, since Git Bash keeps `PATH` in its own form. `cam init powershell` works on Unix too, for `pwsh`.
- **Sharing:** folders link through directory junctions, which any user can make. Files link through symlinks, which Windows makes only with Developer Mode on, or for an administrator, and never through hard links, which Claude Code's saves would split. Without Developer Mode, `add` and `share` refuse a group with a file in it before anything changes, rather than moving the account's own `settings.json` aside and then failing to link it. `skills` has only folders, so it can always be shared. A launch that can't make a file link warns, as for any link it can't make. Windows counts a junction as a link to a folder, so the checks, `unshare` and `remove` treat it as a link, and removing it never touches what it points to.
- **Releases:** an x64 zip, and a PowerShell installer that puts `cam.exe` in `~\.local\bin`. No Arm build yet: it would be cross-compiled and never run before shipping, and Windows on Arm runs the x64 one through its emulation.
- **Manual acceptance (passed 2026-11-24):** on an EC2 Windows Server 2025 VM, as Administrator in Windows PowerShell 5.1, with Claude Code 2.1.331 installed by its own installer and cam by its PowerShell installer:
  - A cam account logged in to a Team org and `system` to a Max plan: `/status` in a folder marked for the account showed the Team org and its managed settings, and in another folder the Max login. `whoami`, `list` and `doctor` agreed. The browser didn't open on Windows Server, so both logins went through Claude Code's paste-the-code flow, from another machine's browser.
  - Each config dir had its own `.credentials.json`, and Credential Manager had no entry for Claude. The org's policy caches went to the account's dir.
  - Sharing `skills` and `settings` made junctions for the folders and a symlink for `settings.json`. The account read the shared skill through its junction. A `/model` change in a session saved into `~\.claude\settings.json` and left the symlink and the junctions in place, with no temp files behind.
  - After `cam init powershell` was fixed (below), plain `claude` in PowerShell went through the shim as the folder's account, and `doctor` found no problems. `cam remove` deleted the account's dir and left `~\.claude` alone.
  - It found one bug, in 0.1.3: `cam init powershell` printed the `PATH` line before clap's completions, whose `using namespace` lines PowerShell allows only at the start of a script, so the profile line failed to parse. 0.1.4 prints the completions first, and a Windows test now runs the profile line in Windows PowerShell.
  - Not covered: a standard user without Developer Mode, since Administrator can make file links (the refusal is unit tested), Windows 11 itself, and Windows on Arm.
- **Plan:** three pull requests. (1) cam builds and runs on Windows: `src/platform/`, launching, finding `claude`, Ctrl-C, and a Windows CI job for the unit tests. (2) The fake `claude` in Rust, so the integration tests run on Windows too (done, see Testing). (3) The shim and `cam init powershell`, sharing, Windows builds and the PowerShell installer, which made Windows a supported platform in the README and the releases.

## Herdr (M11, prototype 2026-11-27)

[Herdr](https://herdr.dev) is a terminal workspace manager for coding agents: it finds `claude` in a pane and lists it in a sidebar. What it does, measured with Herdr 0.9.5 (protocol 22) on macOS in throwaway servers: `herdr --session <name> server`, or `herdr server` with a temporary `HOME`, since Herdr keeps its sockets and plugins in `~/.config/herdr`:
- **What a pane gets:** `HERDR_ENV=1`, `HERDR_PANE_ID`, `HERDR_SOCKET_PATH` and `HERDR_BIN_PATH`. The socket takes one JSON request per line and per connection, `{"id", "method", "params"}`, and answers with one line, `{"id", "result"}` or `{"id", "error"}`. On Windows, `HERDR_SOCKET_PATH` names a file, and the pipe is `\\.\pipe\` followed by that whole path. That's from the notes of Herdr's Auto Title plugin, not tried here *(verify)*.
- **The agent's label:** `pane.report_metadata` with `agent: "claude"` and `display_agent` sets the name that Herdr shows for the pane's agent. Herdr drops the report unless it already sees `claude` in the pane. The label stays through Claude Code's own session reports and clears when that `claude` exits, so the next one starts unlabeled. Tokens, the `$name` values that sidebar rows can show, don't clear: they stay after the agent exits, until something replaces them or their `ttl_ms` runs out. So cam uses the label, not a token.
- **Plugins:** a `herdr-plugin.toml` declares actions, panes (a popup, overlay, split, tab or zoomed pane that runs a command), startup hooks and event hooks. `herdr plugin install <owner>/<repo>/<subdir>` installs from a folder of a repo and runs the manifest's build commands on the user's machine. An action runs with `HERDR_PLUGIN_CONTEXT_JSON`, one object whose `focused_pane_cwd` and `workspace_cwd` are the folders of the focused pane and the workspace, and with its output piped to Herdr's plugin log. `plugin.pane.open` with a `cwd` starts the pane's command in that folder, and a popup's command starts even when no client is attached to draw it. A keybinding (`type = "plugin_action"`) or `herdr plugin action invoke` starts it, since Herdr's docs name no command palette.
- **Herdr's Claude Code hook:** `herdr integration install claude` puts `hooks/herdr-agent-state.sh` in the config dir (`CLAUDE_CONFIG_DIR`, else `~/.claude`) and registers it in that dir's `settings.json`. It reports the session at `SessionStart`, and Herdr reads the agent's state from the screen. An account that doesn't share `settings` doesn't get the hook. `herdr integration status` and `install` both honour `CLAUDE_CONFIG_DIR`.

**What cam does:**
- **The label:** `exec`, and so plain `claude`, labels the pane's agent `claude · <account>` when it runs an account inside Herdr (`HERDR_PANE_ID` and `HERDR_SOCKET_PATH` set). Only the outermost cam does (no `CAM_SHIM_DEPTH` came in), so a `claude` that Claude Code's Bash tool runs doesn't relabel the pane. `system` gets no label: an unlabeled `claude` is Claude Code's own setup.
  - cam replaces itself with `claude`, and Herdr ignores the label until it sees `claude`, so a helper reports it. Before handing over, cam runs `cam herdr label`, which starts a copy of itself and exits at once. The copy is in a process group of its own with no terminal, so it isn't among the pane's foreground processes, and its parent is gone, so it leaves no zombie under `claude`.
  - The copy asks `pane.get` every 100 ms, for up to 20 s, until the pane's agent is `claude`, and reports the label. It watches for 5 s more and reports again if the label is gone while `claude` is still there: a `claude` that just exited in the same pane can still show for a moment.
  - It's silent. No Herdr, an older one or a timeout changes nothing, and `claude` never waits for it.
- **The plugin** (`integrations/herdr/herdr-plugin.toml`): a manifest without a build step, whose commands run the installed `cam`, so it needs nothing else on any platform. Its action `cam.accounts` opens a popup that runs `cam herdr accounts` in the focused pane's folder, else the workspace's. The popup shows which account applies there and why, then a menu: use an account here (or `system`), remove the folder's marker, add an account, log one in again, run `doctor`, and put Herdr's hook in the accounts without it. Each item runs the command's own code, and its output stays until Enter goes back to the menu.
- **`cam herdr open` and `cam herdr accounts`** are what the manifest runs. clap's completion scripts list even a hidden command, so they're added to the parser only, not to the command that completions and `--help` come from.
- **`doctor`:** when Herdr's hook is in `~/.claude`, an account without it is `info`, with the `herdr integration install claude` line for its dir. Herdr still finds Claude Code there from the screen.
- **Tried on 2026-11-27,** against a real Herdr 0.9.5 with a throwaway `HOME`, an account and a `claude` that was `sleep` under that name: the label showed within a second and cleared when it exited, with no helper left running. The action opened the popup in the focused pane's folder. In a pane, the menu used `system` in the folder and put Herdr's hook in the account's dir with Herdr's own installer, and Esc closed it. The popup itself wasn't seen, since a headless server draws nothing, and nothing has been tried on Linux or Windows *(verify)*.
- **A limit:** a Herdr server started from inside a cam session, such as by Claude Code's Bash tool running `herdr update`, passes that session's `CAM_SHIM_DEPTH` to every pane, and then no pane gets a label. Telling the pane's own `claude` apart by its terminal (Claude Code runs commands without one) would avoid that, but needs a terminal in the tests.
- **Where it lives:** in this repo, as one folder, not a monorepo or a repo of its own. The manifest calls `cam herdr …`, so the plugin and the code it runs change in the same pull request. Herdr builds nothing at install time: a plugin in Rust or Go of its own would need that toolchain on every user's machine. `integrations/` is left out of the crate. If the plugin grows a UI of its own or a release pace of its own, it can move to its own repo.
- **Later:** the account on each workspace row, which needs a long-running plugin process, since tokens don't clear; a way to turn the label off.

## Skill (M12, 2026-11-28)

cam comes with a skill for Claude Code, `skills/cam/SKILL.md`, the common way for a tool to teach agents about itself (Herdr ships one the same way). It covers finding out which account a session or folder uses (`CAM_ACTIVE_ACCOUNT`, `which`, `whoami`, `list`, `doctor`), markers and `CAM_ACCOUNT`, the groups and what sharing does both ways, and what to leave to the user: `add` and `login` open a browser, `remove` deletes, and `share --yes` moves files aside. It also says never to read `.claude.json`, credential files or Keychain items, and never to switch accounts by setting `CLAUDE_CONFIG_DIR`.

- **Two ways in, from one file:**
  - `npx skills add brunorodmoreira/cam` ([vercel-labs/skills](https://github.com/vercel-labs/skills)) finds it at `skills/<name>/SKILL.md` and installs it for Claude Code and other agents. With `-g -a claude-code` it lands in `~/.claude/skills`, which is how `system` gets it. Checked 2026-11-28: `npx skills add <this checkout> --list` found the one skill.
  - The binary has it built in (`include_str!`), and links it into the accounts.
- **In accounts:** at each launch (`exec`), `login` and `add`, cam writes the skill to `~/.local/share/cam/skills/cam/SKILL.md` if that copy is missing or from another version, through a temp file and a rename, then links `skills/cam` in the account's own `skills/` folder to that folder, creating `skills/` if needed. So the skill always matches the cam that runs, which `npx skills` can't promise.
  - A launch only adds the link. Anything at `skills/cam` that isn't cam's link, such as a skill of the account's own named `cam`, is left alone, and so is a `skills/` that is a link.
  - An account that shares `skills` gets nothing from cam: its `skills/` is `~/.claude/skills`, where cam never writes. A launch removes a cam link left from before it shared.
  - `skill = false`, a top-level key of the config, turns it off. The next launch removes cam's link, and the `skills/` folder too if that leaves it empty, so turning it off leaves the account as it would have been.
  - Problems are warnings at launch, like the share links.
- **`share`:** before sharing `skills`, a `skills/` that holds only cam's link counts as cam's, not the account's own: `share` removes it instead of asking to move it aside. When it holds more, cam's link is removed first, so the copy moved aside is only the account's own. After `unshare`, the next launch links the skill again.
- **`doctor`** is quiet when the skill is linked or the next launch links it. It's `info` when an account shares `skills` and `~/.claude/skills` has no `cam`, with the `npx skills` line, and when an account's own `skills/cam` or a linked `skills/` keeps cam's out.
- **Rejected:** `claude --plugin-dir`, which loads a plugin for one session and writes nothing. cam would have to add it to every `claude` it starts, `claude auth` and `claude mcp` included, and a plugin's skill is namespaced, so it would be `/cam:cam`. Also rejected: writing the skill as a real folder into each account, since cam couldn't then tell its own copy from a skill of the account's own with the same name.
- **Not tried yet:** a real session in an account, with the link in place *(verify)*. Claude Code loads skills from a linked folder in `~/.claude/skills`, which is how `npx skills` installs them, and an account's `skills/` is the same folder of the config dir.

## Storage

- **Config:** `$CAM_CONFIG`, else `~/.config/cam/config.toml`. It contains no secrets.
  ```toml
  [accounts.work]
  share = ["settings", "skills"]

  [accounts.personal]
  ```
  - An empty `CAM_CONFIG` counts as unset. The fallback really is `~/.config/cam/config.toml` on macOS as well (not `~/Library/Application Support`), and `XDG_CONFIG_HOME` is not consulted.
  - A missing file is an empty config: no accounts.
  - The config is **validated strictly** every time it's read. Unknown keys are errors, so a typo is never silently ignored. Account names follow the name rule and can't be `system`.
  - An account table has one key, `share`, the groups the account shares (see Sharing), added in M8. A missing `share` means none, and an unknown group is an error, like any unknown key.
  - At the top, `skill = false` stops cam from linking its skill into the accounts (see Skill, M12). Missing means true.
- **Account dirs:** `~/.local/share/cam/accounts/<name>/`, built from the home dir and always spelled the same way, because the credential slot is keyed to the spelling.
- **Credentials:** Claude Code's own. cam never reads, writes or lists them. It only runs `claude auth login`, `logout` and `status`.

## Security rules

- cam handles no credentials. It never reads Claude Code's Keychain items or credential files, and never puts anything secret in argv or the environment.
- It parses `claude auth status --json` and prints only the email, org, plan and expiry, never the raw output.
- Redact anything matching `sk-ant-[A-Za-z0-9_-]+` in any output cam produces. cam's own stdout, stderr and clap usage errors all go through redaction, which covers a token pasted into a marker or into `CAM_ACCOUNT` by mistake.
- Marker files contain only names. A hostile repo can at most pick another one of **your own** accounts, or `system`. A later `doctor` check could flag markers tracked in git.
- Account dirs are 0700: their `.claude.json` holds MCP server configs, which can carry secrets.
- Sharing is opt-in, one group at a time, and a shared group is the same for every account that shares it. With `history`, work transcripts and memory sit in `~/.claude/projects` next to personal ones. With `settings`, the same hooks run in every account. An org's managed settings and policy caches always stay in its own account.
- cam never writes inside an existing file of Claude Code's. Sharing only adds links in the account dir, renames the account's own copies aside (never deleting them), and creates a missing shared folder or file in `~/.claude`, empty (`{}` for `settings.json`).

## Crates

`clap` (derive), `clap_complete`, `serde` + `toml` for reading, `toml_edit` for writing, `serde_json` (for `auth status` and `.claude.json`), `dirs`, `anyhow`, `thiserror` (typed errors that tests can match on). Dates use a small built-in civil-date routine instead of a date crate. For tests: `assert_cmd`, `predicates`, `tempfile`. M7 dropped `security-framework` and `rpassword`, and added `libc` to ignore Ctrl-C while `claude auth login` runs. M8 added `dialoguer` (0.12, without its default features) for the picker. M10 made `libc` Unix only, and added `same-file` and `windows-sys` on Windows.

## Testing

- **Tests never run the real `claude`.** A fake `claude` on the sandbox `PATH` keeps a fake login in `$CLAUDE_CONFIG_DIR`, or in the home dir for `system`. It's `examples/fake_claude.rs`, copied in as `claude` (`claude.exe` on Windows): `cargo test` builds examples, and they're neither installed nor released, so one fake serves every platform. Until M10 it was a shell script, which Windows can't run.
  - `auth login` writes a fake `oauthAccount` into `.claude.json`.
  - `auth status --json` reports from it, or reports an API key when `ANTHROPIC_API_KEY` is set, as the real one does.
  - `auth logout` clears it.
  - Each `auth` call is logged to `$FAKE_LOG`, so tests can check what ran, with which dir and in what order.
  - Any other call prints its argv, `CLAUDE_CONFIG_DIR`, `CAM_ACTIVE_ACCOUNT` and `CAM_SHIM_DEPTH`.
  - Variables switch on the other cases: `FAKE_LOGIN=fail` or `nothing`, `FAKE_LOGOUT=fail`, `FAKE_EMAIL` and `FAKE_EXPIRES`.
- Ctrl-C during `add` was checked by hand, by sending SIGINT to the process group the way a terminal does: cam logged out and deleted the dir.
- The sandbox clears the environment and sets its own `HOME`, `USERPROFILE` (the home dir on Windows) and `PATH`, so the real `~/.claude`, the real `claude` and the real Keychain are out of reach.
- **Resolution unit tests** on temp directory trees: nearest wins, `system` overrides a parent, env beats file, invalid or unknown names fail.
- **Sharing (M8):** only the chosen groups are linked, links are topped up on launch, a real file is never replaced at launch and `exec` warns about it, `share` moves the account's copies aside and never deletes, `unshare` removes only links (or leaves copies with `--copy`), and `remove` never follows one, so the shared targets survive. Every group syntax parses to the same list. Tests choose through `--share`, `--yes` and a stdin that isn't a terminal. The picker itself isn't driven by tests: it was tried by hand through a pseudo-terminal (2026-11-23), for `add` and `share`: ticking, unticking a shared group, Enter with no change, the question before moving copies aside, and Esc.
- **PATH discovery** and **plain `claude` end to end:** fake `claude` scripts, cam's link earlier on PATH, `argv[0]` really `claude`, and `init` refusing a real `claude` in the shim dir. `init`'s code is run in a real bash 3.2 (`--norc`) and zsh (`-f`, with and without `compinit`): afterwards `claude` resolves to the link and runs the fake `claude` as the folder's account.
- **`remove`'s question** isn't driven by tests, whose stdin isn't a terminal: they cover the refusal without `--yes`. It was tried by hand through a pseudo-terminal.
- **`whoami` and `doctor`:** the check logic is unit tested without I/O. End to end, settings files and variables hold fake secrets, and the tests assert that no value is ever printed.
- **CI:** GitHub Actions runs `cargo fmt --check`, clippy and the tests on macOS and Ubuntu for each push to `main` and each pull request, and on Windows since M10. On Windows, the tests of shell code, the symlink shim and Unix symlinks and permissions are left out. The copy shim, `cam init powershell` and junctions have Windows tests of their own, and two `exec` tests have Windows versions: a `claude.exe` Windows can't start exits 126, and two copies of cam that keep starting each other stop at the depth limit, with the exit code passed back through every waiting copy. Windows has been a required check since 2026-11-24. Ubuntu gets zsh installed for the `cam init` tests. Locally, the suite also passed in a Debian container, as a non-root user: as root, the test that a read-only dir makes a config write fail can't fail. The macOS checks and the Apple Silicon release build run on Intel runners (`macos-15-intel`): from 2026-11-23, GitHub's arm64 macOS runners were short of capacity, and it cancels a job that waits 15 minutes for one ("The job was not acquired by Runner of type hosted even after multiple attempts"). The tests don't depend on the processor, and Rust builds for Apple Silicon from an Intel Mac. GitHub is phasing out Intel images, so this goes back to arm64 once capacity recovers.
- **Repository rules** (GitHub rulesets, 2026-11-23):
  - `main` takes changes only through pull requests, squash-merged into a linear history (rebase merges went 2026-11-24, so every commit on `main` is a pull request title). The `macOS`, `Linux`, `Windows` (from 2026-11-24) and `PR title` checks must pass, no review is required, and `main` can't be force-pushed or deleted.
  - No one can move or delete a `v*` tag. A rule that only admins could create one went on 2026-11-24: it also stopped the release workflow from creating the GitHub release (HTTP 403, though its token had `contents: write`), and any bypass for workflows would have let every workflow create tags anyway.
  - Also on: Dependabot alerts, security updates and weekly Cargo updates, secret scanning with push protection, and private vulnerability reporting. Workflow tokens are read-only unless a workflow asks for more.
  - Claude reviews each pull request once, when it's opened or marked ready (`.github/workflows/claude-code-review.yml`, Anthropic's `code-review` plugin), and the owner or a collaborator can ask it more by writing `@claude` (`claude.yml`). The review is advisory, not a required check, and bots' pull requests and forks' get none. It runs on the maintainer's Claude subscription through the `CLAUDE_CODE_OAUTH_TOKEN` secret, made by `/install-github-app` on 2026-11-24, which lasts a year.
- **Manual acceptance:** after `cam add work`, `/status` in a folder marked `work` shows the work account, and outside it shows the system login. The Keychain has the slot named above. Passed 2026-11-15 with two work accounts on Team plans next to a personal Max login: each `/status` showed its own org, with "Enterprise managed settings (remote)" only in the work account. Plain `claude` reaches cam only once the shim dir is on `PATH`, and `doctor` warns until it is. With every group shared, the links are still links after a session: passed 2026-11-23 with links made by hand (see the notes under Why another one).

## Milestones

1. Resolution, `which`, `use`/`unuse` and the config file.
2. `SecretStore` (Keychain + test store), `add`/`list`/`remove`. *(token mode)*
3. `exec`: real-binary discovery, env injection, conflict warnings.
4. Shim mode, `shim install`/`uninstall` (replaced by `cam init`).
5. `whoami` and `doctor`.
6. Shell completions.
7. **Native logins (done 2026-11-15):** an account is an empty config dir with its own login, and shares nothing yet. `add`/`login`/`remove` through `claude auth`, `system` in place of `none`, no `default`, `list`/`whoami`/`doctor` from Claude Code's own record. The Keychain store, the token prompt and `token_created` go.
8. **Sharing (done 2026-11-23):** the four groups, the picker and `--share`, `share` (moving copies aside) and `unshare` (with `--copy`), links topped up by every launch and `login` with `exec`'s warning, and `doctor`'s link checks. The file links held in a session first (see the notes under Why another one).
9. **A release (in progress, 2026-11-23):** cargo-dist 0.34 builds on a version tag, from `dist-workspace.toml` and `.github/workflows/release.yml`:
   - Archives for macOS arm64 and x86_64, Linux x86_64 and arm64, and Windows x64 (from M10), with checksums, on GitHub Releases. The Linux builds use glibc and are made on Ubuntu 22.04, so they need glibc 2.35 or newer.
   - A shell installer that puts `cam` in `~/.local/bin`, which Claude Code's own installer already has on `PATH`, and from M10 a PowerShell one that does the same on Windows.
   - The Homebrew formula `cam`, pushed to the `brunorodmoreira/homebrew-tap` repo with a token that can write only there (the `HOMEBREW_TAP_TOKEN` secret): `brew install brunorodmoreira/tap/cam`.
   - The crate `cam-cli` on crates.io. 0.1.0 was published by hand, since crates.io's trusted publishing only works for a crate that exists. From then on, release-plz publishes it (below), with a short-lived token from trusted publishing, so no crates.io token is stored. crates.io's trusted publisher names the workflow `release-plz.yml` and the `release` environment, which only `main` can deploy to.
   - **Versions and releases (release-plz, 2026-11-24):** pull request titles are conventional commits, checked by `.github/workflows/pr-title.yml`, and a squash merge makes the title the commit on `main`. On each push to `main`, release-plz (`.github/workflows/release-plz.yml`, `release-plz.toml`) keeps a release pull request open. It bumps the version by Cargo's rules: before 1.0, `feat!:` or `fix!:` bumps the minor version and anything else the patch. It also writes the CHANGELOG section from the titles, leaving out `refactor`, `test`, `ci`, `chore` and the like unless they're marked breaking. Merging that pull request publishes to crates.io and pushes the `v<version>` tag, which starts cargo-dist's `release.yml`. release-plz creates no GitHub release (cargo-dist does, with the archives), runs no `cargo-semver-checks` (cam's library is internal to the binary), and publishes only when its own pull request merges (`release_always = false`). It opens that pull request only once a `feat`, `fix` or `perf` title, a breaking one or a Dependabot update has merged (`release_commits`): on 2026-11-24 a `ci:` change that touched DESIGN.md opened one for 0.1.3 with no changelog entry, since the docs ship in the crate. It acts through a token from the repo's GitHub App (`RELEASE_APP_CLIENT_ID` variable, `RELEASE_APP_PRIVATE_KEY` secret), because a tag or pull request made with the workflow's own token starts no workflows: the tag wouldn't start `release.yml`, and the release pull request would never get the checks it needs to merge. Dependabot's titles are `chore(deps): …`. Publishing on GitHub waits for every build, and the release is announced only once Homebrew succeeds.
   - Release notes from `CHANGELOG.md`. Linux ships although no real Claude Code session has been tried there, which the README says.
10. **Windows (done 2026-11-24, accepted on Windows Server 2025):** see Windows.
11. **Herdr (prototype, 2026-11-27):** the pane label, the plugin with its accounts popup, and `doctor`'s hook check. See Herdr.
12. **Skill (2026-11-28):** `skills/cam/SKILL.md` for `npx skills`, built into the binary and linked into each account that doesn't share `skills`, `skill = false` to turn it off, `share` removing cam's link, and `doctor`'s notes. See Skill.

Later ideas: Linux (the design no longer needs a secret store, and the whole test suite passes on Debian, run by CI too, but no real session has been tried *(verify)*), and a check of IDE extensions (they may bundle their own CLI and bypass the PATH shim *(verify)*).

## Open questions

- **Do file links survive every write?** Mostly answered on 2026-11-23: Claude Code's own writes keep them, in its code and in a session, and so do vim and Zed. Still unchecked: plugin updates, auto-updates, `claude project purge`, and sessions on the storage backend that `CLAUDE_CODE_HOVER_REST` turns on. The `exec` and `doctor` warnings cover a break.
- **Settings without plugins:** sharing `settings` brings `enabledPlugins`, so `plugins/` comes with it. Is a group for plugins alone worth having?
- **Login lifetime:** does an account that isn't used expire after about a month, while a used one keeps renewing *(verify)*?
- **Remote Control** in an account dir *(verify)*.
- **An exported `CLAUDE_CONFIG_DIR`:** should accounts share from it instead of `~/.claude`? For now they share `~/.claude`, and `doctor` warns.
- **Logical vs physical cwd:** should the walk-up honour `$PWD` when it points at the current directory, as `pwd -L` does? Then markers above a symlinked folder would apply. M1 uses the physical path.
- Do the VS Code and JetBrains extensions honour a `claude` on PATH?
- **Name (settled 2026-11-11):** renamed from `clam` to `cam`. On crates.io, `clam` is a template engine, and [`clam-cli`](https://crates.io/crates/clam-cli) ([jose2kk/clam](https://github.com/jose2kk/clam)) is also a Claude Code profile manager. The crate `cam` is taken by an unrelated camera library, so the package is `cam-cli` and the binary is `cam`. `Cargo.toml` had `publish = false` until the release work, which dropped it. When Linux support comes, check for a clash with libcamera's `cam` utility.
- **Answered 2026-11-23:** the org caches token mode left in `~/.claude` (`remote-settings.json`, `policy-limits.json` and their `.signature*`, `.stamp` and `.synced-at` files) were never rewritten by sessions of a personal system login after token mode ended, so they were leftovers. Deleting them, after a backup, is safe.
- **Decided 2026-11-23, building M8:**
  - Files cam creates in `~/.claude` are 0600 and folders 0700, since they can hold prompts and transcripts.
  - The SHARES column of `list` and the `shares:` line of `whoami` show only once an account shares something, so the common output stays as it was.
  - `cam add` without sharing lists `cam share <name>` among its next steps.
  - A link to the right place by another spelling (say, through `..`) counts as linked.
- **Decided 2026-11-15:**
  - Native logins replace token mode (see Token mode).
  - `none` becomes `system`, because it's your normal login, not "no account".
  - `default` goes, along with the planned `cam default`: a marker in the home folder covers it.
  - No `--email` of cam's own: the account name is the label, and the real email comes from Claude Code. `add` doesn't fill in `--email` for the login either, but the user can pass Claude Code's own after `--`.
  - `cam login [<name>]` runs Claude Code's login again for an account that already exists. It needs no state of its own, and `/login` in a session does the same.
  - Accounts start empty and share nothing by default. Sharing is by group (`settings`, `instructions`, `skills`, `history`), chosen in `add` with a picker or `--share`, and changed with `share`/`unshare`. Links keep the groups in sync.
  - From the DX review, for M8: `extensions` is renamed `skills`. `share` moves an account's own copy aside under a dated name instead of refusing, asking first and never deleting. Missing shared files with a valid empty form are created. `exec` warns about a link that has become a file. Group lists take every syntax and `all`. `unshare` reports what it removed, and `--copy` keeps a snapshot.
  - MCP servers and first-run state are neither shared nor copied, because cam never writes inside Claude Code's files.
  - M7 ships accounts without sharing, and all link code moves to M8.
  - After a DX review of M7: `cam init <shell>` replaces `cam shim install`/`uninstall`. Arguments after `--` go to `claude auth login`, which keeps the native flow while allowing `--email` and `--sso`. `remove` always asks when there's a dir, with `--yes` for scripts. The same-account warning compares the org too.
