//! Sharing: an account can share groups of Claude Code's own setup in
//! `~/.claude` through links in its dir. Nothing is shared by default.
//! See DESIGN.md, Sharing.
//!
//! cam never writes inside an existing file of Claude Code's. It makes
//! links, renames an account's own copies aside, and creates a missing
//! shared folder or file in `~/.claude`, empty.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::{self, Deserialize, Deserializer};

use crate::account;
use crate::platform;

/// Claude Code's own setup, under the home dir. Accounts share from here,
/// even when the user exports `CLAUDE_CONFIG_DIR` (see DESIGN.md).
const SHARED_DIR: &str = ".claude";

/// A part of `~/.claude` that an account shares or doesn't, as a whole.
/// Claude Code keeps some things together, such as plugins and the
/// settings that enable them, so single files aren't offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Settings,
    Instructions,
    Skills,
    History,
}

impl Group {
    /// Every group, in table order.
    pub const ALL: [Self; 4] = [
        Self::Settings,
        Self::Instructions,
        Self::Skills,
        Self::History,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Instructions => "instructions",
            Self::Skills => "skills",
            Self::History => "history",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.name() == name)
    }

    /// The entries of `~/.claude` the group links.
    pub fn entries(self) -> &'static [Entry] {
        match self {
            Self::Settings => SETTINGS,
            Self::Instructions => INSTRUCTIONS,
            Self::Skills => SKILLS,
            Self::History => HISTORY,
        }
    }

    /// What the group covers, in a few words.
    pub fn covers(self) -> &'static str {
        match self {
            Self::Settings => "permissions, hooks, model, theme, status line, plugins",
            Self::Instructions => "your global instructions",
            Self::Skills => "your own skills, agents, slash commands and output styles",
            Self::History => "transcripts, /resume, auto memory and prompt history",
        }
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Group {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::parse(&name).ok_or_else(|| {
            de::Error::custom(format!(
                "unknown group \"{name}\"; the groups are {}",
                group_list()
            ))
        })
    }
}

/// How an entry is created in `~/.claude` when it's missing there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A folder, created empty.
    Dir,
    /// A file, created with this content, a valid empty form.
    File(&'static str),
    /// A file whose empty form cam doesn't know: linked only once it
    /// exists in `~/.claude`.
    FileIfExists,
}

/// One file or folder of `~/.claude` that a group links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub name: &'static str,
    pub kind: Kind,
}

impl Entry {
    const fn dir(name: &'static str) -> Self {
        Self {
            name,
            kind: Kind::Dir,
        }
    }

    const fn file(name: &'static str, empty: &'static str) -> Self {
        Self {
            name,
            kind: Kind::File(empty),
        }
    }

    /// The name, with a trailing `/` for a folder.
    pub fn shown(self) -> String {
        match self.kind {
            Kind::Dir => format!("{}/", self.name),
            Kind::File(_) | Kind::FileIfExists => self.name.to_owned(),
        }
    }

    /// Where it is in Claude Code's own setup, for messages: `~/.claude/x`.
    pub fn shared_shown(self) -> String {
        format!("~/{SHARED_DIR}/{}", self.shown())
    }
}

const SETTINGS: &[Entry] = &[
    Entry::file("settings.json", "{}\n"),
    Entry {
        name: "keybindings.json",
        kind: Kind::FileIfExists,
    },
    Entry::dir("plugins"),
    Entry::dir("hooks"),
];
const INSTRUCTIONS: &[Entry] = &[Entry::file("CLAUDE.md", ""), Entry::dir("rules")];
const SKILLS: &[Entry] = &[
    Entry::dir("skills"),
    Entry::dir("agents"),
    Entry::dir("commands"),
    Entry::dir("output-styles"),
];
const HISTORY: &[Entry] = &[
    Entry::dir("projects"),
    Entry::file("history.jsonl", ""),
    Entry::dir("file-history"),
    Entry::dir("todos"),
    Entry::dir("plans"),
];

