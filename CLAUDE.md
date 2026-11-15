# cam

Rust CLI that picks the Claude Code account per folder via `.claude-account` files. **Read `DESIGN.md` first**; it's the spec. Build milestone by milestone, and update `DESIGN.md` when a decision changes or an open question gets answered.

## Rules

- **cam never handles a credential.** Logins are Claude Code's own: cam only runs `claude auth login`, `auth status --json` and `auth logout`. From `auth status` it reads only the email, org, plan and expiry, and from `.claude.json` only `oauthAccount.emailAddress`, since that file holds MCP server secrets. Never print raw `auth status` output or anything else from `.claude.json`.
- **Never print, log or store a token** or other secret: not in tests, fixtures, error messages, commit messages, or this chat. Redact `sk-ant-…` in any output.
- Tests never run the real `claude` or touch the real `~/.claude`, `~/.claude.json` or Keychain. The `Sandbox` helper in `tests/cli.rs` clears the environment and sets `CAM_CONFIG`, `HOME`, `PATH` and `FAKE_LOG`, and its `FAKE_CLAUDE` script fakes `claude auth`. Unit tests use temp trees (`testutil::Tree`).
- Before calling something done, run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
- Unix-only (macOS first) for v1.

## Git

Open source, dual-licensed `MIT OR Apache-2.0`. Commits don't get a `Co-Authored-By` trailer.
