//! `cam init <shell>`: the code a shell's startup file evaluates so that
//! plain `claude` goes through cam. It puts the dir of the `claude` → `cam`
//! link first on `PATH`, and loads completions with the account names.
//!
//! One line at the end of the startup file does it all:
//! `eval "$(cam init zsh)"`, or in PowerShell's profile
//! `Invoke-Expression (& cam init powershell | Out-String)`. cam never
//! edits startup files itself. On Windows, PowerShell is the only shell.

use std::ffi::OsStr;
use std::path::Path;

use clap::ValueEnum;

/// The shells `cam init` writes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

/// What separates `PATH` entries here.
const PATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

impl Shell {
    /// The shell named by `$SHELL`, if it's one of these. On Windows it's
    /// always PowerShell, the only one `cam init` sets up there.
    pub fn from_env(shell: Option<&OsStr>) -> Option<Self> {
        if cfg!(windows) {
            return Some(Self::Powershell);
        }
        match Path::new(shell?).file_name()?.to_str()? {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            "pwsh" => Some(Self::Powershell),
            _ => None,
        }
    }

    pub fn completions(self) -> clap_complete::Shell {
        match self {
            Self::Bash => clap_complete::Shell::Bash,
            Self::Zsh => clap_complete::Shell::Zsh,
            Self::Fish => clap_complete::Shell::Fish,
            Self::Powershell => clap_complete::Shell::PowerShell,
        }
    }
}

