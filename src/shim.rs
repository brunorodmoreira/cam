//! The shim: a `claude` that is cam, busybox style. When cam runs under the
//! name `claude`, it behaves exactly like `cam exec -- <args…>`.
//!
//! `cam init` keeps it in `~/.local/share/cam/shims` and puts the dir first
//! on `PATH`. It only ever creates or replaces a `claude` there that is
//! already cam's, so it can't clobber a real `claude`.
//!
//! - Unix: a symlink to cam.
//! - Windows: `claude.exe`, a copy of `cam.exe`. Not a symlink, which needs
//!   Developer Mode, and not a hard link: that's the same file as `cam.exe`,
//!   which Windows would then keep locked while any session runs, so cam
//!   couldn't be upgraded. `cam init` replaces the copy when cam changes.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use crate::discover::{is_executable, same_file};
use crate::error::Error;
use crate::platform;

/// The name the shim answers to.
pub const NAME: &str = "claude";
const CAM: &str = "cam";

/// The shim's file in its dir.
#[cfg(unix)]
pub const FILE: &str = NAME;
#[cfg(windows)]
pub const FILE: &str = "claude.exe";

/// Bytes in every cam binary, so `cam init` can tell an older copy of cam
/// from another program named `claude.exe`.
#[cfg(windows)]
const MARK: &[u8] = b"cam shim: this claude.exe is a copy of cam.exe";

/// `~/.local/share/cam/shims`.
pub fn default_dir(home: Option<PathBuf>) -> Option<PathBuf> {
    home.map(|home| home.join(".local").join("share").join("cam").join("shims"))
}

/// Whether cam was started as `claude`, i.e. through the shim.
pub fn invoked_as_shim(argv0: &OsStr) -> bool {
    platform::started_as(argv0, NAME)
}

/// What the shim should point at: the `cam` on `PATH` if it is this very
/// binary, else `own_exe`. The `PATH` spelling, such as
/// `/opt/homebrew/bin/cam`, survives package-manager upgrades, where the
/// canonical path into a versioned directory would not. A copy, on
/// Windows, doesn't point anywhere, so it's always `own_exe` there.
pub fn target(own_exe: &Path, path: Option<&OsStr>) -> PathBuf {
    env::split_paths(path.unwrap_or(OsStr::new("")))
        .filter(|dir| dir.is_absolute() && cfg!(unix))
        .map(|dir| dir.join(CAM))
        .find(|candidate| same_file(candidate, own_exe))
        .unwrap_or_else(|| own_exe.to_owned())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    Created,
    /// An older shim, which pointed at `previous` (Unix) or was an older
    /// copy of cam (Windows).
    Replaced {
        previous: PathBuf,
    },
    /// The shim was already up to date.
    Unchanged,
}

/// Creates or updates `<dir>/claude` (`claude.exe` on Windows). Refuses to
/// touch one there that isn't cam's.
pub fn install(dir: &Path, target: &Path, own_exe: &Path) -> Result<Installed, Error> {
    let path = dir.join(FILE);
    let outcome = match existing(&path, own_exe)? {
        Existing::Missing => Installed::Created,
        Existing::Current => return Ok(Installed::Unchanged),
        Existing::Shim { target: current } if cfg!(unix) && current == target => {
            return Ok(Installed::Unchanged);
        }
        Existing::Shim { target: current } => Installed::Replaced { previous: current },
        Existing::Other => return Err(Error::ShimOccupied { path }),
    };
    let write_error = |source| Error::Write {
        path: path.clone(),
        source,
    };
    fs::create_dir_all(dir).map_err(write_error)?;
    put(&path, target, own_exe).map_err(write_error)?;
    Ok(outcome)
}

/// Links under a temporary name, then renames over: the swap is atomic.
#[cfg(unix)]
fn put(path: &Path, target: &Path, _own_exe: &Path) -> io::Result<()> {
    let tmp = path.with_file_name(format!(".{NAME}.cam-{}", process::id()));
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)
        .and_then(|()| fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
}

