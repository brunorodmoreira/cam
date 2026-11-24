# cam

Rust CLI that picks the Claude Code account per folder via `.claude-account` files. **Read `DESIGN.md` first**; it's the spec. Build milestone by milestone, and update `DESIGN.md` when a decision changes or an open question gets answered.

## Rules

- **cam never handles a credential.** Logins are Claude Code's own: cam only runs `claude auth login`, `auth status --json` and `auth logout`. From `auth status` it reads only the email, org, plan and expiry, and from `.claude.json` only `oauthAccount.emailAddress`, since that file holds MCP server secrets. Never print raw `auth status` output or anything else from `.claude.json`.
- **Never print, log or store a token** or other secret: not in tests, fixtures, error messages, commit messages, or this chat. Redact `sk-ant-…` in any output.
- Tests never run the real `claude` or touch the real `~/.claude`, `~/.claude.json` or Keychain. The `Sandbox` helper in `tests/cli.rs` clears the environment and sets `CAM_CONFIG`, `HOME` (and `USERPROFILE`), `PATH` and `FAKE_LOG`, and the fake `claude` in `examples/fake_claude.rs`, which `cargo test` builds, fakes `claude auth`. Unit tests use temp trees (`testutil::Tree`).
- Before calling something done, run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
- macOS first, then Linux and Windows (M10, see `DESIGN.md`). Code that differs between Unix and Windows goes in `src/platform/`, and the rest stays the same code on both. The exception is the shim's two forms (a symlink, a copy of `cam.exe`), side by side in `src/shim.rs`. On Windows, the tests need Developer Mode, for links to files. Check a change builds for Windows with rustup's toolchain, not Homebrew's: `PATH="$HOME/.cargo/bin:$PATH" RUSTC="$(rustup which rustc)" cargo clippy --target x86_64-pc-windows-msvc --all-targets --target-dir target/windows -- -D warnings`.

## Git

Open source, dual-licensed `MIT OR Apache-2.0`. Commits don't get a `Co-Authored-By` trailer.

- `main` is protected: work on a branch and open a pull request. It merges by squash once CI's `macOS`, `Linux`, `Windows` and `PR title` checks pass. No force pushes.
- **PR titles are conventional commits** (`fix: …`, `feat: …`, `feat!: …` for a breaking change, `docs:`, `refactor:`, `ci:`, `chore:` …), written for the changelog: the squash commit takes the title, and release-plz picks the next version and writes CHANGELOG.md from it. Commits on a branch stay plain sentences. See CONTRIBUTING.md.
- Releases go through release-plz's release pull request, never a hand-pushed tag, and a different version is picked by editing that pull request. Merging it publishes to crates.io and pushes the `v<version>` tag that starts cargo-dist's release (see Milestones in `DESIGN.md`). Nobody can move or delete a release tag.
