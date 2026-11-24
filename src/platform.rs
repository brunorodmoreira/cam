//! What differs between Unix and Windows. The rest of cam is the same code
//! on both. See DESIGN.md, Windows.
//!
//! Each side has the same items:
//! - [`home_dir`]: the home dir, as Claude Code finds it.
//! - [`private_dir`] and [`private_file`]: what cam creates for one user.
//! - [`executable_names`], [`is_executable`] and [`is_named`]: how a
//!   command name maps to files.
//! - [`same_file`]: whether two paths are one file, links included.
//! - [`symlink`], [`copy_link`] and [`remove_link`]: links.
//! - [`IgnoreInterrupts`] and [`describe_exit`]: Ctrl-C and how a child
//!   ended.
//! - [`run_instead`]: how `exec` hands over to `claude`.

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