/// Copies under a temporary name, then renames into place. A session
/// started through the current copy keeps it open, and Windows can rename a
/// running program but not replace it, so the current copy is moved aside
/// first, and deleted once nothing runs it: now, or at a later `cam init`.
#[cfg(windows)]
fn put(path: &Path, _target: &Path, own_exe: &Path) -> io::Result<()> {
    let tmp = path.with_file_name(format!(".claude.cam-{}.exe", process::id()));
    let aside = path.with_file_name(format!(".claude.cam-old-{}.exe", process::id()));
    let _ = fs::remove_file(&tmp);
    if let Err(err) = fs::copy(own_exe, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    let moved = match fs::rename(path, &aside) {
        Ok(()) => true,
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            return Err(err);
        }
    };
    if let Err(err) = fs::rename(&tmp, path) {
        if moved {
            let _ = fs::rename(&aside, path);
        }
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Some(dir) = path.parent() {
        remove_old_copies(dir);
    }
    Ok(())
}

/// Deletes the copies `put` moved aside, except those a session still runs.
#[cfg(windows)]
fn remove_old_copies(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".claude.cam-old-"));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

enum Existing {
    Missing,
    /// cam's and up to date: on Windows, a copy of this very cam. Unix
    /// compares a link's target instead.
    #[cfg_attr(unix, allow(dead_code))]
    Current,
    /// cam's: a symlink to a cam binary (this one, or any file named `cam`,
    /// even one that's gone, such as an old build), or on Windows a copy of
    /// another cam.
    Shim {
        target: PathBuf,
    },
    Other,
}

#[cfg(unix)]
fn existing(path: &Path, own_exe: &Path) -> Result<Existing, Error> {
    let read_error = |source| Error::Read {
        path: path.to_owned(),
        source,
    };
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Existing::Missing),
        Err(err) => return Err(read_error(err)),
    };
    if !meta.file_type().is_symlink() {
        return Ok(Existing::Other);
    }
    let target = fs::read_link(path).map_err(read_error)?;
    let resolved = path
        .parent()
        .map_or(target.clone(), |dir| dir.join(&target));
    if target.file_name() == Some(OsStr::new(CAM)) || same_file(&resolved, own_exe) {
        Ok(Existing::Shim { target })
    } else {
        Ok(Existing::Other)
    }
}

#[cfg(windows)]
fn existing(path: &Path, own_exe: &Path) -> Result<Existing, Error> {
    let read_error = |source| Error::Read {
        path: path.to_owned(),
        source,
    };
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Existing::Missing),
        Err(err) => return Err(read_error(err)),
    };
    if !meta.is_file() {
        return Ok(Existing::Other);
    }
    // A copy keeps the size and the time it was written, so an up-to-date
    // one is known without reading it.
    let copied = fs::metadata(own_exe).is_ok_and(|own| {
        own.len() == meta.len()
            && own
                .modified()
                .ok()
                .is_some_and(|at| meta.modified().ok() == Some(at))
    });
    if copied || same_file(path, own_exe) {
        return Ok(Existing::Current);
    }
    let bytes = fs::read(path).map_err(read_error)?;
    if bytes.windows(MARK.len()).any(|window| window == MARK) {
        Ok(Existing::Shim {
            target: path.to_owned(),
        })
    } else {
        Ok(Existing::Other)
    }
}

/// Whether `path` is a cam shim, by the rule `install` uses.
pub fn is_shim(path: &Path, own_exe: &Path) -> bool {
    matches!(
        existing(path, own_exe),
        Ok(Existing::Shim { .. } | Existing::Current)
    )
}

/// What a shell runs for plain `claude`, going by `PATH`.
#[derive(Debug, PartialEq, Eq)]
pub enum Reaches {
    /// A cam shim, or cam itself under the name `claude`.
    Shim(PathBuf),
    /// Some other `claude`, normally the real one.
    Other(PathBuf),
    Nothing,
}

/// The first executable `claude` on `PATH`, and whether it is cam.
pub fn first_on_path(path: Option<&OsStr>, own_exe: &Path) -> Reaches {
    let first = env::split_paths(path.unwrap_or(OsStr::new("")))
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| {
            platform::executable_names(NAME)
                .into_iter()
                .map(move |name| dir.join(name))
        })
        .find(|candidate| is_executable(candidate));
    match first {
        None => Reaches::Nothing,
        Some(claude) if is_shim(&claude, own_exe) || same_file(&claude, own_exe) => {
            Reaches::Shim(claude)
        }
        Some(claude) => Reaches::Other(claude),
    }
}