/// "a, b and c".
pub fn and_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "settings, instructions, skills and history".
pub fn group_list() -> String {
    and_list(&Group::ALL.map(Group::name))
}

/// Groups as "settings and skills", or "nothing".
pub fn describe(groups: &[Group]) -> String {
    match groups {
        [] => "nothing".to_owned(),
        _ => and_list(&groups.iter().map(|group| group.name()).collect::<Vec<_>>()),
    }
}

/// `groups` in table order, without repeats.
pub fn normalize(mut groups: Vec<Group>) -> Vec<Group> {
    groups.sort();
    groups.dedup();
    groups
}

/// What a group list on the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Groups(Vec<Group>),
    /// `none`, which only `--share` takes.
    Nothing,
}

/// Why a group list didn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadGroups {
    /// No group in it, as with `--share ""`.
    Empty,
    /// `none` together with groups.
    NoneWithGroups,
    Unknown {
        name: String,
        hint: String,
    },
}

/// Parses group lists in every syntax, which mix: comma-separated
/// (`settings,skills`), separate words, or repeated flags, each an item of
/// `args`. `all` is every group. The result is in table order, without
/// repeats.
pub fn parse_groups<S: AsRef<str>>(args: &[S]) -> Result<Parsed, BadGroups> {
    let words: Vec<&str> = args
        .iter()
        .flat_map(|arg| arg.as_ref().split(','))
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return Err(BadGroups::Empty);
    }
    if words.contains(&"none") {
        return match words.iter().all(|word| *word == "none") {
            true => Ok(Parsed::Nothing),
            false => Err(BadGroups::NoneWithGroups),
        };
    }
    let mut groups = Vec::new();
    for word in words {
        if word == "all" {
            groups.extend(Group::ALL);
            continue;
        }
        match Group::parse(word) {
            Some(group) => groups.push(group),
            None => {
                return Err(BadGroups::Unknown {
                    name: word.to_owned(),
                    hint: group_hint(word),
                });
            }
        }
    }
    Ok(Parsed::Groups(normalize(groups)))
}

/// The hint for an unknown group: a close name, then the groups.
fn group_hint(word: &str) -> String {
    let groups = format!("the groups are {}, or `all`", group_list());
    match account::closest(&word.to_ascii_lowercase(), Group::ALL.map(Group::name)) {
        Some(close) => format!("did you mean `{close}`? {}", capitalized(&groups)),
        None => groups,
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Where `entry` lives in Claude Code's own setup: `<home>/.claude/<name>`.
pub fn shared_path(home: &Path, entry: Entry) -> PathBuf {
    home.join(SHARED_DIR).join(entry.name)
}

/// What is at an entry's place in an account dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A link to its place in `~/.claude`.
    Linked,
    /// Nothing: the next launch links it.
    Missing,
    /// Nothing, and nothing to link yet: `~/.claude` doesn't have it, and
    /// cam doesn't know its empty form.
    NotYet,
    /// The account's own, so not shared.
    Own(Own),
}

/// An account's own entry, where a link to `~/.claude` would go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Own {
    File,
    Dir,
    /// A link somewhere other than `~/.claude`.
    Link(PathBuf),
}

impl Own {
    /// "a file", "a folder" or "a link to …".
    pub fn describe(&self) -> String {
        match self {
            Self::File => "a file".to_owned(),
            Self::Dir => "a folder".to_owned(),
            Self::Link(to) => format!("a link to {}", to.display()),
        }
    }
}

