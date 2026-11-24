# Changelog

## [0.1.0](https://github.com/brunorodmoreira/cam/releases/tag/v0.1.0) - 2026-11-23

The first release.

- A `.claude-account` file picks the Claude Code account for a folder and every folder below it. The nearest one up the tree wins, and `CAM_ACCOUNT` overrides it.
- Accounts log in with Claude Code's own login (`cam add`, `cam login`), each in its own Claude Code config folder. cam never handles a credential.
- Plain `claude` picks the account once `eval "$(cam init zsh)"` (or bash, or fish) is at the end of the shell's startup file.
- An account can share parts of your own setup in `~/.claude`: settings, instructions, skills and history (`cam share`, `cam unshare`). Nothing is shared by default.
- `cam which`, `whoami`, `list` and `doctor` say which account applies, why, and who it's logged in as. Shell completions include the account names.
- Builds for macOS and Linux. On Linux the test suite passes, but cam hasn't been tried in a real Claude Code session yet.
