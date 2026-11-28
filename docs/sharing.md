# Sharing

Nothing, unless you choose. An account can share groups of your own setup in `~/.claude`, through links in its folder. A shared group is the same files for both, so a change in either account shows up in the other.

| Group | Links | Covers |
|---|---|---|
| `settings` | `settings.json`, `keybindings.json`, `plugins/`, `hooks/` | permissions, hooks, model, theme, status line, plugins |
| `instructions` | `CLAUDE.md`, `rules/` | your global instructions |
| `skills` | `skills/`, `agents/`, `commands/`, `output-styles/` | your own skills, agents, slash commands and output styles |
| `history` | `projects/`, `history.jsonl`, `file-history/`, `todos/`, `plans/` | transcripts, `/resume`, auto memory, prompt history |

```sh
cam add work --share settings,skills   # or answer when cam asks
cam share work history                 # share more later
cam share work                         # tick and untick, on a terminal
cam unshare work history               # stop: removes the links, your files stay
cam unshare work settings --copy       # stop, and keep a copy of the current files
```

Groups go comma-separated or as separate words, and `all` is every group. `cam list` and `cam whoami` show what each account shares.

An account that doesn't share `skills` gets [cam's own skill](usage.md#cams-skill-for-claude-code) in its `skills/` folder. Sharing `skills` replaces that folder with `~/.claude/skills`, which has cam's skill only if you installed it there. When the folder holds nothing but cam's skill, `cam share` just removes it, with nothing to move aside.

Sharing goes both ways, so choose with that in mind. With `settings`, the same hooks and plugins run in both accounts. With `history`, work transcripts and memory sit next to personal ones in `~/.claude/projects`. An org's managed settings and policy caches always stay in its own account.

MCP servers are never shared: Claude Code keeps them in `.claude.json`, next to the login. Add them per account with `CAM_ACCOUNT=work claude mcp add -s user …`, or get them from a plugin in a shared `settings`. Each account also goes through Claude Code's first-run screens once.

When an account already has its own copy of something it starts to share, such as its own `settings.json`, `cam share` asks, then renames it to `settings.json.before-share-<date>`. Nothing is deleted: copy what you want from it into `~/.claude`. `cam doctor` lists these copies until you delete them.

On Windows, folders link through junctions, and files through symlinks, which Windows makes only with [Developer Mode](https://learn.microsoft.com/windows/apps/get-started/enable-your-device-for-development) on. Without it, `cam add` and `cam share` refuse a group that has a file in it (`settings`, `instructions` and `history` do) before changing anything, and `skills`, which is only folders, still works.

Every launch adds missing links, and never replaces a real file. If something saved a shared file by replacing it, cam says so at launch, and `cam share work settings` links it again. Claude Code's own writes keep the links (checked with Claude Code 2.1.330), and so do saves from vim and Zed. `cam doctor` checks each link.