/// What is at `entry`'s place in `account_dir`.
pub fn state(home: &Path, account_dir: &Path, entry: Entry) -> State {
    let path = account_dir.join(entry.name);
    let target = shared_path(home, entry);
    match fs::symlink_metadata(&path) {
        Err(_) if entry.kind == Kind::FileIfExists && !account::exists(&target) => State::NotYet,
        Err(_) => State::Missing,
        Ok(meta) if meta.file_type().is_symlink() => {
            let points_to = fs::read_link(&path).unwrap_or_default();
            if points_to == target || same_place(&path, &target) {
                State::Linked
            } else {
                State::Own(Own::Link(points_to))
            }
        }
        Ok(meta) if meta.is_dir() => State::Own(Own::Dir),
        Ok(_) => State::Own(Own::File),
    }
}

/// Whether two paths lead to the same existing file or folder.
fn same_place(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

/// The state of every entry of `groups` in `account_dir`.
pub fn states(home: &Path, account_dir: &Path, groups: &[Group]) -> Vec<(Group, Entry, State)> {
    groups
        .iter()
        .flat_map(|&group| {
            group
                .entries()
                .iter()
                .map(move |&entry| (group, entry, state(home, account_dir, entry)))
        })
        .collect()
}

/// What [`top_up`] did.
#[derive(Debug, Default)]
pub struct TopUp {
    /// Entries it linked.
    pub linked: Vec<Entry>,
    /// The account's own entries, left alone.
    pub own: Vec<(Group, Entry, Own)>,
    /// Entries whose link couldn't be made.
    pub failed: Vec<(Entry, io::Error)>,
}

/// Links each missing entry of `groups` in `account_dir` to its place in
/// `~/.claude`, creating that place empty if needed. The account's own
/// files and folders are never replaced: they're reported in `own`.
pub fn top_up(home: &Path, account_dir: &Path, groups: &[Group]) -> TopUp {
    let mut result = TopUp::default();
    for (group, entry, state) in states(home, account_dir, groups) {
        match state {
            State::Missing => match link(home, account_dir, entry) {
                Ok(()) => result.linked.push(entry),
                Err(err) => result.failed.push((entry, err)),
            },
            State::Own(own) => result.own.push((group, entry, own)),
            State::Linked | State::NotYet => {}
        }
    }
    result
}

/// Makes the link for `entry`, after making sure its target exists.
fn link(home: &Path, account_dir: &Path, entry: Entry) -> io::Result<()> {
    let target = shared_path(home, entry);
    ensure_shared(home, &target, entry)?;
    match platform::symlink(&target, &account_dir.join(entry.name)) {
        // Another launch got there first.
        Err(err)
            if err.kind() == io::ErrorKind::AlreadyExists
                && state(home, account_dir, entry) == State::Linked =>
        {
            Ok(())
        }
        other => other,
    }
}

/// Makes sure `target` exists in `~/.claude`. A missing folder is created
/// empty, and a missing file with its empty form, so the account and
/// Claude Code's own setup write to the same one from the start.
fn ensure_shared(home: &Path, target: &Path, entry: Entry) -> io::Result<()> {
    let shared = home.join(SHARED_DIR);
    if !account::exists(&shared) {
        platform::private_dir().recursive(true).create(&shared)?;
    }
    let ignore_exists = |result: io::Result<()>| match result {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    };
    match entry.kind {
        Kind::Dir => match fs::metadata(target) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(io::Error::other(format!(
                "{} isn't a folder",
                target.display()
            ))),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                ignore_exists(platform::private_dir().create(target))
            }
            Err(err) => Err(err),
        },
        Kind::File(empty) => match fs::symlink_metadata(target) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => ignore_exists(
                platform::private_file()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .and_then(|mut file| file.write_all(empty.as_bytes())),
            ),
            Err(err) => Err(err),
        },
        Kind::FileIfExists => Ok(()),
    }
}

