//! The `claude` → `cam` shim, busybox style. When cam runs under the name
//! `claude`, it behaves exactly like `cam exec -- <args…>`.
//!
//! `cam shim install` puts that symlink in a directory meant to come first on
//! `PATH`. It only ever creates, replaces or removes a `claude` that is
//! already a cam shim, so a mistaken `--dir` can't clobber the real `claude`.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process;

use crate::discover::{is_executable, same_file};
use crate::error::Error;

/// The name the shim answers to.
pub const NAME: &str = "claude";
const CAM: &str = "cam";

/// `~/.local/share/cam/shims`.
pub fn default_dir(home: Option<PathBuf>) -> Option<PathBuf> {
    home.map(|home| home.join(".local/share/cam/shims"))
}

/// Whether cam was started as `claude`, i.e. through the shim.
pub fn invoked_as_shim(argv0: &OsStr) -> bool {
    Path::new(argv0).file_name() == Some(OsStr::new(NAME))
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
    symlink(target, &tmp)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|err| {
            let _ = fs::remove_file(&tmp);
            write_error(err)
        })?;
    Ok(outcome)
}

/// Removes `<dir>/claude` if it is a cam shim, returning its path.
pub fn uninstall(dir: &Path, own_exe: &Path) -> Result<PathBuf, Error> {
    let path = dir.join(NAME);
    match existing(&path, own_exe)? {
        Existing::Missing => Err(Error::NoShim { path }),
        Existing::Other => Err(Error::NotAShim { path }),
        Existing::Shim { .. } => match fs::remove_file(&path) {
            Ok(()) => Ok(path),
            Err(source) => Err(Error::Write { path, source }),
        },
    }
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

/// Whether `path` is a cam shim, by the rule `install` and `uninstall` use.
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

/// Where the shim dir sits on `PATH`, relative to the other `claude`s.
#[derive(Debug, PartialEq, Eq)]
pub enum PathOrder {
    /// Plain `claude` reaches the shim.
    ShimFirst,
    /// The shim dir isn't on `PATH`. `claude` is the first other `claude`,
    /// if any.
    Missing { claude: Option<PathBuf> },
    /// The shim dir is on `PATH`, but `claude` comes first.
    After { claude: PathBuf },
}

/// What a shell finds first when it runs `claude`: the shim dir, or another
/// executable `claude` that isn't cam.
pub fn path_order(path: Option<&OsStr>, shim_dir: &Path, own_exe: &Path) -> PathOrder {
    let mut other = None;
    for dir in env::split_paths(path.unwrap_or(OsStr::new(""))).filter(|dir| dir.is_absolute()) {
        if same_file(&dir, shim_dir) {
            return match other {
                None => PathOrder::ShimFirst,
                Some(claude) => PathOrder::After { claude },
            };
        }
        let candidate = dir.join(NAME);
        if other.is_none() && is_executable(&candidate) && !same_file(&candidate, own_exe) {
            other = Some(candidate);
        }
    }
    PathOrder::Missing { claude: other }
}

/// What to tell the user after installing, for their shell (`$SHELL`).
pub fn advice(
    order: &PathOrder,
    shell: Option<&OsStr>,
    dir: &Path,
    home: Option<&Path>,
) -> Vec<String> {
    let shell = shell
        .and_then(|shell| Path::new(shell).file_name())
        .and_then(OsStr::to_str)
        .unwrap_or("");
    let (file, line) = startup_line(shell, &shell_path(dir, home));
    let below = |claude: &Path| {
        claude
            .parent()
            .map(|parent| format!(", below any line that adds {}", parent.display()))
            .unwrap_or_default()
    };
    match order {
        PathOrder::ShimFirst => vec![
            "Plain `claude` now runs through cam: the shim dir comes first on PATH.".to_owned(),
            "If this shell still runs the old `claude`, run `hash -r` or open a new terminal."
                .to_owned(),
        ],
        PathOrder::Missing { claude } => vec![
            format!(
                "Next, put the shim dir first on PATH. Add this line to the end of {file}{}:",
                claude.as_deref().map(below).unwrap_or_default()
            ),
            String::new(),
            format!("    {line}"),
            String::new(),
            "Then open a new terminal. `cam which` shows the account `claude` will use.".to_owned(),
        ],
        PathOrder::After { claude } => vec![
            format!(
                "The shim dir is on PATH, but after {}, so plain `claude` still skips cam.",
                claude.display()
            ),
            format!(
                "In {file}, move the line that adds the shim dir{}, then open a new terminal:",
                below(claude)
            ),
            String::new(),
            format!("    {line}"),
        ],
    }
}

/// The startup file and the line that puts `dir` first on `PATH`.
fn startup_line(shell: &str, dir: &str) -> (&'static str, String) {
    match shell {
        "fish" => (
            "~/.config/fish/config.fish",
            format!("set -gx PATH \"{dir}\" $PATH"),
        ),
        "zsh" => ("~/.zshrc", posix_line(dir)),
        "bash" => ("~/.bashrc", posix_line(dir)),
        _ => ("your shell's startup file", posix_line(dir)),
    }
}

fn posix_line(dir: &str) -> String {
    format!("export PATH=\"{dir}:$PATH\"")
}

/// `dir` for a double-quoted shell string: `$HOME/…` when under the home
/// directory, so the line stays portable, with `"`, `\`, `$` and `` ` ``
/// in the path escaped.
fn shell_path(dir: &Path, home: Option<&Path>) -> String {
    let escape = |text: &str| {
        text.chars().fold(String::new(), |mut out, c| {
            if matches!(c, '"' | '\\' | '$' | '`') {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    match home.and_then(|home| dir.strip_prefix(home).ok()) {
        Some(rest) if !rest.as_os_str().is_empty() => {
            format!("$HOME/{}", escape(&rest.to_string_lossy()))
        }
        _ => escape(&dir.to_string_lossy()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

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
    fn install_and_uninstall_leave_other_files_alone() {
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
            assert!(matches!(
                uninstall(dir, &fx.own_exe),
                Err(Error::NotAShim { .. })
            ));
        }
        assert_eq!(fs::read_link(bin.join(NAME)).unwrap(), real);
        assert!(plain.join(NAME).is_file());
    }

    #[test]
    fn uninstall_removes_a_shim() {
        let fx = Fixture::new();
        install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap();
        assert_eq!(uninstall(&fx.shims(), &fx.own_exe).unwrap(), fx.link());
        assert!(fs::symlink_metadata(fx.link()).is_err());
        assert!(matches!(
            uninstall(&fx.shims(), &fx.own_exe),
            Err(Error::NoShim { .. })
        ));
    }

    #[test]
    fn path_order_cases() {
        let fx = Fixture::new();
        install(&fx.shims(), &fx.own_exe, &fx.own_exe).unwrap();
        let local = fx.tree.dir("local/bin");
        write_exe(&local.join(NAME));
        let empty = fx.tree.dir("empty");
        let order = |dirs: &[&Path]| {
            let path = env::join_paths(dirs).unwrap();
            path_order(Some(&path), &fx.shims(), &fx.own_exe)
        };
        let shims = fx.shims();
        assert_eq!(order(&[&empty, &shims, &local]), PathOrder::ShimFirst);
        assert_eq!(
            order(&[&local, &shims]),
            PathOrder::After {
                claude: local.join(NAME)
            }
        );
        assert_eq!(
            order(&[&empty, &local]),
            PathOrder::Missing {
                claude: Some(local.join(NAME))
            }
        );
        assert_eq!(order(&[&empty]), PathOrder::Missing { claude: None });
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
    fn path_order_ignores_other_shims_to_this_cam() {
        let fx = Fixture::new();
        let elsewhere = fx.tree.dir("elsewhere");
        symlink(&fx.own_exe, elsewhere.join(NAME)).unwrap();
        fs::create_dir_all(fx.shims()).unwrap();
        let path = env::join_paths([&elsewhere, &fx.shims()]).unwrap();
        assert_eq!(
            path_order(Some(&path), &fx.shims(), &fx.own_exe),
            PathOrder::ShimFirst
        );
    }

    #[test]
    fn advice_for_each_shell() {
        let home = Path::new("/home/me");
        let dir = home.join(".local/share/cam/shims");
        let missing = PathOrder::Missing {
            claude: Some(home.join(".local/bin/claude")),
        };
        let zsh = advice(&missing, Some(OsStr::new("/bin/zsh")), &dir, Some(home));
        assert_eq!(
            zsh[0],
            "Next, put the shim dir first on PATH. Add this line to the end of ~/.zshrc, below any line that adds /home/me/.local/bin:"
        );
        assert_eq!(
            zsh[2],
            "    export PATH=\"$HOME/.local/share/cam/shims:$PATH\""
        );

        let fish = advice(
            &missing,
            Some(OsStr::new("/opt/homebrew/bin/fish")),
            &dir,
            Some(home),
        );
        assert!(
            fish[0].contains("~/.config/fish/config.fish"),
            "{}",
            fish[0]
        );
        assert_eq!(
            fish[2],
            "    set -gx PATH \"$HOME/.local/share/cam/shims\" $PATH"
        );

        let other = advice(&PathOrder::Missing { claude: None }, None, &dir, Some(home));
        assert_eq!(
            other[0],
            "Next, put the shim dir first on PATH. Add this line to the end of your shell's startup file:"
        );

        let after = advice(
            &PathOrder::After {
                claude: home.join(".local/bin/claude"),
            },
            Some(OsStr::new("bash")),
            &dir,
            Some(home),
        );
        assert!(
            after[0].contains("after /home/me/.local/bin/claude"),
            "{}",
            after[0]
        );
        assert!(after[1].starts_with("In ~/.bashrc"), "{}", after[1]);
    }

    #[test]
    fn shell_path_uses_home_and_escapes() {
        let home = Path::new("/home/me");
        assert_eq!(
            shell_path(Path::new("/home/me/shims"), Some(home)),
            "$HOME/shims"
        );
        assert_eq!(
            shell_path(Path::new("/opt/shims"), Some(home)),
            "/opt/shims"
        );
        assert_eq!(
            shell_path(Path::new("/opt/a \"b\" $c"), None),
            "/opt/a \\\"b\\\" \\$c"
        );
        // A home of `/home/me` doesn't match `/home/meg`.
        assert_eq!(
            shell_path(Path::new("/home/meg/shims"), Some(home)),
            "/home/meg/shims"
        );
    }
}
