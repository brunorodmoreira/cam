//! cam's own skill for Claude Code, `skills/cam/SKILL.md`, which tells a
//! session how to use cam. It's built into the binary, so it always matches
//! the cam that runs. See DESIGN.md, Skill.
//!
//! cam writes it to a folder of its own, and links it into each account's
//! own `skills/` folder. An account that shares `skills` uses `~/.claude`'s,
//! where cam never writes, so it gets none from cam.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use crate::account;
use crate::platform;
use crate::share;

/// The skill's name, and the name of its folder.
pub const NAME: &str = "cam";

/// The skill itself, as `npx skills add` finds it in this repo.
pub const SKILL: &str = include_str!("../skills/cam/SKILL.md");

/// What puts the skill in `~/.claude/skills`, for `system` and the
/// accounts that share `skills`.
pub const INSTALL: &str = "npx skills add brunorodmoreira/cam -g -a claude-code";

/// The skill's file in its folder.
const FILE: &str = "SKILL.md";

/// The folder of skills in a Claude Code config dir.
pub const SKILLS: &str = "skills";

/// Where cam keeps the skill that accounts link to, under the home dir.
const DIR: [&str; 4] = [".local", "share", "cam", SKILLS];

/// cam's copy of the skill: `<home>/.local/share/cam/skills/cam`.
pub fn source(home: &Path) -> PathBuf {
    let mut dir = home.to_owned();
    dir.extend(DIR);
    dir.join(NAME)
}

/// What is at `skills/cam` in an account dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A link to cam's copy.
    Linked,
    /// Nothing, in a `skills/` folder of the account's own or with no
    /// `skills/` at all: the next launch links it.
    Missing,
    /// The account's `skills/` is a link, to `~/.claude` when it shares
    /// `skills`, so cam leaves it alone.
    SharedFolder,
    /// Something of the account's own, such as a skill of its own named
    /// `cam`, or a `skills` that isn't a folder. cam leaves it alone.
    Own,
}

/// What is at `skills/cam` in `account_dir`.
pub fn state(home: &Path, account_dir: &Path) -> State {
    let skills = account_dir.join(SKILLS);
    match fs::symlink_metadata(&skills) {
        Err(_) => return State::Missing,
        Ok(meta) if meta.file_type().is_symlink() => return State::SharedFolder,
        Ok(meta) if !meta.is_dir() => return State::Own,
        Ok(_) => {}
    }
    let path = skills.join(NAME);
    let target = source(home);
    match fs::symlink_metadata(&path) {
        Err(_) => State::Missing,
        Ok(meta) if meta.file_type().is_symlink() => {
            let points_to = fs::read_link(&path).unwrap_or_default();
            match points_to == target || share::same_place(&path, &target) {
                true => State::Linked,
                false => State::Own,
            }
        }
        Ok(_) => State::Own,
    }
}

/// Before a launch or a login: with `wanted`, links cam's skill into the
/// account's own `skills/` folder, creating that folder if needed, and
/// writes cam's copy if it's missing or from another version. Without it,
/// as when the account shares `skills` or the config turns the skill off,
/// removes cam's link. Anything of the account's own is left alone.
pub fn top_up(home: &Path, account_dir: &Path, wanted: bool) -> io::Result<()> {
    match (wanted, state(home, account_dir)) {
        (true, State::Missing) => link(home, account_dir),
        (true, State::Linked) => write_source(home).map(drop),
        (false, State::Linked) => unlink(home, account_dir).map(drop),
        _ => Ok(()),
    }
}

fn link(home: &Path, account_dir: &Path) -> io::Result<()> {
    let source = write_source(home)?;
    let skills = account_dir.join(SKILLS);
    if !account::exists(&skills) {
        ignore_exists(platform::private_dir().create(&skills))?;
    }
    match platform::link(&source, &skills.join(NAME)) {
        // Another launch got there first.
        Err(err)
            if err.kind() == io::ErrorKind::AlreadyExists
                && state(home, account_dir) == State::Linked =>
        {
            Ok(())
        }
        other => other,
    }
}

/// Writes the skill to cam's copy, unless it's there already, and returns
/// the copy's folder. The file is written beside it and renamed into place,
/// so a session never reads half of it.
fn write_source(home: &Path) -> io::Result<PathBuf> {
    let dir = source(home);
    let file = dir.join(FILE);
    if fs::read(&file).is_ok_and(|text| text == SKILL.as_bytes()) {
        return Ok(dir);
    }
    fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{FILE}.{}.tmp", process::id()));
    let written = fs::write(&tmp, SKILL).and_then(|()| fs::rename(&tmp, &file));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map(|()| dir)
}

/// Removes cam's link from the account's `skills/` folder, and the folder
/// too if that leaves it empty, as when cam made it. Returns whether there
/// was a link to remove.
pub fn unlink(home: &Path, account_dir: &Path) -> io::Result<bool> {
    if state(home, account_dir) != State::Linked {
        return Ok(false);
    }
    let skills = account_dir.join(SKILLS);
    platform::remove_link(&skills.join(NAME))?;
    if fs::read_dir(&skills)?.next().is_none() {
        fs::remove_dir(&skills)?;
    }
    Ok(true)
}