/// Renames the account's own `entry` to the first free name of
/// `<name>.before-share-<date>`, `-2`, `-3` and so on, and returns its new
/// path. Nothing is deleted.
pub fn move_aside(account_dir: &Path, entry: Entry, date: &str) -> io::Result<PathBuf> {
    let base = format!("{}.before-share-{date}", entry.name);
    let free = (1..)
        .map(|n| match n {
            1 => account_dir.join(&base),
            _ => account_dir.join(format!("{base}-{n}")),
        })
        .find(|path| !account::exists(path))
        .unwrap_or_else(|| account_dir.join(&base));
    fs::rename(account_dir.join(entry.name), &free)?;
    Ok(free)
}

/// The entries `share` moved aside in `account_dir`, by name.
pub fn set_aside(account_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(account_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains(".before-share-"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// What [`unlink`] did with an entry.
#[derive(Debug)]
pub enum Unlinked {
    /// Removed the link.
    Removed,
    /// Replaced the link with a copy of what it pointed to.
    Copied,
    /// Left it, since it isn't a link to `~/.claude`.
    Skipped(State),
}

/// Removes `entry`'s link in `account_dir`, and nothing else. With `copy`,
/// puts a copy of what it pointed to in its place, made beside it first,
/// so a failed copy leaves the link.
pub fn unlink(home: &Path, account_dir: &Path, entry: Entry, copy: bool) -> io::Result<Unlinked> {
    let path = account_dir.join(entry.name);
    match state(home, account_dir, entry) {
        State::Linked => {
            let target = shared_path(home, entry);
            if copy && fs::metadata(&target).is_ok() {
                let tmp = account_dir.join(format!(".{}.cam-copy", entry.name));
                remove_any(&tmp)?;
                if let Err(err) = copy_tree(&target, &tmp) {
                    let _ = remove_any(&tmp);
                    return Err(err);
                }
                platform::remove_link(&path)?;
                fs::rename(&tmp, &path)?;
                Ok(Unlinked::Copied)
            } else {
                platform::remove_link(&path)?;
                Ok(Unlinked::Removed)
            }
        }
        other => Ok(Unlinked::Skipped(other)),
    }
}

/// Removes whatever is at `path`, without following links. Nothing there
/// is fine.
fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => platform::remove_link(path),
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Copies `from` to `to`: a file with its permissions, a folder with
/// everything in it. A link at `from` itself is followed, but links inside
/// a folder are copied as links.
pub fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::metadata(from)?;
    if meta.is_dir() {
        copy_dir(from, to, meta.permissions())
    } else {
        fs::copy(from, to).map(drop)
    }
}

fn copy_dir(from: &Path, to: &Path, permissions: fs::Permissions) -> io::Result<()> {
    // Writable while it's filled, then given the original's permissions.
    platform::private_dir().create(to)?;
    for item in fs::read_dir(from)? {
        let item = item?;
        let (source, dest) = (item.path(), to.join(item.file_name()));
        let kind = item.file_type()?;
        if kind.is_symlink() {
            platform::copy_link(&source, &dest)?;
        } else if kind.is_dir() {
            copy_dir(&source, &dest, fs::symlink_metadata(&source)?.permissions())?;
        } else if kind.is_file() {
            fs::copy(&source, &dest)?;
        }
        // Sockets and pipes are left out.
    }
    fs::set_permissions(to, permissions)
}

/// The bytes [`copy_tree`] would copy from `path`.
pub fn size(path: &Path) -> u64 {
    fn walk(path: &Path, meta: &fs::Metadata) -> u64 {
        if !meta.is_dir() {
            return meta.len();
        }
        fs::read_dir(path)
            .map(|items| {
                items
                    .filter_map(Result::ok)
                    .filter_map(|item| {
                        let meta = fs::symlink_metadata(item.path()).ok()?;
                        Some(match meta.file_type().is_symlink() {
                            true => 0,
                            false => walk(&item.path(), &meta),
                        })
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    fs::metadata(path).map_or(0, |meta| walk(path, &meta))
}

/// A byte count for people: `512 B`, `3.4 MB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// What `group` would share today: the entries that exist in `~/.claude`,
/// and how many entries each folder holds, as `settings.json, plugins/ (4)`.
pub fn preview(home: &Path, group: Group) -> String {
    let parts: Vec<String> = group
        .entries()
        .iter()
        .filter_map(|&entry| {
            let path = shared_path(home, entry);
            let meta = fs::metadata(&path).ok()?;
            Some(match meta.is_dir() {
                true => {
                    let count = fs::read_dir(&path).map_or(0, Iterator::count);
                    format!("{} ({count})", entry.shown())
                }
                false => entry.name.to_owned(),
            })
        })
        .collect();
    match parts.is_empty() {
        true => format!("nothing in ~/{SHARED_DIR} yet"),
        false => parts.join(", "),
    }
}

// Unix for now: Windows makes other kinds of links, with tests of their own
// (see DESIGN.md, Windows).
#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::testutil::Tree;

    fn groups(args: &[&str]) -> Vec<Group> {
        match parse_groups(args).unwrap() {
            Parsed::Groups(groups) => groups,
            Parsed::Nothing => panic!("{args:?} parsed as none"),
        }
    }

    #[test]
    fn every_syntax_parses_to_the_same_list() {
        let expected = vec![Group::Settings, Group::Skills];
        for args in [
            &["settings,skills"][..],
            &["skills,settings"],
            &["settings", "skills"],
            &["settings, skills"],
            &["skills", "settings,skills"],
            &["settings,", ",skills"],
        ] {
            assert_eq!(groups(args), expected, "{args:?}");
        }
        assert_eq!(groups(&["all"]), Group::ALL);
        assert_eq!(groups(&["history", "all"]), Group::ALL);
    }

    #[test]
    fn none_is_alone_and_empty_is_an_error() {
        assert_eq!(parse_groups(&["none"]), Ok(Parsed::Nothing));
        assert_eq!(parse_groups(&["none,none"]), Ok(Parsed::Nothing));
        assert_eq!(
            parse_groups(&["none", "settings"]),
            Err(BadGroups::NoneWithGroups)
        );
        assert_eq!(parse_groups(&[""]), Err(BadGroups::Empty));
        assert_eq!(parse_groups::<&str>(&[]), Err(BadGroups::Empty));
    }

    #[test]
    fn unknown_groups_get_a_hint() {
        let Err(BadGroups::Unknown { name, hint }) = parse_groups(&["settings", "skils"]) else {
            panic!("should fail");
        };
        assert_eq!(name, "skils");
        assert_eq!(
            hint,
            "did you mean `skills`? The groups are settings, instructions, skills and history, or `all`"
        );
        let Err(BadGroups::Unknown { hint, .. }) = parse_groups(&["Settings"]) else {
            panic!("should fail");
        };
        assert!(hint.starts_with("did you mean `settings`?"), "{hint}");
        let Err(BadGroups::Unknown { hint, .. }) = parse_groups(&["mcp"]) else {
            panic!("should fail");
        };
        assert_eq!(
            hint,
            "the groups are settings, instructions, skills and history, or `all`"
        );
    }

    #[test]
    fn describes_groups() {
        assert_eq!(describe(&[]), "nothing");
        assert_eq!(describe(&[Group::Skills]), "skills");
        assert_eq!(
            describe(&Group::ALL),
            "settings, instructions, skills and history"
        );
    }

    /// A home with `~/.claude`, and an empty account dir.
    fn setup() -> (Tree, PathBuf, PathBuf) {
        let tree = Tree::new();
        let home = tree.dir("home");
        let account = tree.dir("home/.local/share/cam/accounts/work");
        (tree, home, account)
    }

    #[test]
    fn top_up_links_and_creates_what_is_missing() {
        let (_tree, home, account) = setup();
        let result = top_up(&home, &account, &[Group::Settings, Group::History]);
        assert!(
            result.own.is_empty() && result.failed.is_empty(),
            "{result:?}"
        );
        let claude = home.join(".claude");
        for name in [
            "settings.json",
            "plugins",
            "hooks",
            "projects",
            "history.jsonl",
            "todos",
        ] {
            assert_eq!(
                fs::read_link(account.join(name)).unwrap(),
                claude.join(name),
                "{name}"
            );
        }
        assert_eq!(
            fs::read_to_string(claude.join("settings.json")).unwrap(),
            "{}\n"
        );
        assert_eq!(
            fs::read_to_string(claude.join("history.jsonl")).unwrap(),
            ""
        );
        assert!(claude.join("plugins").is_dir());
        // Its empty form is unknown, so it waits until Claude Code makes one.
        assert!(!account::exists(&account.join("keybindings.json")));
        assert!(!account::exists(&claude.join("keybindings.json")));
        // Groups not asked for stay out.
        assert!(!account::exists(&account.join("CLAUDE.md")));
        assert!(!account::exists(&account.join("skills")));
    }

    #[test]
    fn top_up_keeps_existing_shared_files_and_links_keybindings_once_they_exist() {
        let (_tree, home, account) = setup();
        let claude = home.join(".claude");
        fs::create_dir(&claude).unwrap();
        fs::write(claude.join("settings.json"), "{\"model\": \"x\"}\n").unwrap();
        top_up(&home, &account, &[Group::Settings]);
        assert_eq!(
            fs::read_to_string(account.join("settings.json")).unwrap(),
            "{\"model\": \"x\"}\n"
        );
        let keybindings = Entry {
            name: "keybindings.json",
            kind: Kind::FileIfExists,
        };
        assert_eq!(state(&home, &account, keybindings), State::NotYet);
        fs::write(claude.join("keybindings.json"), "{}\n").unwrap();
        assert_eq!(state(&home, &account, keybindings), State::Missing);
        top_up(&home, &account, &[Group::Settings]);
        assert_eq!(state(&home, &account, keybindings), State::Linked);
    }

    #[test]
    fn top_up_never_replaces_the_accounts_own_entries() {
        let (_tree, home, account) = setup();
        fs::write(account.join("settings.json"), "mine\n").unwrap();
        fs::create_dir(account.join("plugins")).unwrap();
        symlink("/elsewhere/hooks", account.join("hooks")).unwrap();
        let result = top_up(&home, &account, &[Group::Settings]);
        let own: Vec<(&str, Own)> = result
            .own
            .into_iter()
            .map(|(_, entry, own)| (entry.name, own))
            .collect();
        assert_eq!(
            own,
            [
                ("settings.json", Own::File),
                ("plugins", Own::Dir),
                ("hooks", Own::Link("/elsewhere/hooks".into())),
            ]
        );
        assert_eq!(
            fs::read_to_string(account.join("settings.json")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn a_link_by_another_spelling_counts() {
        let (tree, home, account) = setup();
        let claude = tree.dir("home/.claude/skills");
        symlink(
            home.join(".claude/../.claude/skills"),
            account.join("skills"),
        )
        .unwrap();
        assert!(claude.is_dir());
        assert_eq!(
            state(&home, &account, Group::Skills.entries()[0]),
            State::Linked
        );
    }

    #[test]
    fn top_up_reports_a_shared_folder_that_is_a_file() {
        let (tree, home, account) = setup();
        tree.dir("home/.claude");
        fs::write(home.join(".claude/skills"), "not a folder\n").unwrap();
        let result = top_up(&home, &account, &[Group::Skills]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0.name, "skills");
        assert!(!account::exists(&account.join("skills")));
    }

    #[test]
    fn move_aside_picks_a_free_name() {
        let (_tree, _home, account) = setup();
        let entry = SETTINGS[0];
        for expected in [
            "settings.json.before-share-2026-11-20",
            "settings.json.before-share-2026-11-20-2",
            "settings.json.before-share-2026-11-20-3",
        ] {
            fs::write(account.join("settings.json"), expected).unwrap();
            let moved = move_aside(&account, entry, "2026-11-20").unwrap();
            assert_eq!(moved, account.join(expected));
            assert_eq!(fs::read_to_string(&moved).unwrap(), expected);
        }
        assert!(!account::exists(&account.join("settings.json")));
        assert_eq!(set_aside(&account).len(), 3);
    }

    #[test]
    fn unlink_removes_only_links() {
        let (_tree, home, account) = setup();
        top_up(&home, &account, &[Group::Skills]);
        fs::write(home.join(".claude/skills/keep.md"), "mine\n").unwrap();
        fs::remove_file(account.join("agents")).unwrap();
        fs::create_dir(account.join("agents")).unwrap();
        let [skills, agents, ..] = SKILLS else {
            unreachable!()
        };
        assert!(matches!(
            unlink(&home, &account, *skills, false).unwrap(),
            Unlinked::Removed
        ));
        assert!(matches!(
            unlink(&home, &account, *agents, false).unwrap(),
            Unlinked::Skipped(State::Own(Own::Dir))
        ));
        assert!(!account::exists(&account.join("skills")));
        assert!(account.join("agents").is_dir());
        assert_eq!(
            fs::read_to_string(home.join(".claude/skills/keep.md")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn unlink_with_copy_keeps_a_copy_and_links_inside_stay_links() {
        let (_tree, home, account) = setup();
        top_up(&home, &account, &[Group::Skills, Group::Instructions]);
        let skills = home.join(".claude/skills");
        fs::create_dir(skills.join("deep")).unwrap();
        fs::write(skills.join("deep/a.md"), "a\n").unwrap();
        symlink("/somewhere/else", skills.join("pointer")).unwrap();
        fs::write(home.join(".claude/CLAUDE.md"), "be brief\n").unwrap();

        assert!(matches!(
            unlink(&home, &account, SKILLS[0], true).unwrap(),
            Unlinked::Copied
        ));
        assert!(matches!(
            unlink(&home, &account, INSTRUCTIONS[0], true).unwrap(),
            Unlinked::Copied
        ));
        let copy = account.join("skills");
        assert!(
            !fs::symlink_metadata(&copy)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(copy.join("deep/a.md")).unwrap(), "a\n");
        assert_eq!(
            fs::read_link(copy.join("pointer")).unwrap(),
            PathBuf::from("/somewhere/else")
        );
        assert_eq!(
            fs::read_to_string(account.join("CLAUDE.md")).unwrap(),
            "be brief\n"
        );
        // The shared copies are untouched.
        assert_eq!(fs::read_to_string(skills.join("deep/a.md")).unwrap(), "a\n");
        assert_eq!(
            fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
            "be brief\n"
        );
    }

    #[test]
    fn preview_counts_what_exists() {
        let (tree, home, _account) = setup();
        assert_eq!(preview(&home, Group::Settings), "nothing in ~/.claude yet");
        let plugins = tree.dir("home/.claude/plugins");
        fs::write(plugins.join("a"), "").unwrap();
        fs::write(plugins.join("b"), "").unwrap();
        fs::write(home.join(".claude/settings.json"), "{}").unwrap();
        tree.dir("home/.claude/hooks");
        assert_eq!(
            preview(&home, Group::Settings),
            "settings.json, plugins/ (2), hooks/ (0)"
        );
    }

    #[test]
    fn sizes() {
        let (tree, home, _account) = setup();
        let projects = tree.dir("home/.claude/projects/a");
        fs::write(projects.join("t.jsonl"), vec![b'x'; 1500]).unwrap();
        symlink("/dev/zero", projects.join("link")).unwrap();
        assert_eq!(size(&home.join(".claude/projects")), 1500);
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1500), "1.5 KB");
        assert_eq!(human_size(3_400_000), "3.4 MB");
        assert_eq!(human_size(2_000_000_000), "2.0 GB");
    }
}
