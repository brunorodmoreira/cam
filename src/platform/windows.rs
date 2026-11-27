//! Windows.

use std::env;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::mem;
use std::os::windows::fs::FileTypeExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, ExitStatus};
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
use windows_sys::core::BOOL;

/// Windows' answer when it won't make a symlink: Developer Mode is off and
/// the user isn't an administrator.
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

/// What cam says instead of [`ERROR_PRIVILEGE_NOT_HELD`].
const FILE_LINKS_NEED_DEVELOPER_MODE: &str =
    "Windows makes links to files only with Developer Mode on (Settings > System > For developers)";

/// What `cmd.exe` uses when `PATHEXT` is unset.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// How a process ends when Ctrl-C reaches it unhandled.
const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// The home dir as Claude Code finds it, through Node's `os.homedir()`:
/// `USERPROFILE` if set and not empty, else the profile folder Windows
/// knows. `HOME`, which Git Bash sets, is ignored, as Claude Code ignores
/// it for its config dir.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// The absolute path with links resolved, without the `\\?\` prefix
/// `fs::canonicalize` adds, as Windows tools print paths. A path that needs
/// it keeps it: one at least `MAX_PATH` long, or not on a drive letter.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path).map(simplified)
}

fn simplified(path: PathBuf) -> PathBuf {
    let plain = path.to_str().and_then(|text| text.strip_prefix(r"\\?\"));
    match plain {
        Some(rest)
            if rest.len() < 260
                && rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && rest[1..].starts_with(":\\") =>
        {
            PathBuf::from(rest)
        }
        _ => path,
    }
}

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

/// Whether cam was started as `command`: Windows knows the file it
/// started, such as the shim's `claude.exe`, whatever the caller put in
/// `argv[0]`.
pub fn started_as(argv0: &OsStr, command: &str) -> bool {
    env::current_exe().is_ok_and(|exe| is_named(&exe, command))
        || is_named(Path::new(argv0), command)
}

/// Compares the volume and file index, which also catches hard links.
pub(super) fn same_file_id(a: &Path, b: &Path) -> bool {
    same_file::is_same_file(a, b).unwrap_or(false)
}

/// A link at `link` to `target`. A folder gets a directory junction, which
/// any user can make. A file gets a symlink, which Windows makes only with
/// Developer Mode on, or for an administrator: without either, the error
/// says so.
pub fn link(target: &Path, link: &Path) -> io::Result<()> {
    if target.is_dir() {
        junction::create(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link).map_err(|err| {
            match err.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) {
                true => io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    FILE_LINKS_NEED_DEVELOPER_MODE,
                ),
                false => err,
            }
        })
    }
}

/// Whether Windows lets this user make links to files: it tries one, in
/// the temp dir. Developer Mode and the right to make them are the same
/// for every folder.
pub fn file_links_allowed() -> bool {
    let probe = env::temp_dir().join(format!(".cam-link-check-{}", process::id()));
    let _ = fs::remove_file(&probe);
    match std::os::windows::fs::symlink_file("nowhere", &probe) {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(err) => err.raw_os_error() != Some(ERROR_PRIVILEGE_NOT_HELD),
    }
}

/// A link at `to` with the same target as the link at `from`, and of the
/// same kind, even when the target is gone: a junction, or a symlink to a
/// folder or a file.
pub fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    if junction::exists(from).unwrap_or(false) {
        return junction::create(junction::get_target(from)?, to);
    }
    let target = fs::read_link(from)?;
    if fs::symlink_metadata(from)?.file_type().is_symlink_dir() {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

/// Removes the link at `path`, never what it points to. A link to a folder,
/// a junction included, is removed as a folder.
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

/// A connection to a local socket, such as Herdr's: a named pipe.
pub type LocalSocket = fs::File;

/// Opens the named pipe that stands for the socket file at `path`:
/// `\\.\pipe\` followed by the whole path, as Herdr names it. A pipe opened
/// as a file takes no timeout, so `_timeout` is unused: Herdr answers each
/// request at once, then closes the pipe.
pub fn connect_local(path: &Path, _timeout: Duration) -> io::Result<LocalSocket> {
    let mut name = OsString::from(r"\\.\pipe\");
    name.push(path);
    OpenOptions::new().read(true).write(true).open(name)
}

/// Makes `command` start without the console and in a process group of its
/// own, so Ctrl-C in the console doesn't reach it.
pub fn detach(command: &mut Command) -> &mut Command {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
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
    fn canonical_paths_drop_the_prefix_when_they_can() {
        let plain = |text: &str| simplified(PathBuf::from(text));
        assert_eq!(plain(r"\\?\C:\Users\me"), PathBuf::from(r"C:\Users\me"));
        assert_eq!(
            plain(r"\\?\UNC\server\share"),
            PathBuf::from(r"\\?\UNC\server\share")
        );
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(plain(&long), PathBuf::from(&long));
        let dir = canonicalize(&env::temp_dir()).unwrap();
        assert!(
            !dir.to_str().unwrap().starts_with(r"\\?\"),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn folders_link_through_junctions_that_go_without_their_target() {
        let tree = crate::testutil::Tree::new();
        let target = tree.dir("shared/skills");
        fs::write(target.join("a.md"), "a\n").unwrap();
        let link_path = tree.dir("account").join("skills");
        link(&target, &link_path).unwrap();
        assert!(junction::exists(&link_path).unwrap());
        assert_eq!(fs::read_to_string(link_path.join("a.md")).unwrap(), "a\n");
        assert!(crate::platform::same_file(&link_path, &target));

        let copy = tree.root.join("account").join("copy");
        copy_link(&link_path, &copy).unwrap();
        assert!(junction::exists(&copy).unwrap());

        remove_link(&link_path).unwrap();
        remove_link(&copy).unwrap();
        assert!(!link_path.exists() && !copy.exists());
        assert_eq!(fs::read_to_string(target.join("a.md")).unwrap(), "a\n");
    }

    #[test]
    fn files_link_through_symlinks_where_windows_allows_them() {
        if !file_links_allowed() {
            eprintln!("skipped: turn on Developer Mode to test links to files");
            return;
        }
        let tree = crate::testutil::Tree::new();
        let target = tree.dir("shared").join("settings.json");
        fs::write(&target, "{}\n").unwrap();
        let link_path = tree.dir("account").join("settings.json");
        link(&target, &link_path).unwrap();
        assert!(
            fs::symlink_metadata(&link_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&link_path).unwrap(), "{}\n");
        remove_link(&link_path).unwrap();
        assert!(target.exists());
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
