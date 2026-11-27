//! What differs between Unix and Windows. The rest of cam is the same code
//! on both. See DESIGN.md, Windows.
//!
//! Each side has the same items:
//! - [`home_dir`]: the home dir, as Claude Code finds it.
//! - [`canonicalize`]: a path with links resolved, as people read it.
//! - [`private_dir`] and [`private_file`]: what cam creates for one user.
//! - [`executable_names`], [`is_executable`], [`is_named`] and
//!   [`started_as`]: how a command name maps to files.
//! - [`same_file`]: whether two paths are one file, links included.
//! - [`link`], [`file_links_allowed`], [`copy_link`] and [`remove_link`]:
//!   links. Windows links folders with junctions, and files with symlinks
//!   only where Developer Mode allows them.
//! - [`IgnoreInterrupts`] and [`describe_exit`]: Ctrl-C and how a child
//!   ended.
//! - [`run_instead`]: how `exec` hands over to `claude`.
//! - [`LocalSocket`], [`connect_local`] and [`detach`]: for Herdr, a Unix
//!   socket or a named pipe, and a process apart from the terminal.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

use std::path::Path;

/// Whether `a` and `b` are the same file or folder, following links. Hard
/// links count too.
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || same_file_id(a, b)
}
