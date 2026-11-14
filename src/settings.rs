//! Claude Code's settings files, scanned for what would beat cam's token
//! (see [`crate::conflict`]). Only names are reported, never values:
//! `env` values and `apiKeyHelper` commands can hold secrets.
//!
//! Files, per https://code.claude.com/docs/en/settings.md:
//! - User: `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`.
//! - Project: `.claude/settings.json` in the current directory.
//! - Local: `.claude/settings.local.json` at the git root, else in the
//!   current directory.
//!
//! Managed settings aren't checked: remote ones can't be read, and the file
//! ones live outside anything cam controls in tests.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::conflict::{CLOUD, Conflict, is_truthy};
use crate::discover::same_file;
use crate::launch::{ENV_TOKEN, OUTRANKING};

pub const ENV_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
const DIR: &str = ".claude";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    /// Project and local files. They can't set `CLAUDE_CODE_OAUTH_TOKEN`.
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
    pub scope: Scope,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Inspection {
    Missing,
    /// Can't be read or isn't JSON. The reason never quotes the file.
    Unreadable(String),
    Read(Vec<Conflict>),
}

/// The settings files Claude Code would read when started in `cwd`.
pub fn files(config_dir: Option<&OsStr>, home: Option<&Path>, cwd: &Path) -> Vec<File> {
    let user = match config_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(Path::new(dir).join("settings.json")),
        None => home.map(|home| home.join(DIR).join("settings.json")),
    };
    let root = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(cwd);
    let project = [
        cwd.join(DIR).join("settings.json"),
        root.join(DIR).join("settings.local.json"),
    ];
    let mut files: Vec<File> = user
        .into_iter()
        .map(|path| File {
            path,
            scope: Scope::User,
        })
        .collect();
    for path in project {
        // Started in the home directory, the user file is also the project
        // file. Count it once, as the user file.
        if !files.iter().any(|file| same_file(&file.path, &path)) {
            files.push(File {
                path,
                scope: Scope::Project,
            });
        }
    }
    files
}

pub fn inspect(file: &File) -> Inspection {
    match fs::read_to_string(&file.path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Inspection::Missing,
        Err(err) => Inspection::Unreadable(err.to_string()),
        Ok(text) => match scan(&text, file.scope) {
            Ok(conflicts) => Inspection::Read(conflicts),
            Err(err) => Inspection::Unreadable(format!("not valid JSON ({err})")),
        },
    }
}

/// The conflicts a settings file's text sets up.
pub fn scan(text: &str, scope: Scope) -> Result<Vec<Conflict>, serde_json::Error> {
    let settings: Value = serde_json::from_str(text)?;
    let mut found = Vec::new();
    if settings
        .get("apiKeyHelper")
        .and_then(as_text)
        .is_some_and(|helper| !helper.trim().is_empty())
    {
        found.push(Conflict::ApiKeyHelper);
    }
    let Some(env) = settings.get("env").and_then(Value::as_object) else {
        return Ok(found);
    };
    let value = |var: &str| env.get(var).and_then(as_text);
    for (var, provider) in CLOUD {
        if value(var).is_some_and(|value| is_truthy(&value)) {
            found.push(Conflict::Cloud { var, provider });
        }
    }
    for var in OUTRANKING {
        if value(var).is_some_and(|value| !value.is_empty()) {
            found.push(Conflict::Credential(var));
        }
    }
    if scope == Scope::User && value(ENV_TOKEN).is_some_and(|value| !value.is_empty()) {
        found.push(Conflict::Token);
    }
    Ok(found)
}

