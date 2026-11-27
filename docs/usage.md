# Using cam

## Adding an account

cam creates the account's folder, then runs Claude Code's login in it: the browser opens, and you sign in to the claude.ai account you want, as with `/login`.

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

Before the login, on a terminal, `add` asks what the account should share (see [Sharing](sharing.md)); `--share settings,skills` or `--share none` answers up front, and without a terminal it shares nothing. After the login, `add` prints the next steps. If the login fails or you cancel it, cam logs the half-made account out and deletes its folder, so you can just try again. If the new account turns out to be the same claude.ai account as `system` or another account (same email, same org), cam warns: the browser was probably still signed in to it.

## Using an account in a folder

A marker covers its folder and every folder below it:

```sh
cam use work                      # write .claude-account here
cam use system --dir ~/Work/oss   # Claude Code's own setup for a subtree
cam which                         # what applies here, and why
cam unuse                         # remove the marker in this folder
```

`--dir` takes a leading `~`, even as `--dir=~/Work`. `use` says when it replaces a marker naming another account, and notes when the folder is in a git repository, since the marker is yours rather than the repo's.

The nearest marker wins, and [Configuration](configuration.md) has the details.

## Plain `claude`

To make plain `claude` pick the account, add one line at the end of your shell's startup file:

```sh
eval "$(cam init zsh)"            # ~/.zshrc
eval "$(cam init bash)"           # ~/.bashrc
cam init fish | source            # ~/.config/fish/config.fish
Invoke-Expression (& cam init powershell | Out-String)   # $PROFILE, on Windows
```

It goes at the end, after the line that adds `~/.local/bin`, where Claude Code installs itself. At each shell start, `cam init` makes sure `~/.local/share/cam/shims/claude` is a link to cam, puts that folder first on `PATH` (once, even in nested shells), and loads completions with your account names. Open a new terminal, and every `claude` goes through cam. cam never edits your shell files itself, and it never replaces a `claude` there that isn't its own link. On Windows, the shim is `claude.exe`, a copy of `cam.exe` that `cam init powershell` replaces whenever cam changes.

cam finds the real `claude` on your `PATH` and replaces itself with it, so the terminal, signals and exit code are claude's own. For an account, that process gets `CLAUDE_CONFIG_DIR` set to the account's folder. For `system`, `claude` runs exactly as it would without cam.

Without that line, `cam exec` does the same:

```sh
cam exec                   # claude, with the account that applies here
cam exec -p "summarize"    # arguments go to claude unchanged
cam exec -- --help         # use -- when an argument could be cam's own
```

If cam ever gets in your way, `CAM_ACCOUNT=system claude` runs Claude Code untouched, even with a broken config. When plain `claude` stops because of a marker or the config, its error ends with that hint. To stop using cam for plain `claude`, delete the `cam init` line.

## Logging in again

Claude Code warns a few days before a login expires. To log in again, run `/login` in a session, or:

```sh
cam login          # the account that applies here
cam login work     # a named account
cam login system   # Claude Code's own setup
cam login work -- --sso            # arguments for `claude auth login`
```

## Checking the setup

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

## Showing the account in Claude Code

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

## Showing the account in your shell prompt

`cam which --short` prints just the name of the account that `claude` would use here, nothing for `system`, and `cam?` when cam can't tell, such as for a marker that names an unknown account (`cam which` or `cam doctor` says why). It takes a few milliseconds and never prints an error, so a prompt can run it every time.

With [Starship](https://starship.rs), add this to `~/.config/starship.toml`:

```toml
[custom.cam]
description = "The Claude Code account that cam picks here"
command = "cam which --short"
when = true
format = "(as [✻ $output]($style) )"
style = "bold #d77757"
```

The prompt then reads `~/Work/api on main as ✻ work`, and shows nothing where `claude` runs as `system`. Change the icon, color and words as you like. Starship's default `format` already includes custom modules. If you've set your own, add `${custom.cam}` where you want it.

Other prompts can do the same with the command's output: `$(cam which --short)` in bash or zsh, `(cam which --short)` in fish.

## Shell completions

`cam init` loads completions with your account names, so `cam use <TAB>` offers them. Accounts you add later show up in new shells. In zsh they need `compinit`, which frameworks like Oh My Zsh run for you: the `cam init` line has to come after it.

For completions without the rest, `cam completions <shell>` prints only the script:

```sh
eval "$(cam completions zsh)"     # ~/.zshrc, after compinit
eval "$(cam completions bash)"    # ~/.bashrc
cam completions fish | source     # ~/.config/fish/config.fish
```

## Removing an account

```sh
cam remove work
```

It asks first, since everything in the account's folder goes, including its transcripts and memory, unless it shares them: what it shares stays in `~/.claude`. `--yes` skips the question, and scripts need it. Then it logs the account out, so its login doesn't stay behind (in the Keychain, on macOS), and deletes its folder and its config entry. If the logout fails, nothing is deleted. A `.claude-account` that still names the account then makes `claude` stop with an error until you change it.