/// The startup file for `shell`, and the line that goes at its end.
/// Without a known shell, a placeholder line.
pub fn setup(shell: Option<Shell>) -> (&'static str, &'static str) {
    match shell {
        Some(Shell::Zsh) => ("~/.zshrc", r#"eval "$(cam init zsh)""#),
        Some(Shell::Bash) => ("~/.bashrc", r#"eval "$(cam init bash)""#),
        Some(Shell::Fish) => ("~/.config/fish/config.fish", "cam init fish | source"),
        Some(Shell::Powershell) => (
            "your PowerShell profile ($PROFILE)",
            "Invoke-Expression (& cam init powershell | Out-String)",
        ),
        None => ("your shell's startup file", r#"eval "$(cam init <shell>)""#),
    }
}

/// The code that puts `dir` first on `PATH`, and takes it out of anywhere
/// else, so it stays first even when a nested shell's startup file adds
/// `~/.local/bin` in front again. `path` is the current `PATH`. One that
/// isn't UTF-8 is left as it is, with `dir` added in front.
pub fn path_code(shell: Shell, dir: &str, path: Option<&OsStr>) -> String {
    let separator = match shell {
        Shell::Powershell => PATH_SEPARATOR,
        _ => ':',
    };
    let entries: Option<Vec<&str>> = match path.map(OsStr::to_str) {
        None => Some(vec![dir]),
        Some(Some(path)) => Some(
            std::iter::once(dir)
                .chain(path.split(separator).filter(|entry| !same_dir(entry, dir)))
                .collect(),
        ),
        Some(None) => None,
    };
    match (shell, entries) {
        (Shell::Powershell, Some(entries)) => {
            let joined = entries.join(&separator.to_string());
            format!("$env:PATH = {}\n", ps_quote(&joined))
        }
        (Shell::Powershell, None) => format!(
            "$env:PATH = {} + [IO.Path]::PathSeparator + $env:PATH\n",
            ps_quote(dir)
        ),
        (Shell::Fish, Some(entries)) => {
            let quoted: Vec<String> = entries.iter().map(|entry| fish_quote(entry)).collect();
            format!("set -gx PATH {}\n", quoted.join(" "))
        }
        (Shell::Fish, None) => format!("set -gx PATH {} $PATH\n", fish_quote(dir)),
        (_, Some(entries)) => format!("export PATH={}\n", sh_quote(&entries.join(":"))),
        (_, None) => format!("export PATH={}:\"$PATH\"\n", sh_quote(dir)),
    }
}

/// `completions` (clap's script), made safe to run at every shell start.
/// zsh's needs `compdef`, which exists only once `compinit` has run, so
/// it's skipped without it rather than failing.
pub fn completions_code(shell: Shell, completions: &str) -> String {
    match shell {
        Shell::Zsh => format!("if (( $+functions[compdef] )); then\n{completions}fi\n"),
        Shell::Bash | Shell::Fish | Shell::Powershell => completions.to_owned(),
    }
}

/// Whether a `PATH` entry names `dir`, give or take trailing slashes. On
/// Windows, `\\` is a slash too, and case doesn't count.
fn same_dir(entry: &str, dir: &str) -> bool {
    let trim = |text: &str| {
        let text = text.trim_end_matches(['/', if cfg!(windows) { '\\' } else { '/' }]);
        match cfg!(windows) {
            true => text.to_lowercase(),
            false => text.to_owned(),
        }
    };
    !entry.is_empty() && trim(entry) == trim(dir)
}

/// A single-quoted word for sh, bash and zsh.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A single-quoted string for PowerShell, where a quote is written twice.
/// It takes the curly quotes for quotes too.
fn ps_quote(text: &str) -> String {
    let mut quoted = String::from("'");
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            quoted.push(c);
        }
        quoted.push(c);
    }
    quoted.push('\'');
    quoted
}

/// A single-quoted word for fish, where `\\` and `\'` are escapes.
fn fish_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', r"\\").replace('\'', r"\'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    const SHIMS: &str = "/home/me/.local/share/cam/shims";

    #[cfg(windows)]
    #[test]
    fn windows_always_gets_powershell() {
        assert_eq!(Shell::from_env(None), Some(Shell::Powershell));
        assert_eq!(
            Shell::from_env(Some(OsStr::new("/usr/bin/bash"))),
            Some(Shell::Powershell)
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_from_env() {
        assert_eq!(
            Shell::from_env(Some(OsStr::new("/bin/zsh"))),
            Some(Shell::Zsh)
        );
        assert_eq!(
            Shell::from_env(Some(OsStr::new("/opt/homebrew/bin/fish"))),
            Some(Shell::Fish)
        );
        assert_eq!(Shell::from_env(Some(OsStr::new("bash"))), Some(Shell::Bash));
        assert_eq!(Shell::from_env(Some(OsStr::new("/bin/tcsh"))), None);
        assert_eq!(Shell::from_env(None), None);
        assert_eq!(
            Shell::from_env(Some(OsStr::new("/usr/local/bin/pwsh"))),
            Some(Shell::Powershell)
        );
    }

    /// `parts` joined the way `PATH` is here: `;` on Windows, `:` elsewhere.
    fn path_of(parts: &[&str]) -> String {
        parts.join(&PATH_SEPARATOR.to_string())
    }

    #[test]
    fn powershell_path_comes_first_and_only_once() {
        let (shims, bin, other) = match cfg!(windows) {
            true => (
                r"C:\Users\me\.local\share\cam\shims",
                r"C:\Users\me\.local\bin",
                r"C:\Windows",
            ),
            false => (
                "/home/me/.local/share/cam/shims",
                "/home/me/.local/bin",
                "/usr/bin",
            ),
        };
        // Spelled with a trailing slash, and on Windows in another case.
        let again = match cfg!(windows) {
            true => format!("{}\\", shims.to_uppercase()),
            false => format!("{shims}/"),
        };
        let path = path_of(&[bin, &again, other]);
        assert_eq!(
            path_code(Shell::Powershell, shims, Some(OsStr::new(&path))),
            format!("$env:PATH = '{}'\n", path_of(&[shims, bin, other]))
        );
        assert_eq!(
            path_code(Shell::Powershell, shims, None),
            format!("$env:PATH = '{shims}'\n")
        );
    }

    #[test]
    fn powershell_quotes_double_every_kind_of_quote() {
        assert_eq!(
            path_code(Shell::Powershell, "C:\\o'neil\\\u{2019}$x", None),
            "$env:PATH = 'C:\\o''neil\\\u{2019}\u{2019}$x'\n"
        );
    }

    // Unix `PATH`s: Windows splits on `;`.
    #[cfg(unix)]
    #[test]
    fn path_comes_first_and_only_once() {
        let path = OsStr::new("/home/me/.local/bin:/home/me/.local/share/cam/shims/:/usr/bin");
        assert_eq!(
            path_code(Shell::Zsh, SHIMS, Some(path)),
            "export PATH='/home/me/.local/share/cam/shims:/home/me/.local/bin:/usr/bin'\n"
        );
        assert_eq!(
            path_code(Shell::Fish, SHIMS, Some(path)),
            "set -gx PATH '/home/me/.local/share/cam/shims' '/home/me/.local/bin' '/usr/bin'\n"
        );
        assert_eq!(
            path_code(Shell::Bash, SHIMS, None),
            "export PATH='/home/me/.local/share/cam/shims'\n"
        );
    }

    // Unix `PATH`s: Windows splits on `;`.
    #[cfg(unix)]
    #[test]
    fn path_keeps_other_entries_as_they_are() {
        // Empty entries and duplicates are the user's business.
        let path = OsStr::new("/a::/a:/b");
        assert_eq!(
            path_code(Shell::Bash, SHIMS, Some(path)),
            "export PATH='/home/me/.local/share/cam/shims:/a::/a:/b'\n"
        );
    }

    #[test]
    fn path_quotes_for_each_shell() {
        let dir = "/home/o'neil/$x\\y";
        assert_eq!(
            path_code(Shell::Zsh, dir, None),
            "export PATH='/home/o'\\''neil/$x\\y'\n"
        );
        assert_eq!(
            path_code(Shell::Fish, dir, None),
            "set -gx PATH '/home/o\\'neil/$x\\\\y'\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_kept() {
        use std::os::unix::ffi::OsStrExt;
        let path = OsStr::from_bytes(b"/bin:/\xff");
        assert_eq!(
            path_code(Shell::Zsh, SHIMS, Some(path)),
            "export PATH='/home/me/.local/share/cam/shims':\"$PATH\"\n"
        );
        assert_eq!(
            path_code(Shell::Fish, SHIMS, Some(path)),
            "set -gx PATH '/home/me/.local/share/cam/shims' $PATH\n"
        );
    }

    #[test]
    fn zsh_completions_wait_for_compinit() {
        assert_eq!(
            completions_code(Shell::Zsh, "compdef _cam cam\n"),
            "if (( $+functions[compdef] )); then\ncompdef _cam cam\nfi\n"
        );
        assert_eq!(completions_code(Shell::Bash, "complete\n"), "complete\n");
    }

    #[test]
    fn setup_lines() {
        assert_eq!(
            setup(Some(Shell::Zsh)),
            ("~/.zshrc", "eval \"$(cam init zsh)\"")
        );
        assert_eq!(
            setup(Some(Shell::Fish)),
            ("~/.config/fish/config.fish", "cam init fish | source")
        );
        assert_eq!(setup(None).1, "eval \"$(cam init <shell>)\"");
    }
}