/// Whether the account's `skills/` folder holds cam's link and nothing
/// else, so it's cam's to remove rather than the account's own.
pub fn only_link(home: &Path, account_dir: &Path) -> bool {
    state(home, account_dir) == State::Linked
        && fs::read_dir(account_dir.join(SKILLS)).is_ok_and(|items| items.count() == 1)
}

/// Whether `~/.claude/skills` has a skill named `cam`, as `npx skills add`
/// puts there.
pub fn in_claude(home: &Path) -> bool {
    home.join(share::SHARED_DIR)
        .join(SKILLS)
        .join(NAME)
        .join(FILE)
        .is_file()
}

fn ignore_exists(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    /// A home and an empty account dir.
    fn setup() -> (Tree, PathBuf, PathBuf) {
        let tree = Tree::new();
        let home = tree.dir("home");
        let account = tree.dir("home/.local/share/cam/accounts/work");
        (tree, home, account)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_skill_names_itself_cam() {
        assert!(
            SKILL.starts_with("---\nname: cam\ndescription: "),
            "{SKILL}"
        );
    }

    #[test]
    fn top_up_links_the_skill_into_a_new_account() {
        let (_tree, home, account) = setup();
        assert_eq!(state(&home, &account), State::Missing);
        top_up(&home, &account, true).unwrap();
        assert_eq!(state(&home, &account), State::Linked);
        assert_eq!(read(&account.join("skills/cam/SKILL.md")), SKILL);
        assert_eq!(read(&source(&home).join("SKILL.md")), SKILL);
        // Again changes nothing.
        top_up(&home, &account, true).unwrap();
        assert_eq!(state(&home, &account), State::Linked);
        assert!(only_link(&home, &account));
    }

    #[test]
    fn top_up_writes_the_skill_of_this_version() {
        let (_tree, home, account) = setup();
        top_up(&home, &account, true).unwrap();
        fs::write(source(&home).join("SKILL.md"), "an older cam's\n").unwrap();
        top_up(&home, &account, true).unwrap();
        assert_eq!(read(&account.join("skills/cam/SKILL.md")), SKILL);
        let names: Vec<_> = fs::read_dir(source(&home))
            .unwrap()
            .map(|item| item.unwrap().file_name())
            .collect();
        assert_eq!(names, ["SKILL.md"]);
    }

    #[test]
    fn top_up_adds_to_the_accounts_own_skills_and_leaves_its_cam() {
        let (tree, home, account) = setup();
        tree.dir("home/.local/share/cam/accounts/work/skills/review");
        top_up(&home, &account, true).unwrap();
        assert_eq!(state(&home, &account), State::Linked);
        assert!(!only_link(&home, &account));
        assert!(account.join("skills/review").is_dir());

        let (tree, home, account) = setup();
        let own = tree.dir("home/.local/share/cam/accounts/work/skills/cam");
        fs::write(own.join("SKILL.md"), "mine\n").unwrap();
        assert_eq!(state(&home, &account), State::Own);
        top_up(&home, &account, true).unwrap();
        assert_eq!(read(&own.join("SKILL.md")), "mine\n");
        assert!(!unlink(&home, &account).unwrap());
        assert_eq!(read(&own.join("SKILL.md")), "mine\n");
    }

    #[test]
    fn top_up_never_writes_into_a_shared_skills_folder() {
        let (_tree, home, account) = setup();
        share::top_up(&home, &account, &[share::Group::Skills]);
        assert_eq!(state(&home, &account), State::SharedFolder);
        top_up(&home, &account, true).unwrap();
        assert_eq!(
            fs::read_dir(home.join(".claude/skills")).unwrap().count(),
            0
        );
        assert!(!account::exists(&source(&home)));
        assert!(!in_claude(&home));
    }

    #[test]
    fn unwanted_removes_the_link_and_the_folder_cam_made() {
        let (_tree, home, account) = setup();
        top_up(&home, &account, true).unwrap();
        top_up(&home, &account, false).unwrap();
        assert_eq!(state(&home, &account), State::Missing);
        assert!(!account::exists(&account.join("skills")));
        // cam's copy stays, for the other accounts.
        assert_eq!(read(&source(&home).join("SKILL.md")), SKILL);
        // Nothing to remove is fine.
        top_up(&home, &account, false).unwrap();
        assert!(!unlink(&home, &account).unwrap());
    }

    #[test]
    fn unlink_keeps_a_folder_with_other_skills() {
        let (tree, home, account) = setup();
        top_up(&home, &account, true).unwrap();
        tree.dir("home/.local/share/cam/accounts/work/skills/review");
        assert!(unlink(&home, &account).unwrap());
        assert!(account.join("skills/review").is_dir());
        assert!(!account::exists(&account.join("skills/cam")));
    }

    #[test]
    fn finds_a_skill_in_claudes_own_setup() {
        let (tree, home, _account) = setup();
        assert!(!in_claude(&home));
        let dir = tree.dir("home/.claude/skills/cam");
        fs::write(dir.join("SKILL.md"), SKILL).unwrap();
        assert!(in_claude(&home));
    }
}