// The symlink shim. Windows has its own tests, below.
#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;
    use crate::testutil::Tree;

    struct Fixture {
        tree: Tree,
        own_exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tree = Tree::new();
            let own_exe = tree.dir("cargo/bin").join(CAM);
            write_exe(&own_exe);
            Self { tree, own_exe }
        }

        fn shims(&self) -> PathBuf {
            self.tree.root.join("shims")
        }

        fn link(&self) -> PathBuf {
            self.shims().join(NAME)
        }
    }

    fn write_exe(path: &Path) {
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn detects_the_shim_name() {
        assert!(invoked_as_shim(OsStr::new("claude")));
        assert!(invoked_as_shim(OsStr::new(
            "/home/me/.local/share/cam/shims/claude"
        )));
        assert!(!invoked_as_shim(OsStr::new("cam")));
        assert!(!invoked_as_shim(OsStr::new("/usr/bin/claude-code")));
        assert!(!invoked_as_shim(OsStr::new("")));
    }

    #[test]
    fn default_dir_is_under_home() {
        assert_eq!(
            default_dir(Some("/home/me".into())),
            Some(PathBuf::from("/home/me/.local/share/cam/shims"))
        );
        assert_eq!(default_dir(None), None);
    }

    #[test]
    fn target_prefers_the_path_spelling_of_this_binary() {
        let fx = Fixture::new();
        let brew = fx.tree.dir("brew/bin");
        symlink(&fx.own_exe, brew.join(CAM)).unwrap();
        let other = fx.tree.dir("other");
        write_exe(&other.join(CAM));
        let path = env::join_paths([&other, &brew]).unwrap();
        assert_eq!(target(&fx.own_exe, Some(&path)), brew.join(CAM));
        // Not on PATH: the executable itself.
        let path = env::join_paths([&other]).unwrap();
        assert_eq!(target(&fx.own_exe, Some(&path)), fx.own_exe);
        assert_eq!(target(&fx.own_exe, None), fx.own_exe);
    }

    #[test]
    fn install_creates_then_is_idempotent() {
        let fx = Fixture::new();
        assert_eq!(
            install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap(),
            Installed::Created
        );
        assert_eq!(fs::read_link(fx.link()).unwrap(), fx.own_exe);
        assert_eq!(
            install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap(),
            Installed::Unchanged
        );
        // No temp file left behind.
        assert_eq!(fs::read_dir(fx.shims()).unwrap().count(), 1);
    }

    #[test]
    fn install_replaces_an_older_shim_even_a_dangling_one() {
        let fx = Fixture::new();
        fs::create_dir_all(fx.shims()).unwrap();
        let old = fx.tree.root.join("old-build/cam");
        symlink(&old, fx.link()).unwrap();
        assert_eq!(
            install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap(),
            Installed::Replaced { previous: old }
        );
        assert_eq!(fs::read_link(fx.link()).unwrap(), fx.own_exe);
    }

    #[test]
    fn first_on_path_tells_a_shim_from_the_real_claude() {
        let fx = Fixture::new();
        install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap();
        let local = fx.tree.dir("local/bin");
        write_exe(&local.join(NAME));
        // A shim to an older cam elsewhere, which still exists.
        let old = fx.tree.dir("old").join(CAM);
        write_exe(&old);
        let other_shims = fx.tree.dir("other-shims");
        symlink(&old, other_shims.join(NAME)).unwrap();
        // A dangling shim, which a shell would skip.
        let dangling = fx.tree.dir("dangling");
        symlink(fx.tree.root.join("gone/cam"), dangling.join(NAME)).unwrap();
        let empty = fx.tree.dir("empty");
        let first = |dirs: &[&Path]| {
            let path = env::join_paths(dirs).unwrap();
            first_on_path(Some(&path), &fx.own_exe)
        };
        let shims = fx.shims();
        assert_eq!(first(&[&empty, &shims, &local]), Reaches::Shim(fx.link()));
        assert_eq!(
            first(&[&dangling, &local, &shims]),
            Reaches::Other(local.join(NAME))
        );
        assert_eq!(
            first(&[&other_shims, &local]),
            Reaches::Shim(other_shims.join(NAME))
        );
        assert_eq!(first(&[&empty]), Reaches::Nothing);
        assert!(is_shim(&fx.link(), &fx.own_exe));
        assert!(!is_shim(&local.join(NAME), &fx.own_exe));
        assert!(!is_shim(&empty.join(NAME), &fx.own_exe));
    }

    #[test]
    fn install_leaves_other_files_alone() {
        let fx = Fixture::new();
        let real = fx.tree.dir("versions").join("2.1.322");
        write_exe(&real);
        let bin = fx.tree.dir("bin");
        symlink(&real, bin.join(NAME)).unwrap();
        let plain = fx.tree.dir("plain");
        write_exe(&plain.join(NAME));
        for dir in [&bin, &plain] {
            assert!(matches!(
                install(dir, &fx.own_exe, &fx.own_exe),
                Err(Error::ShimOccupied { .. })
            ));
        }
        assert_eq!(fs::read_link(bin.join(NAME)).unwrap(), real);
        assert!(plain.join(NAME).is_file());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::testutil::Tree;

    /// A tree with `own/cam.exe` standing in for cam: any file with the
    /// mark counts as a cam binary.
    struct Fixture {
        tree: Tree,
        own_exe: PathBuf,
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tree = Tree::new();
            let own_exe = tree.dir("own").join("cam.exe");
            fs::write(&own_exe, [b"new ".as_slice(), MARK].concat()).unwrap();
            let dir = tree.root.join("shims");
            Self { tree, own_exe, dir }
        }

        fn install(&self) -> Result<Installed, Error> {
            install(&self.dir, &self.own_exe, &self.own_exe)
        }

        fn shim(&self) -> PathBuf {
            self.dir.join(FILE)
        }
    }

    #[test]
    fn makes_a_copy_and_leaves_a_current_one() {
        let fx = Fixture::new();
        assert_eq!(fx.install().unwrap(), Installed::Created);
        assert_eq!(fs::read(fx.shim()).unwrap(), fs::read(&fx.own_exe).unwrap());
        assert!(!same_file(&fx.shim(), &fx.own_exe), "a copy, not a link");
        assert_eq!(fx.install().unwrap(), Installed::Unchanged);
        assert!(is_shim(&fx.shim(), &fx.own_exe));
    }

    #[test]
    fn replaces_an_older_cam_and_leaves_nothing_behind() {
        let fx = Fixture::new();
        fs::create_dir_all(&fx.dir).unwrap();
        fs::write(fx.shim(), [b"older cam ".as_slice(), MARK].concat()).unwrap();
        assert!(is_shim(&fx.shim(), &fx.own_exe));
        assert_eq!(
            fx.install().unwrap(),
            Installed::Replaced {
                previous: fx.shim()
            }
        );
        assert_eq!(fs::read(fx.shim()).unwrap(), fs::read(&fx.own_exe).unwrap());
        let left: Vec<_> = fs::read_dir(&fx.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [FILE]);
    }

    #[test]
    fn never_replaces_another_claude() {
        let fx = Fixture::new();
        fs::create_dir_all(&fx.dir).unwrap();
        fs::write(fx.shim(), "the real claude\n").unwrap();
        assert!(!is_shim(&fx.shim(), &fx.own_exe));
        assert!(matches!(fx.install(), Err(Error::ShimOccupied { .. })));
        assert_eq!(fs::read_to_string(fx.shim()).unwrap(), "the real claude\n");
    }

    #[test]
    fn first_on_path_knows_the_copy() {
        let fx = Fixture::new();
        fx.install().unwrap();
        let real = fx.tree.dir("bin").join("claude.exe");
        fs::write(&real, "the real claude\n").unwrap();
        let bin = real.parent().unwrap();
        let path = env::join_paths([fx.dir.as_path(), bin]).unwrap();
        assert_eq!(
            first_on_path(Some(&path), &fx.own_exe),
            Reaches::Shim(fx.shim())
        );
        let path = env::join_paths([bin, fx.dir.as_path()]).unwrap();
        assert_eq!(
            first_on_path(Some(&path), &fx.own_exe),
            Reaches::Other(real)
        );
    }
}
