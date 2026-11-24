//! Unix: macOS and Linux.

use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

/// The home dir: `HOME`, else the user database.
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// The absolute path with links resolved, as `realpath(3)` gives it.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

/// A builder for folders only their owner can use: mode 0700.
pub fn private_dir() -> DirBuilder {
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    builder
}

/// Options for files only their owner can read: mode 0600.
pub fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(0o600);
    options
}

/// The file names a command is found under on `PATH`: just its own.
pub fn executable_names(command: &str) -> Vec<OsString> {
    vec![command.into()]
}

/// A regular file, after following symlinks, with an execute bit set.
pub fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Whether `path`'s file name is `command`.
pub fn is_named(path: &Path, command: &str) -> bool {
    path.file_name() == Some(command.as_ref())
}

/// Whether cam was started as `command`, going by `argv[0]`: a symlink
/// named `claude` starts cam's own executable.
pub fn started_as(argv0: &OsStr, command: &str) -> bool {
    is_named(Path::new(argv0), command)
}

/// Comparing device and inode also catches hard links.
pub(super) fn same_file_id(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// A symlink at `link` to `target`, a file or a folder.
pub fn link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Whether links to files can be made: always.
pub fn file_links_allowed() -> bool {
    true
}

/// A link at `to` with the same target as the link at `from`.
pub fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

/// Removes the link at `path`, never what it points to.
pub fn remove_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// "exit status 1", or the signal that ended it.
pub fn describe_exit(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit status {code}"),
        (None, Some(libc::SIGINT)) => "interrupted".to_owned(),
        (None, Some(signal)) => format!("ended by signal {signal}"),
        (None, None) => "no exit status".to_owned(),
    }
}

/// Ignores SIGINT and SIGQUIT until dropped, then restores what was there,
/// as `system(3)` does while its child runs. The terminal still sends them
/// to the child.
pub struct IgnoreInterrupts {
    previous: [(libc::c_int, libc::sighandler_t); 2],
}

impl IgnoreInterrupts {
    pub fn new() -> Self {
        let ignore = |signal| {
            // SAFETY: `signal` with SIG_IGN installs no handler code, so
            // nothing runs in signal context.
            (signal, unsafe { libc::signal(signal, libc::SIG_IGN) })
        };
        Self {
            previous: [ignore(libc::SIGINT), ignore(libc::SIGQUIT)],
        }
    }
}

impl Drop for IgnoreInterrupts {
    fn drop(&mut self) {
        for (signal, handler) in self.previous {
            if handler != libc::SIG_ERR {
                // SAFETY: restores the disposition `new` replaced.
                unsafe { libc::signal(signal, handler) };
            }
        }
    }
}

/// Replaces cam with `command`, so the terminal, signals and the exit code
/// are `claude`'s own. Returns only if that fails.
pub fn run_instead(mut command: Command) -> io::Error {
    command.exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_how_a_command_ended() {
        assert_eq!(describe_exit(ExitStatus::from_raw(1 << 8)), "exit status 1");
        assert_eq!(
            describe_exit(ExitStatus::from_raw(libc::SIGINT)),
            "interrupted"
        );
        assert_eq!(
            describe_exit(ExitStatus::from_raw(libc::SIGKILL)),
            "ended by signal 9"
        );
    }

    #[test]
    fn a_command_is_found_under_its_own_name() {
        assert_eq!(executable_names("claude"), [OsString::from("claude")]);
        assert!(is_named(Path::new("/usr/local/bin/claude"), "claude"));
        assert!(!is_named(Path::new("/usr/local/bin/claude.exe"), "claude"));
    }
}
