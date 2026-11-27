# Herdr

[Herdr](https://herdr.dev) runs coding agents in terminal panes and lists them in a sidebar. cam works there as it does anywhere, and adds two things: the account on each `claude`, and a popup to see and change it.

## The account on each `claude`

There's nothing to install. When plain `claude` or `cam exec` runs an account in a Herdr pane, Herdr shows that pane's agent as `claude · work`. A `claude` that runs as `system` stays `claude`, and the label goes when `claude` exits.

## The accounts popup

cam's Herdr plugin opens a popup in the focused pane's folder. It says which account applies there and why, then offers to:

- use an account there, or `system`
- remove the folder's `.claude-account`
- add an account, or log one in again
- run `cam doctor`
- put Herdr's Claude Code hook in the accounts that don't have it (see below)

Install the plugin:

```sh
herdr plugin install brunorodmoreira/cam/integrations/herdr
```

It builds nothing: it runs the `cam` on Herdr's `PATH`, and needs a cam newer than 0.1.4. Herdr has no command palette, so bind the popup to a key in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+alt+a"
type = "plugin_action"
command = "cam.accounts"
description = "cam accounts"
```

Then run `herdr server reload-config`. `herdr plugin action invoke cam.accounts` opens it too.

## Herdr's Claude Code hook

`herdr integration install claude` puts a hook in `~/.claude` that tells Herdr when a session starts. An account that doesn't share `settings` (see [Sharing](sharing.md)) doesn't run it, so Herdr follows that account's `claude` from the screen alone. `cam doctor` lists those accounts. To give one the hook, pick it in the popup, share `settings` with it, or run Herdr's installer in its folder:

```sh
CLAUDE_CONFIG_DIR=~/.local/share/cam/accounts/work herdr integration install claude
```

## Platforms

Tried with Herdr 0.9.5 on macOS. On Linux and Windows it's the same code, but it hasn't been tried there.
