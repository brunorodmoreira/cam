//! Windows.

use std::env;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::mem;
use std::os::windows::fs::FileTypeExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{self, Child, Command, ExitStatus};
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::core::BOOL;

/// What `cmd.exe` uses when `PATHEXT` is unset.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// How a process ends when Ctrl-C reaches it unhandled.
const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// A builder for folders only their owner can use. The user profile's ACL
/// already keeps other users out, and what cam creates in it inherits it.
pub fn private_dir() -> DirBuilder {
    DirBuilder::new()
}

/// Options for files only their owner can read, as for [`private_dir`].
pub fn private_file() -> OpenOptions {
    OpenOptions::new()
}

/// The extensions that make a file runnable by name, from `PATHEXT`, in
/// its order and lowercase: `.com`, `.exe`, `.bat`, `.cmd`.
fn extensions() -> Vec<String> {
    let pathext = env::var("PATHEXT").unwrap_or_default();
    let pathext = match pathext.trim() {
        "" => DEFAULT_PATHEXT,
        value => value,
    };
    pathext
        .split(';')
        .map(|ext| ext.trim().to_ascii_lowercase())
        .filter(|ext| ext.len() > 1 && ext.starts_with('.'))
        .collect()
}

/// The file names a command is found under on `PATH`, as `cmd.exe` looks
/// for them: `claude.com`, `claude.exe`, `claude.bat`, `claude.cmd`. A
/// file with no extension can't be started, such as the shell script npm
/// puts beside `claude.cmd` for Git Bash.
pub fn executable_names(command: &str) -> Vec<OsString> {
    extensions()
        .into_iter()
        .map(|ext| format!("{command}{ext}").into())
        .collect()
}

fn has_runnable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| extensions().contains(&format!(".{}", ext.to_ascii_lowercase())))
}

/// A file, after following links, with an extension from `PATHEXT`.
pub fn is_executable(path: &Path) -> bool {
    has_runnable_extension(path) && fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// Whether `path` is `command` as Windows names it: `claude`, `claude.exe`
/// or `CLAUDE.EXE`, say.
pub fn is_named(path: &Path, command: &str) -> bool {
    let stem_matches = path
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|stem| stem.eq_ignore_ascii_case(command));
    stem_matches && (path.extension().is_none() || has_runnable_extension(path))
}

/// Compares the volume and file index, which also catches hard links.
pub(super) fn same_file_id(a: &Path, b: &Path) -> bool {
    same_file::is_same_file(a, b).unwrap_or(false)
}

/// A symlink at `link` to `target`. Windows makes them only with Developer
/// Mode on, or for an administrator.
pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// A link at `to` with the same target as the link at `from`, and of the
/// same kind, even when the target is gone.
pub fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    let target = fs::read_link(from)?;
    if fs::symlink_metadata(from)?.file_type().is_symlink_dir() {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

/// Removes the link at `path`, never what it points to. A link to a folder
/// is removed as a folder.
pub fn remove_link(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// "exit status 1", or "interrupted" for Ctrl-C.
pub fn describe_exit(status: ExitStatus) -> String {
    match status.code() {
        Some(code) if code as u32 == STATUS_CONTROL_C_EXIT => "interrupted".to_owned(),
        Some(code) => format!("exit status {code}"),
        None => "no exit status".to_owned(),
    }
}

/// Tells Windows that Ctrl-C and Ctrl-Break are handled, so they don't end
/// cam. The console sends them to `claude` as well, which handles them its
/// own way. Closing the console still ends cam.
unsafe extern "system" fn ignore(ctrl_type: u32) -> BOOL {
    BOOL::from(ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT)
}

/// Ignores Ctrl-C and Ctrl-Break until dropped, while a child runs. A
/// handler rather than the console's ignore flag, which children inherit.
pub struct IgnoreInterrupts {
    installed: bool,
}

impl IgnoreInterrupts {
    pub fn new() -> Self {
        // SAFETY: `ignore` only compares its argument.
        let installed = unsafe { SetConsoleCtrlHandler(Some(ignore), 1) } != 0;
        Self { installed }
    }
}

impl Drop for IgnoreInterrupts {
    fn drop(&mut self) {
        if self.installed {
            // SAFETY: removes the handler `new` added.
            unsafe { SetConsoleCtrlHandler(Some(ignore), 0) };
        }
    }
}

/// A job object that ends `claude` if cam ends first, such as when a
/// program stops the `claude` it started, which is cam. Only `claude` is in
/// it: what `claude` starts breaks away, as it would outlive `claude` on
/// Unix.
struct EndWithCam(HANDLE);

impl EndWithCam {
    fn new(child: &Child) -> Option<Self> {
        // SAFETY: the job handle is checked before use and closed on drop,
        // the limit information is a plain struct passed with its size,
        // and the child's handle stays open while `child` lives.
        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return None;
            }
            let job = Self(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            let set = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast::<c_void>(),
                mem::size_of_val(&limits) as u32,
            );
            (set != 0 && AssignProcessToJobObject(job.0, child.as_raw_handle()) != 0).then_some(job)
        }
    }
}

impl Drop for EndWithCam {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Runs `command` in cam's place: with the console passed through and
/// Ctrl-C left to it, then exits with its exit code. Windows can't replace
/// a process with another, so cam waits, as Claude Code's own launcher
/// does on Windows. Returns only if `command` can't be started or waited
/// for.
pub fn run_instead(mut command: Command) -> io::Error {
    let _ignored = IgnoreInterrupts::new();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return err,
    };
    // Best effort: without it, `claude` still runs.
    let _job = EndWithCam::new(&child);
    match child.wait() {
        Ok(status) => process::exit(status.code().unwrap_or(1)),
        Err(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::process::ExitStatusExt;

    use super::*;

    #[test]
    fn describes_how_a_command_ended() {
        assert_eq!(describe_exit(ExitStatus::from_raw(1)), "exit status 1");
        assert_eq!(
            describe_exit(ExitStatus::from_raw(STATUS_CONTROL_C_EXIT)),
            "interrupted"
        );
    }

    #[test]
    fn a_command_is_found_by_its_extensions() {
        let names = executable_names("claude");
        assert!(names.contains(&OsString::from("claude.exe")), "{names:?}");
        assert!(names.contains(&OsString::from("claude.cmd")), "{names:?}");
        assert!(is_named(Path::new(r"C:\Users\me\bin\CLAUDE.EXE"), "claude"));
        assert!(is_named(Path::new("claude"), "claude"));
        assert!(!is_named(Path::new("claude.txt"), "claude"));
        assert!(!is_named(Path::new("claude-code.exe"), "claude"));
    }
}
