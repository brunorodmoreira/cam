# cam — Claude Account Manager

Status: milestones 1–3 done (resolution, config, token store, `add`/`remove`/`list`, `use`/`unuse`/`which`, `exec`) · Written 2026-11-11

Pick the Claude Code account **per folder**, the way `.nvmrc` / `.node-version` pick a Node version with fnm. A `.claude-account` file names an account. The nearest one up the folder tree wins. `claude` starts with that account's credentials, and nothing else about your Claude Code setup changes.

## Why another one

Existing tools ([Nemo-Illusionist/claude-code-account-switcher](https://github.com/Nemo-Illusionist/claude-code-account-switcher), [mtxr/claude-switch](https://github.com/mtxr/claude-switch), [realiti4/claude-swap](https://github.com/realiti4/claude-swap), [diananerd/claude-account-switcher](https://github.com/diananerd/claude-account-switcher), [fairy-pitta/cc-account-switcher](https://github.com/fairy-pitta/cc-account-switcher)) all switch **`CLAUDE_CONFIG_DIR`**. That isolates credentials, but it also splits settings, plugins, hooks, memory and history per account.

cam's differentiators:

1. **One shared Claude Code setup.** It swaps only the credential, using a `claude setup-token` token passed as `CLAUDE_CODE_OAUTH_TOKEN`. Settings, plugins, memory and history stay shared.
2. **Resolved at launch, scoped to one process.** There is no `cd` hook exporting secrets into the shell. The token goes only into the environment of the `claude` process cam starts.
3. **A portable marker file with walk-up lookup**, like fnm's `--version-file-strategy=recursive`. One file at a parent folder covers every nested repo, so nothing is committed into the repos themselves.
4. **It explains itself.** `cam which` says which account was chosen and why. `cam doctor` checks that the token really belongs to the expected account.

Known trade-offs of token mode (documented by Anthropic):
- Sessions using `CLAUDE_CODE_OAUTH_TOKEN` cannot use Remote Control or fetch claude.ai connectors.
- The token lasts about one year.
- `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `apiKeyHelper` and cloud-provider variables **outrank** it. See https://code.claude.com/docs/en/authentication.md#authentication-precedence
- A config-dir mode per account is planned for v2 to cover these.

Docs facts this design relies on (verified 2026-11-11):
- `claude setup-token` prints a long-lived token and saves nothing. It works with Pro, Max, Team and Enterprise plans.
- `CLAUDE_CODE_OAUTH_TOKEN` outranks `/login` credentials.
- `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CONFIG_DIR` are **ignored in project/local `.claude/settings.json`**. That's why an external launcher is needed at all.

Observed with CLI 2.1.322 (2026-11-12):
- `setup-token` says the token is "valid for 1 year".
- With the token from `cam exec`, `claude auth status --json` reports `authMethod: "oauth_token"`, where the `/login` session reports `"claude.ai"`. So Claude Code uses the token over a `/login` for a different account.
- In token mode, `auth status` reports **no identity**: no `email`, `orgId`, `orgName`, `subscriptionType` or `planTier`, and `loginExpiresAt` is `null` (shape under Doctor checks).
- Inside such a session, `/status` shows `Auth token: CLAUDE_CODE_OAUTH_TOKEN` and no account email or org either. The welcome header names "Claude API" instead of the subscription plan. The server accepts the token: prompts get replies.
- The docs say the token "authenticates with your Claude subscription" but don't say which usage pool it draws from, or why the header says "Claude API". Community issues call that label misleading *(unverified)*. The Usage tab in `/status` should show which pool applies *(verify)*.

## Resolution

Order (first match wins):

1. `CAM_ACCOUNT` environment variable: an account name, or `none` to force pass-through. The value is trimmed, and an empty or blank value counts as unset.
2. Nearest `.claude-account`, from the current directory up to `/`. The walk starts from the physical current directory (`getcwd`), not `$PWD`, so inside a symlinked folder it walks the target's parents.
3. `default` in the config file, if set. `default = "none"` is allowed and means pass-through.
4. Nothing: **pass-through**. Run the real `claude` with the environment untouched, so the normal `/login` account is used.

The config is read only when a step needs it: to check that a name exists, or at step 3. So `none` from `CAM_ACCOUNT` or a marker works even when the config is broken, and `CAM_ACCOUNT=none` is the escape hatch.

Marker file format:
- UTF-8, and a leading BOM is ignored. The first line that isn't empty once comments are stripped is the account name, trimmed. Later lines are ignored.
- `#` starts a comment anywhere on a line, so `work # day job` names `work`.
- Names match `^[a-z0-9][a-z0-9_-]*$`.
- `none` is reserved and forces pass-through for that subtree, overriding a parent marker.
- An **unknown account name is a hard error**. cam exits non-zero with a hint (`cam add <name>`) and never silently falls back to another account.
- An **unusable marker is also a hard error, never skipped**. That covers a marker that is unreadable (for example a directory), not UTF-8, names nothing (empty or only comments), or holds an invalid name. Skipping it would silently pick up a parent's account.

`cam which` prints one line. The first word is the account, or `none` for pass-through:
```
work (from /Users/me/Work/.claude-account)
work (from CAM_ACCOUNT)
personal (from default in /Users/me/.config/cam/config.toml)
none (pass-through, from /Users/me/Work/oss/.claude-account)
none (pass-through: no CAM_ACCOUNT, .claude-account or default)
```

## Commands (v1)

| Command | Does |
|---|---|
| `cam add <name> [--email <expected>] [--force]` | Prompts for the token with **hidden input** (from `claude setup-token`). Stores it in the Keychain and records the account in config. An identity check that compares the email is planned for M5, but `auth status` can't provide it in token mode (see Doctor checks). |
| `cam remove <name>` | Deletes the Keychain item and the config entry. |
| `cam list` | Accounts, expected email, token age and default. Never shows tokens. |
| `cam use <name> [--dir <d>]` | Writes `.claude-account` in the current directory (or `<d>`, which must exist), containing just `<name>\n` and replacing any existing marker. Validates that the name exists. `cam use none` writes a pass-through marker and needs no account. |
| `cam unuse [--dir <d>]` | Removes the marker in that folder only, never a parent's. If that folder has none, it exits non-zero and names the nearest parent marker. |
| `cam which` | Resolved account and its source: env, a file path, the default, or none. |
| `cam whoami` | Shows the resolved account and its expected email, and whether its token works. Email, org and plan only if a source for them is found (see Doctor checks). |
| `cam doctor` | Health checks (below). |
| `cam exec [--] <args…>` | Resolves, then execs the real `claude` with `<args…>` and the account's token. |
| `cam shim install [--dir <d>]` / `uninstall` | Manages the `claude` → `cam` shim symlink. |

### `add`, `remove` and `list` (M2)
- **Token input:** when stdin is a terminal, `add` uses a hidden prompt. Otherwise it reads all of stdin, so scripts can run `cam add work < file`. The input is trimmed and must match `sk-ant-[A-Za-z0-9_-]+`. `setup-token` tokens start `sk-ant-oat01-` (checked 2026-11-12), but cam doesn't require that, so a new token version still works. Anything starting `sk-ant-api` is rejected as an API key. Token errors never echo the input.
- **Validation order:** `add` validates the name (valid, not `none`), the `--email` (one `@`, no spaces) and the config before prompting.
- **Replacing:** if a token is already stored, `add` refuses unless `--force` is given. A config entry with no stored token, such as one written by hand, is filled in without `--force`. Replacing keeps the entry's existing email unless `--email` is given.
- **Recording:** `add` stores the token first, then writes the config entry with `token_created` = today (UTC). If the config write fails, a newly stored token is deleted again. A replaced one is kept, since the old token is already gone.
- **Identity check:** deferred to M5, and `claude auth status` can't do it: in token mode it reports no email. Until a source is found, `--email` only records the expected identity.
- **`remove`:** checks that the config parses, deletes the token (a failure here leaves the config untouched), then deletes the config entry. If the account was the `default`, it clears `default` and says so on stderr. It works on partial state (only a token, or only an entry), and fails if there's neither.
- **`list`:** reads only the config, never the store, so it can't trigger a Keychain prompt. It shows one row per account (name, email or `-`, and token date with age) and marks the default. Whether a token is actually stored is left to `doctor`.
- **Config writes:** via `toml_edit`, so comments and formatting survive. cam refuses to edit a config that doesn't validate and checks the result validates too. It skips the write when nothing changed, writes atomically (temp file + rename), writes through a symlink rather than replacing it, keeps an existing file's permissions, and creates `~/.config/cam/` if needed.

### `exec` (M3)
`cam exec` resolves the account for the current directory, then replaces cam with the real `claude` (`std::os::unix::process::CommandExt::exec`), so the TTY, signals and exit codes behave natively. Everything after `exec` goes to `claude` unchanged. `--` is needed only when an argument could be read as cam's own, as in `cam exec -- --help`.

- **Finding the real binary,** on **every** launch and never cached, because `~/.local/bin/claude` is a symlink the auto-updater repoints:
  1. `CAM_CLAUDE_BIN`, if set and non-empty. It must be an absolute path to an executable file and must not be cam itself. Otherwise it's an error, with no fallback to `PATH`.
  2. Otherwise the first executable `claude` on `PATH`. Three kinds of entry are skipped:
     - Relative entries, including empty ones. They mean the current directory, and a `claude` shipped inside a repo must never receive a token.
     - The shim dir (`~/.local/share/cam/shims`), however it's spelled.
     - Any `claude` that is cam's own executable. Files are compared by device and inode, so symlinks and hard links both count.
  - The `PATH` spelling is kept, not canonicalized, as a shell does.
- **Account resolved:**
  1. Read the token with `SecretStore::get`. If there's no token, or the stored value isn't a valid token, cam fails with a `cam add <name>` or `cam add <name> --force` hint, and `claude` isn't started.
  2. Set `CLAUDE_CODE_OAUTH_TOKEN` and `CAM_ACTIVE_ACCOUNT=<name>`, replacing any inherited values. `CAM_ACTIVE_ACCOUNT` isn't secret; it's for status lines and debugging.
  3. For each of `ANTHROPIC_AUTH_TOKEN` and `ANTHROPIC_API_KEY` that is set and non-empty, warn on stderr that it outranks the token. Cloud-provider variables and `apiKeyHelper` are left to `doctor`.
- **Pass-through:** the environment is unchanged, with one exception. If `CAM_ACTIVE_ACCOUNT` is set, cam is running inside a session that a parent cam started, and the `CLAUDE_CODE_OAUTH_TOKEN` it sees is that session's. Both variables are removed, so a pass-through folder really gets the normal `/login`. A `CLAUDE_CODE_OAUTH_TOKEN` that the user exported themselves is left alone. In the one session checked, Claude Code kept the token out of its shells (see Security rules), so this matters wherever that doesn't hold.
- Pass-through never opens the store. It can't cause a Keychain prompt, and it works on platforms without a store.
- **Order:** loop guard, resolution, discovery, then the token. So a missing `claude` never causes a Keychain prompt.
- **Recursion guard:** cam sets `CAM_SHIM_DEPTH` to one more than it received (a missing or garbled value counts as 0). It refuses to launch once the incoming value reaches 10. A low limit would break legitimate nesting, because claude's Bash tool running `claude` raises the depth too. A real loop, such as a wrapper script on `PATH` that calls cam again, reaches 10 in milliseconds.
- **Exit codes:** `claude`'s own, since cam has been replaced. When cam itself fails, it exits with 127 if `claude` can't be found (including a missing `CAM_CLAUDE_BIN` target), 126 if it can't be run, and 1 otherwise. That's the convention `env` and the shell use.
- Like `execvp`, cam hands a `claude` that the kernel can't run directly (one without a shebang) to `/bin/sh`.
- The token is **never** placed in argv, because argv is visible in `ps`.

### Shim (busybox style, M4)
- If `argv[0]`'s basename is `claude`, behave exactly like `cam exec -- <args…>`.
- `cam shim install` creates `<shim-dir>/claude -> <path to cam>`. The default shim dir is `~/.local/share/cam/shims`. It then prints the PATH line to add. The shim dir must come **before** `~/.local/bin`, where the native installer puts `claude`.
- Discovery already skips the shim dir and cam itself, and the recursion guard covers the rest (see `exec`).

### Doctor checks
- The shim dir is on PATH before the real `claude`, and the real `claude` is found.
- The config parses, and every configured account has a stored token.
- The identity check per account: the token's account email matches the expected one. This catches a token minted in the wrong browser session. **Open:** `auth status` reports no email in token mode, so M5 needs another source. `/status` in a token session shows no email or org either. The docs say a `setup-token` token "can only make model requests", so the OAuth profile endpoint will probably refuse it. If nothing works, `doctor` can only check that the token is accepted, plus indirect hints such as which managed settings load. The check matters more than first thought, because `setup-token` enforces only `forceLoginMethod`, not `forceLoginOrgUUID`, so it can mint a token in a different org than the admin intended (https://code.claude.com/docs/en/authentication.md).
- A token older than about 330 days gets a renewal warning, counted from `token_created`. `loginExpiresAt` is `null` in token mode, so it can't be used.
- Conflicting variables (`ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `CLAUDE_CODE_USE_BEDROCK/VERTEX/FOUNDRY`), and `apiKeyHelper` in `~/.claude/settings.json`. All of these outrank the token.

`claude auth status --json` output shape, observed with CLI 2.1.322. With a `/login` session:
```json
{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
 "configDirectory": "...", "loginExpiresAt": "2026-11-16T15:59:25.959Z",
 "email": "...", "orgId": "...", "orgName": "...",
 "subscriptionType": "max", "planTier": "max_20x", "storedClaudeAiLogin": true}
```
With `CLAUDE_CODE_OAUTH_TOKEN` (through `cam exec`). `loggedIn` may only mean that the variable is set, not that the server accepted the token *(verify)*:
```json
{"loggedIn": true, "authMethod": "oauth_token", "apiProvider": "firstParty",
 "analyticsDisabled": false, "projectsDirectory": "...", "configDirectory": "...",
 "loginExpiresAt": null}
```

## Storage

- **Config:** `$CAM_CONFIG`, else `~/.config/cam/config.toml`. It contains no secrets.
  ```toml
  default = "personal"            # optional

  [accounts.work]
  email = "you@company.com"       # expected identity, checked by doctor
  token_created = 2026-11-11
  ```
  - An empty `CAM_CONFIG` counts as unset. The fallback really is `~/.config/cam/config.toml` on macOS as well (not `~/Library/Application Support`), and `XDG_CONFIG_HOME` is not consulted.
  - A missing file is an empty config: no accounts and no default.
  - The config is **validated strictly** every time it's read. Unknown keys are errors, because a typo like `defualt` must not silently mean pass-through. Account names follow the name rule and can't be `none`. `default` must be `none` or a configured account.
- **Secrets:** the macOS Keychain, generic password, service `cam`, account `<name>`. Put it behind a `SecretStore` trait (`contains`, `get`, `set`, `delete`):
  - `KeychainStore`: production, using `security-framework` directly. `keyring` 4 reorganised into per-platform store crates and calls `security-framework` on macOS anyway. `contains` searches attributes only, so checking for a token never reads the secret.
  - `FileStore`: tests only, one plain file per account. Selected by `CAM_TEST_STORE=<dir>`, compiled only with `cfg(test)` or the `test-store` feature. A self dev-dependency in `Cargo.toml` turns the feature on for `cargo test`, so the `assert_cmd` tests get it, while normal, release and `cargo install` builds don't.
  - A build without the test store **refuses to run** a store command when `CAM_TEST_STORE` is set, so a misconfigured test fails instead of reaching the real Keychain.
  - Tests never touch the real Keychain.
  - macOS may ask for Keychain access the first time a newly built (unsigned) `cam` reads an item that an earlier build saved. Choose "Always Allow". Only `get` reads the secret, and only `exec` with an account calls it.
  - Non-macOS builds compile, but store commands fail with "no token store for this platform yet".

## Security rules

- The token exists only in the Keychain and in the environment of the `claude` child. It never goes into config, logs, argv, or error messages.
- Redact anything matching `sk-ant-[A-Za-z0-9_-]+` in any output cam produces, including captured stderr from `claude`. Since M1, cam's own stdout, stderr and clap usage errors all go through redaction, which covers a token pasted into a marker or into `CAM_ACCOUNT` by mistake.
- Marker files contain only names. A hostile repo can at most pick another one of **your own** accounts. A later `doctor` check could flag markers tracked in git.
- **In the one session checked, the token didn't reach the shell commands Claude Code runs** (2026-11-12, CLI 2.1.322). Inside a `cam exec` session, `! env | grep -c CLAUDE_CODE_OAUTH_TOKEN` printed 0, while `CAM_ACTIVE_ACCOUNT` came through.
  - It's unknown whether that's Claude Code's default. The session had remote enterprise managed settings, and the docs describe an opt-in, `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`, that strips credentials from Bash commands, hooks and stdio MCP servers. Its documented list doesn't name `CLAUDE_CODE_OAUTH_TOKEN`. See https://code.claude.com/docs/en/env-vars.md *(verify)*.
  - If the token does reach shells by default, cam could set the scrub in the child. But the scrub also strips the user's other credentials, such as `NPM_TOKEN`, from those commands, so it would have to be opt-in.
  - Hooks, MCP servers and the status line command weren't checked.

## Crates (suggested)

`clap` (derive), `security-framework` (macOS only; Linux/Windows stores can sit behind the same trait later), `rpassword`, `serde` + `toml` for reading, `toml_edit` for writing, `dirs`, `anyhow`, `thiserror` (typed errors that tests can match on). Dates use a small built-in civil-date routine instead of a date crate. For tests: `assert_cmd`, `predicates`, `tempfile`.

## Testing

- **Resolution unit tests** on temp directory trees: nearest wins, `none` overrides a parent, env beats file, default fallback, invalid or unknown names fail.
- **PATH discovery:** temp dirs holding fake `claude` scripts, plus a shim symlink earlier on PATH. Assert it skips itself, honours `CAM_CLAUDE_BIN` and trips the recursion guard.
- **End to end** with `assert_cmd`: a fake `claude` script prints its path, argv, `CAM_ACTIVE_ACCOUNT` and `CAM_SHIM_DEPTH`. It names which of the known fake tokens it got and never prints the value. The sandbox clears the environment and points `PATH` at its own `bin`, so the real `claude` and any real token are out of reach. Use `FileStore`.
- **Manual acceptance:** with `.claude-account` = `work` at a work folder, `/status` inside `claude` shows the work account. Outside that folder it shows the personal login.

## Milestones

1. Resolution, `which`, `use`/`unuse` and the config file. No secrets involved.
2. `SecretStore` (Keychain + test store), `add`/`list`/`remove`.
3. `exec`: real-binary discovery, env injection, conflict warnings.
4. Shim mode, `shim install`/`uninstall`.
5. `whoami` and `doctor`.
6. Later: shell completions, README, release (cargo-dist / Homebrew tap).

v2 ideas: a per-account `mode = "config-dir"` (for Remote Control and connectors), Linux/Windows stores, a prompt/status-line snippet that reads `CAM_ACTIVE_ACCOUNT`, and a check of IDE extensions (they may bundle their own CLI and bypass the PATH shim *(verify)*).

## Open questions

- **Name (settled 2026-11-11):** renamed from `clam` to `cam`. On crates.io, `clam` is a template engine, and [`clam-cli`](https://crates.io/crates/clam-cli) ([jose2kk/clam](https://github.com/jose2kk/clam)) is also a Claude Code profile manager. The crate `cam` is taken by an unrelated camera library, so the package is `cam-cli` and the binary is `cam`. `Cargo.toml` keeps `publish = false` until a release. When Linux support comes, check for a clash with libcamera's `cam` utility.
- **Logical vs physical cwd:** should the walk-up honour `$PWD` when it points at the current directory, as `pwd -L` does? Then markers above a symlinked folder would apply. M1 uses the physical path.
- **Setting the default:** there's no command for it yet, and `remove` can clear it. Add `cam default <name|none>`, or keep editing `config.toml` by hand?
- Do the VS Code and JetBrains extensions honour a `claude` on PATH?
- **Answered 2026-11-12:** what `claude auth status --json` reports on `CLAUDE_CODE_OAUTH_TOKEN`. It's `authMethod: "oauth_token"` with no identity and `loginExpiresAt: null`. That leaves a new question: how M5 can tell which account a token belongs to (see Doctor checks).
