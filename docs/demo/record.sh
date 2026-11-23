#!/bin/sh
# Records docs/demo.gif with vhs (https://github.com/charmbracelet/vhs),
# against the fake `claude` next to this script and made-up accounts, in a
# throwaway home. Run from the repo root, on macOS: sh docs/demo/record.sh
set -eu
home=/Users/Shared/me
[ ! -e "$home" ] || { echo "$home exists; move it away first" >&2; exit 1; }
cargo build --release --quiet
trap 'rm -rf "$home"' EXIT
mkdir -p "$home/bin" "$home/Work/api" "$home/.claude/plugins/a" "$home/.claude/plugins/b" \
    "$home/.claude/skills/review" "$home/.claude/skills/release" "$home/.claude/projects"
cp target/release/cam docs/demo/claude "$home/bin/"
# Your own login, `system`, as Claude Code would have it.
printf '{"oauthAccount": {"emailAddress": "me@example.com"}}\n' > "$home/.claude.json"
printf 'me@example.com\n' > "$home/.claude.json.email"
printf '{}\n' > "$home/.claude/settings.json"
printf '# Be brief.\n' > "$home/.claude/CLAUDE.md"
: > "$home/.claude/history.jsonl"
vhs docs/demo/demo.tape
