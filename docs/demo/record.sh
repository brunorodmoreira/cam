#!/bin/sh
# Records a demo with vhs (https://github.com/charmbracelet/vhs), against the
# fake `claude` next to this script and made-up accounts, in a throwaway
# home. Run from the repo root, on macOS:
#   sh docs/demo/record.sh                         # docs/demo.gif, the README's
#   sh docs/demo/record.sh docs/demo/prompt.tape   # docs/prompt.gif, Starship
set -eu
tape=${1:-docs/demo/demo.tape}
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
if [ "$tape" = docs/demo/prompt.tape ]; then
    # The account `work`, already added and used in ~/Work, a repo there and
    # a folder elsewhere, and Starship with the module from docs/usage.md.
    mkdir -p "$home/.config/cam" "$home/.local/share/cam/accounts/work" "$home/oss"
    printf '[accounts.work]\n' > "$home/.config/cam/config.toml"
    printf 'work\n' > "$home/Work/.claude-account"
    git -C "$home/Work/api" init --quiet --initial-branch main
    cat > "$home/.config/starship.toml" <<'EOF'
format = "$directory$git_branch${custom.cam}$line_break$character"

[directory]
truncate_to_repo = false

[git_branch]
format = "on [$branch]($style) "

[custom.cam]
description = "The Claude Code account that cam picks here"
command = "cam which --short"
when = true
format = "(as [✻ $output]($style) )"
style = "bold #d77757"
EOF
fi
vhs "$tape"
