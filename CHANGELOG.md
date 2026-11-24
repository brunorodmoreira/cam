# Changelog

## [0.1.3](https://github.com/brunorodmoreira/cam/compare/v0.1.2...v0.1.3) - 2026-11-24

### Added

- Support Windows ([#16](https://github.com/brunorodmoreira/cam/pull/16))

## [0.1.2](https://github.com/brunorodmoreira/cam/compare/v0.1.1...v0.1.2) - 2026-11-24

### Dependencies

- *(deps)* bump the cargo group across 1 directory with 4 updates ([#7](https://github.com/brunorodmoreira/cam/pull/7))

### Fixed

- Warn when CLAUDE_SECURESTORAGE_CONFIG_DIR makes every account share one login ([#8](https://github.com/brunorodmoreira/cam/pull/8))

## [0.1.1](https://github.com/brunorodmoreira/cam/compare/v0.1.0...v0.1.1) - 2026-11-24

### Other

- Shorten the README and move the reference into docs/ ([#3](https://github.com/brunorodmoreira/cam/pull/3))
- Run macOS CI and the Apple Silicon build on Intel runners ([#4](https://github.com/brunorodmoreira/cam/pull/4))
- Remove token-mode leftovers, tidy the README, add Dependabot and a security policy ([#1](https://github.com/brunorodmoreira/cam/pull/1))
- Publish to crates.io from the release workflow

## [0.1.0](https://github.com/brunorodmoreira/cam/releases/tag/v0.1.0) - 2026-11-23

The first release.

- A `.claude-account` file picks the Claude Code account for a folder and every folder below it. The nearest one up the tree wins, and `CAM_ACCOUNT` overrides it.
- Accounts log in with Claude Code's own login (`cam add`, `cam login`), each in its own Claude Code config folder. cam never handles a credential.
- Plain `claude` picks the account once `eval "$(cam init zsh)"` (or bash, or fish) is at the end of the shell's startup file.
- An account can share parts of your own setup in `~/.claude`: settings, instructions, skills and history (`cam share`, `cam unshare`). Nothing is shared by default.
- `cam which`, `whoami`, `list` and `doctor` say which account applies, why, and who it's logged in as. Shell completions include the account names.
- Builds for macOS and Linux. On Linux the test suite passes, but cam hasn't been tried in a real Claude Code session yet.