/// A JSON scalar as the string an environment variable would hold.
fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tree;

    #[test]
    fn finds_every_kind_of_conflict() {
        let text = r#"{
            "apiKeyHelper": "~/bin/key.sh",
            "env": {
                "CLAUDE_CODE_USE_VERTEX": "true",
                "CLAUDE_CODE_USE_BEDROCK": "0",
                "CLAUDE_CODE_USE_FOUNDRY": 1,
                "ANTHROPIC_AUTH_TOKEN": "",
                "ANTHROPIC_API_KEY": "x",
                "CLAUDE_CODE_OAUTH_TOKEN": "x",
                "OTHER": "x"
            }
        }"#;
        let vertex = Conflict::Cloud {
            var: "CLAUDE_CODE_USE_VERTEX",
            provider: "Google Cloud",
        };
        let foundry = Conflict::Cloud {
            var: "CLAUDE_CODE_USE_FOUNDRY",
            provider: "Microsoft Foundry",
        };
        let common = vec![
            Conflict::ApiKeyHelper,
            vertex,
            foundry,
            Conflict::Credential("ANTHROPIC_API_KEY"),
        ];
        let mut user = common.clone();
        user.push(Conflict::Token);
        assert_eq!(scan(text, Scope::User).unwrap(), user);
        // Project files can't set the token.
        assert_eq!(scan(text, Scope::Project).unwrap(), common);
    }

    #[test]
    fn ordinary_settings_have_no_conflicts() {
        for text in [
            "{}",
            r#"{"model": "opus", "env": {"DEBUG": "1"}}"#,
            r#"{"apiKeyHelper": "", "env": []}"#,
            r#"{"apiKeyHelper": null}"#,
            "[]",
        ] {
            assert_eq!(scan(text, Scope::User).unwrap(), [], "{text}");
        }
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(scan("{", Scope::User).is_err());
        assert!(scan("", Scope::User).is_err());
    }

    #[test]
    fn inspect_reads_missing_unreadable_and_valid_files() {
        let tree = Tree::new();
        let file = |rel: &str| File {
            path: tree.root.join(rel),
            scope: Scope::User,
        };
        assert_eq!(inspect(&file("missing.json")), Inspection::Missing);
        fs::write(tree.root.join("bad.json"), "{ sk-ant-oat01-SECRET").unwrap();
        match inspect(&file("bad.json")) {
            Inspection::Unreadable(reason) => {
                assert!(reason.starts_with("not valid JSON"), "{reason}");
                assert!(!reason.contains("SECRET"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        tree.dir("dir.json");
        assert!(matches!(
            inspect(&file("dir.json")),
            Inspection::Unreadable(_)
        ));
        fs::write(tree.root.join("ok.json"), r#"{"apiKeyHelper": "x"}"#).unwrap();
        assert_eq!(
            inspect(&file("ok.json")),
            Inspection::Read(vec![Conflict::ApiKeyHelper])
        );
    }

    fn paths(files: &[File]) -> Vec<(PathBuf, Scope)> {
        files
            .iter()
            .map(|file| (file.path.clone(), file.scope))
            .collect()
    }

    #[test]
    fn files_outside_git() {
        let tree = Tree::new();
        let home = tree.dir("home");
        let cwd = tree.dir("work/repo");
        assert_eq!(
            paths(&files(None, Some(&home), &cwd)),
            [
                (home.join(".claude/settings.json"), Scope::User),
                (cwd.join(".claude/settings.json"), Scope::Project),
                (cwd.join(".claude/settings.local.json"), Scope::Project),
            ]
        );
    }

    #[test]
    fn files_use_the_config_dir_and_the_git_root() {
        let tree = Tree::new();
        let config_dir = tree.dir("claude-config");
        let repo = tree.dir("repo");
        tree.dir("repo/.git");
        let cwd = tree.dir("repo/src/deep");
        assert_eq!(
            paths(&files(
                Some(config_dir.as_os_str()),
                Some(&tree.dir("home")),
                &cwd
            )),
            [
                (config_dir.join("settings.json"), Scope::User),
                (cwd.join(".claude/settings.json"), Scope::Project),
                (repo.join(".claude/settings.local.json"), Scope::Project),
            ]
        );
        // An empty CLAUDE_CONFIG_DIR counts as unset.
        let home = tree.dir("home");
        assert_eq!(
            files(Some(OsStr::new("")), Some(&home), &cwd)[0].path,
            home.join(".claude/settings.json")
        );
    }

    #[test]
    fn files_count_the_home_settings_once() {
        let tree = Tree::new();
        let home = tree.dir("home");
        assert_eq!(
            paths(&files(None, Some(&home), &home)),
            [
                (home.join(".claude/settings.json"), Scope::User),
                (home.join(".claude/settings.local.json"), Scope::Project),
            ]
        );
    }
}
