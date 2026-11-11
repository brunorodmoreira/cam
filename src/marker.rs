//! `.claude-account` marker files.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::account::Selection;
use crate::error::Error;

pub const FILE_NAME: &str = ".claude-account";

/// The name in a marker's text: the first line that is not empty once `#`
/// comments are stripped, trimmed. Not validated here.
pub fn parse(text: &str) -> Option<&str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .map(|line| line.split_once('#').map_or(line, |(name, _)| name).trim())
        .find(|line| !line.is_empty())
}

/// Reads the marker at `path`. `Ok(None)` means there is no marker there; a
/// marker that exists but can't be read or names nothing is an error, never
/// skipped.
pub fn read(path: &Path) -> Result<Option<String>, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let text = String::from_utf8(bytes).map_err(|_| Error::NotUtf8 {
        path: path.to_owned(),
    })?;
    match parse(&text) {
        Some(name) => Ok(Some(name.to_owned())),
        None => Err(Error::EmptyMarker {
            path: path.to_owned(),
        }),
    }
}

/// The nearest marker from `start` up to `/`, with the raw name it contains.
pub fn find(start: &Path) -> Result<Option<(PathBuf, String)>, Error> {
    for dir in start.ancestors() {
        let path = dir.join(FILE_NAME);
        if let Some(name) = read(&path)? {
            return Ok(Some((path, name)));
        }
    }
    Ok(None)
}

/// The path of the nearest marker from `start` up to `/`, without reading it.
pub fn nearest(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(FILE_NAME))
        .find(|path| path.exists())
}

/// Writes a marker naming `selection` into `dir`, replacing any existing one.
pub fn write(dir: &Path, selection: &Selection) -> io::Result<PathBuf> {
    let path = dir.join(FILE_NAME);
    fs::write(&path, format!("{selection}\n"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    #[test]
    fn parse_plain_name() {
        assert_eq!(parse("work"), Some("work"));
        assert_eq!(parse("work\n"), Some("work"));
        assert_eq!(parse("work\r\n"), Some("work"));
        assert_eq!(parse("  work \t\n"), Some("work"));
    }

    #[test]
    fn parse_skips_blank_lines_and_comments() {
        assert_eq!(
            parse("\n\n# which account\n  # indented\n\nwork\n"),
            Some("work")
        );
    }

    #[test]
    fn parse_strips_inline_comment() {
        assert_eq!(parse("work # the day job"), Some("work"));
        assert_eq!(parse("work#tight"), Some("work"));
    }

    #[test]
    fn parse_takes_first_name_only() {
        assert_eq!(parse("work\npersonal\n"), Some("work"));
    }

    #[test]
    fn parse_strips_bom() {
        assert_eq!(parse("\u{feff}work\n"), Some("work"));
    }

    #[test]
    fn parse_keeps_invalid_text_for_the_caller_to_reject() {
        assert_eq!(parse("Work Account\n"), Some("Work Account"));
    }

    #[test]
    fn parse_empty() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("\n  \n"), None);
        assert_eq!(parse("# only a comment\n#another\n"), None);
    }

    #[test]
    fn find_in_start_dir() {
        let tree = Tree::new();
        let marker = tree.marker("work", "work\n");
        assert_eq!(
            find(&tree.dir("work")).unwrap(),
            Some((marker, "work".into()))
        );
    }

    #[test]
    fn find_walks_up() {
        let tree = Tree::new();
        let marker = tree.marker("work", "work\n");
        assert_eq!(
            find(&tree.dir("work/a/b/c")).unwrap(),
            Some((marker, "work".into()))
        );
    }

    #[test]
    fn find_returns_nearest() {
        let tree = Tree::new();
        tree.marker("", "personal\n");
        let inner = tree.marker("work", "work\n");
        assert_eq!(
            find(&tree.dir("work/repo")).unwrap(),
            Some((inner, "work".into()))
        );
    }

    #[test]
    fn find_nothing() {
        let tree = Tree::new();
        assert_eq!(find(&tree.dir("a/b")).unwrap(), None);
    }

    #[test]
    fn read_rejects_empty_marker() {
        let tree = Tree::new();
        let marker = tree.marker("x", "# nothing here\n\n");
        assert!(matches!(read(&marker), Err(Error::EmptyMarker { path }) if path == marker));
    }

    #[test]
    fn read_rejects_non_utf8() {
        let tree = Tree::new();
        let path = tree.dir("x").join(FILE_NAME);
        fs::write(&path, [0xff, 0xfe, b'w']).unwrap();
        assert!(matches!(read(&path), Err(Error::NotUtf8 { .. })));
    }

    #[test]
    fn read_rejects_unreadable_marker() {
        let tree = Tree::new();
        let path = tree.dir("x").join(FILE_NAME);
        fs::create_dir(&path).unwrap();
        assert!(matches!(read(&path), Err(Error::Read { .. })));
    }

    #[test]
    fn nearest_finds_marker_path() {
        let tree = Tree::new();
        let marker = tree.marker("work", "work\n");
        assert_eq!(nearest(&tree.dir("work/a")), Some(marker));
        assert_eq!(nearest(&tree.dir("other")), None);
    }

    #[test]
    fn write_then_read_round_trips() {
        let tree = Tree::new();
        let dir = tree.dir("x");
        let path = write(&dir, &Selection::Account("work".into())).unwrap();
        assert_eq!(path, dir.join(FILE_NAME));
        assert_eq!(fs::read_to_string(&path).unwrap(), "work\n");
        write(&dir, &Selection::PassThrough).unwrap();
        assert_eq!(read(&path).unwrap(), Some("none".into()));
    }
}
