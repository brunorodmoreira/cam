//! Temp directory trees for unit tests.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::marker::FILE_NAME;

pub struct Tree {
    _tmp: TempDir,
    /// Canonical root, so paths compare equal to what the code builds.
    pub root: PathBuf,
}

impl Tree {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        Self { _tmp: tmp, root }
    }

    /// Creates `root/rel` (and parents) and returns it.
    pub fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// Writes `root/rel/.claude-account` with `content` and returns its path.
    pub fn marker(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.dir(rel).join(FILE_NAME);
        fs::write(&path, content).unwrap();
        path
    }

    /// Writes the config file and returns its path.
    pub fn config(&self, toml: &str) -> PathBuf {
        let path = self.root.join("config.toml");
        fs::write(&path, toml).unwrap();
        path
    }
}
