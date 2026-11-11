# cam

Rust CLI that picks the Claude Code account per folder via `.claude-account` files. **Read `DESIGN.md` first**; it's the spec. Build milestone by milestone, and update `DESIGN.md` when a decision changes or an open question gets answered.

## Rules

- **Never print, log or store a token** outside the Keychain and the `claude` child's environment. Not in tests, fixtures, error messages, commit messages, or this chat. Redact `sk-ant-…` in any output.
- Tests never touch the real Keychain. Use the test `SecretStore`: `FileStore` in unit tests, and `CAM_TEST_STORE` (set by the `Sandbox` helper in `tests/cli.rs`) for `assert_cmd` tests. `cargo test` enables the `test-store` feature on its own.
- Before calling something done, run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
- Unix-only (macOS first) for v1. Keep Keychain access behind the `SecretStore` trait.

## Git

Open source, dual-licensed `MIT OR Apache-2.0`. Commits don't get a `Co-Authored-By` trailer.
