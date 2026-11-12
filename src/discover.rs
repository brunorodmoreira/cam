//! Finding the real `claude` binary.
//!
//! 1. `CAM_CLAUDE_BIN`, if set: an absolute path to an executable that isn't
//!    cam itself.
//! 2. Otherwise the first executable `claude` on `PATH`, skipping relative
//!    entries, the shim dir, and any `claude` that is cam's own executable
//!    (through a symlink or a hard link).
//!
//! Resolved on every launch and never cached: `~/.local/bin/claude` is a
//! symlink that Claude Code's updater repoints.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::Error;

pub const ENV_CLAUDE_BIN: &str = "CAM_CLAUDE_BIN";
const BINARY: &str = "claude";

/// What discovery looks at. The caller takes it from the environment.
pub struct Search<'a> {
    /// `CAM_CLAUDE_BIN`. Empty counts as unset.
    pub claude_bin: Option<&'a OsStr>,
    /// `PATH`.
    pub path: Option<&'a OsStr>,
    /// cam's own executable.
    pub own_exe: &'a Path,
    /// The shim dir. A `claude` in it is always skipped.
    pub shim_dir: Option<&'a Path>,
}

pub fn real_claude(search: &Search) -> Result<PathBuf, Error> {
    if let Some(value) = search.claude_bin.filter(|value| !value.is_empty()) {
        return from_env(Path::new(value), search.own_exe);
    }
    env::split_paths(search.path.unwrap_or(OsStr::new("")))
        // An empty or relative entry means the current directory, and a
        // `claude` that a repo ships must never receive a token.
        .filter(|dir| dir.is_absolute())
        .filter(|dir| !search.shim_dir.is_some_and(|shim| same_file(dir, shim)))
        .map(|dir| dir.join(BINARY))
        .find(|candidate| is_executable(candidate) && !same_file(candidate, search.own_exe))
        .ok_or(Error::ClaudeNotFound)
}

fn from_env(path: &Path, own_exe: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() {
        return Err(Error::ClaudeBinNotAbsolute {
            value: path.to_owned(),
        });
    }
    if !is_executable(path) {
        return Err(Error::ClaudeBinMissing {
            path: path.to_owned(),
        });
    }
    if same_file(path, own_exe) {
        return Err(Error::ClaudeBinIsCam {
            path: path.to_owned(),
        });
    }
    Ok(path.to_owned())
}

