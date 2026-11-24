//! The `claude` → `cam` link (the shim), busybox style. When cam runs under
//! the name `claude`, it behaves exactly like `cam exec -- <args…>`.
//!
//! `cam init` keeps that link in `~/.local/share/cam/shims` and puts the dir
//! first on `PATH`. It only ever creates or replaces a `claude` that is
//! already a cam link, so it can't clobber a real `claude`.

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

/// `~/.local/share/cam/shims`.
pub fn default_dir(home: Option<PathBuf>) -> Option<PathBuf> {
    home.map(|home| home.join(".local").join("share").join("cam").join("shims"))
}

/// Whether cam was started as `claude`, i.e. through the shim.
pub fn invoked_as_shim(argv0: &OsStr) -> bool {
    platform::is_named(Path::new(argv0), NAME)
}

/// What the shim should point at: the `cam` on `PATH` if it is this very
/// binary, else `own_exe`. The `PATH` spelling, such as
/// `/opt/homebrew/bin/cam`, survives package-manager upgrades, where the
/// canonical path into a versioned directory would not.
pub fn target(own_exe: &Path, path: Option<&OsStr>) -> PathBuf {
    env::split_paths(path.unwrap_or(OsStr::new("")))
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(CAM))
        .find(|candidate| same_file(candidate, own_exe))
        .unwrap_or_else(|| own_exe.to_owned())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    Created,
    /// An older shim pointed at `previous`.
    Replaced {
        previous: PathBuf,
    },
    /// The shim already pointed at the target.
    Unchanged,
}

/// Creates or updates `<dir>/claude` → `target`. Refuses to touch a
/// `claude` there that isn't a cam shim.
pub fn install(dir: &Path, target: &Path, own_exe: &Path) -> Result<Installed, Error> {
    let path = dir.join(NAME);
    let outcome = match existing(&path, own_exe)? {
        Existing::Missing => Installed::Created,
        Existing::Shim { target: current } if current == target => {
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
    // Link under a temporary name, then rename over: the swap is atomic.
    let tmp = dir.join(format!(".{NAME}.cam-{}", process::id()));
    let _ = fs::remove_file(&tmp);
    platform::symlink(target, &tmp)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|err| {
            let _ = fs::remove_file(&tmp);
            write_error(err)
        })?;
    Ok(outcome)
}

enum Existing {
    Missing,
    /// A symlink to a cam binary: this one, or any file named `cam`, even one
    /// that's gone, such as an old build.
    Shim {
        target: PathBuf,
    },
    Other,
}

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

/// Whether `path` is a cam shim, by the rule `install` uses.
pub fn is_shim(path: &Path, own_exe: &Path) -> bool {
    matches!(existing(path, own_exe), Ok(Existing::Shim { .. }))
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
        .map(|dir| dir.join(NAME))
        .find(|candidate| is_executable(candidate));
    match first {
        None => Reaches::Nothing,
        Some(claude) if is_shim(&claude, own_exe) || same_file(&claude, own_exe) => {
            Reaches::Shim(claude)
        }
        Some(claude) => Reaches::Other(claude),
    }
}

// Unix for now: on Windows the shim is another kind of file, with tests of
// its own (see DESIGN.md, Windows).
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