/// A regular file, after following symlinks, with an execute bit set.
pub fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Whether `a` and `b` are the same file or directory, following symlinks.
/// Comparing device and inode also catches hard links.
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (fs::metadata(a), fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::testutil::Tree;

    /// A tree with `own/cam` standing in for cam's executable.
    struct Fixture {
        tree: Tree,
        own_exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tree = Tree::new();
            let own_exe = tree.dir("own").join("cam");
            write_exe(&own_exe);
            Self { tree, own_exe }
        }

        /// Writes an executable `claude` in `rel` and returns the dir.
        fn claude(&self, rel: &str) -> PathBuf {
            let dir = self.tree.dir(rel);
            write_exe(&dir.join(BINARY));
            dir
        }

        fn find(&self, path: &[&Path]) -> Result<PathBuf, Error> {
            self.find_with(None, path, None)
        }

        fn find_with(
            &self,
            claude_bin: Option<&OsStr>,
            path: &[&Path],
            shim_dir: Option<&Path>,
        ) -> Result<PathBuf, Error> {
            let path = env::join_paths(path).unwrap();
            real_claude(&Search {
                claude_bin,
                path: Some(&path),
                own_exe: &self.own_exe,
                shim_dir,
            })
        }
    }

    fn write_exe(path: &Path) {
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn first_executable_on_path_wins() {
        let fx = Fixture::new();
        let (a, b) = (fx.claude("a"), fx.claude("b"));
        assert_eq!(fx.find(&[&a, &b]).unwrap(), a.join(BINARY));
        assert_eq!(fx.find(&[&b, &a]).unwrap(), b.join(BINARY));
    }

    #[test]
    fn skips_missing_non_executable_and_directories() {
        let fx = Fixture::new();
        let empty = fx.tree.dir("empty");
        let no_exec = fx.tree.dir("no-exec");
        fs::write(no_exec.join(BINARY), "#!/bin/sh\n").unwrap();
        let is_dir = fx.tree.dir("is-dir/claude");
        let is_dir = is_dir.parent().unwrap();
        let real = fx.claude("real");
        assert_eq!(
            fx.find(&[&empty, &fx.tree.root.join("gone"), &no_exec, is_dir, &real])
                .unwrap(),
            real.join(BINARY)
        );
    }

    #[test]
    fn follows_symlinks_to_the_real_binary() {
        let fx = Fixture::new();
        let versions = fx.claude("versions");
        let bin = fx.tree.dir("bin");
        symlink(versions.join(BINARY), bin.join(BINARY)).unwrap();
        // The PATH spelling is kept, so the updater's symlink is honoured.
        assert_eq!(fx.find(&[&bin]).unwrap(), bin.join(BINARY));
    }

    #[test]
    fn skips_cam_itself_through_symlinks_and_hard_links() {
        let fx = Fixture::new();
        let soft = fx.tree.dir("soft");
        symlink(&fx.own_exe, soft.join(BINARY)).unwrap();
        let hard = fx.tree.dir("hard");
        fs::hard_link(&fx.own_exe, hard.join(BINARY)).unwrap();
        let real = fx.claude("real");
        assert_eq!(fx.find(&[&soft, &hard, &real]).unwrap(), real.join(BINARY));
    }

    #[test]
    fn skips_the_shim_dir_however_it_is_spelled() {
        let fx = Fixture::new();
        let shims = fx.claude("shims");
        let alias = fx.tree.root.join("alias");
        symlink(&shims, &alias).unwrap();
        let real = fx.claude("real");
        for spelling in [&shims, &alias] {
            assert_eq!(
                fx.find_with(None, &[spelling, &real], Some(&shims))
                    .unwrap(),
                real.join(BINARY)
            );
        }
    }

    #[test]
    fn never_uses_relative_entries() {
        let fx = Fixture::new();
        // A relative entry that really leads to a `claude`: up from the
        // test's working directory to `/`, then down into the tree.
        let decoy = fx.claude("decoy");
        let cwd = env::current_dir().unwrap();
        let up = "../".repeat(cwd.components().count() - 1);
        let relative = format!("{up}{}", decoy.strip_prefix("/").unwrap().display());
        assert!(Path::new(&relative).join(BINARY).is_file());

        let real = fx.claude("real");
        let path = OsString::from(format!(":.:{relative}:{}", real.display()));
        let found = real_claude(&Search {
            claude_bin: None,
            path: Some(&path),
            own_exe: &fx.own_exe,
            shim_dir: None,
        });
        assert_eq!(found.unwrap(), real.join(BINARY));
    }

    #[test]
    fn not_found() {
        let fx = Fixture::new();
        let empty = fx.tree.dir("empty");
        assert!(matches!(fx.find(&[&empty]), Err(Error::ClaudeNotFound)));
        let found = real_claude(&Search {
            claude_bin: None,
            path: None,
            own_exe: &fx.own_exe,
            shim_dir: None,
        });
        assert!(matches!(found, Err(Error::ClaudeNotFound)));
    }

    #[test]
    fn claude_bin_beats_path() {
        let fx = Fixture::new();
        let on_path = fx.claude("on-path");
        let chosen = fx.claude("chosen").join(BINARY);
        assert_eq!(
            fx.find_with(Some(chosen.as_os_str()), &[&on_path], None)
                .unwrap(),
            chosen
        );
        // Empty counts as unset.
        assert_eq!(
            fx.find_with(Some(OsStr::new("")), &[&on_path], None)
                .unwrap(),
            on_path.join(BINARY)
        );
    }

    #[test]
    fn claude_bin_must_be_absolute_executable_and_not_cam() {
        let fx = Fixture::new();
        let on_path = fx.claude("on-path");
        let find = |value: &OsStr| fx.find_with(Some(value), &[&on_path], None);

        assert!(matches!(
            find(OsStr::new("claude")),
            Err(Error::ClaudeBinNotAbsolute { .. })
        ));
        let missing = fx.tree.root.join("missing");
        assert!(matches!(
            find(missing.as_os_str()),
            Err(Error::ClaudeBinMissing { .. })
        ));
        let plain = fx.tree.root.join("plain");
        fs::write(&plain, "").unwrap();
        assert!(matches!(
            find(plain.as_os_str()),
            Err(Error::ClaudeBinMissing { .. })
        ));
        let link = fx.tree.root.join("link-to-cam");
        symlink(&fx.own_exe, &link).unwrap();
        assert!(matches!(
            find(link.as_os_str()),
            Err(Error::ClaudeBinIsCam { .. })
        ));
    }
}
