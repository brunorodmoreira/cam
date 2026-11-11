//! `which`, `use` and `unuse`.

use std::env;
use std::fmt::Display;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};

use crate::account::Selection;
use crate::config::Config;
use crate::error::Error;
use crate::marker::{self, FILE_NAME};
use crate::redact::redact;
use crate::resolve;

/// Prints a line to stdout, redacted like everything else cam prints.
fn say(line: impl Display) {
    println!("{}", redact(&line.to_string()));
}

pub fn which(config_path: &Path) -> Result<()> {
    let env_account = resolve::env_account()?;
    let cwd = current_dir()?;
    let resolution = resolve::resolve(env_account.as_deref(), &cwd, config_path)?;
    say(resolution);
    Ok(())
}

pub fn use_account(name: &str, dir: Option<&Path>, config_path: &Path) -> Result<()> {
    let selection = Selection::parse(name).ok_or_else(|| Error::InvalidNameArg {
        name: name.to_owned(),
    })?;
    if let Selection::Account(name) = &selection
        && !Config::load(config_path)?.has_account(name)
    {
        return Err(Error::UnknownAccountArg { name: name.clone() }.into());
    }
    let dir = target_dir(dir)?;
    let path = marker::write(&dir, &selection)
        .with_context(|| format!("cannot write {}", dir.join(FILE_NAME).display()))?;
    say(format_args!("wrote {} ({selection})", path.display()));
    Ok(())
}

pub fn unuse(dir: Option<&Path>) -> Result<()> {
    let dir = target_dir(dir)?;
    let path = dir.join(FILE_NAME);
    match fs::remove_file(&path) {
        Ok(()) => {
            say(format_args!("removed {}", path.display()));
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let mut message = format!("no {FILE_NAME} in {}", dir.display());
            if let Some(nearest) = dir.parent().and_then(marker::nearest) {
                let parent = nearest.parent().unwrap_or(&dir);
                message.push_str(&format!(
                    "\nhint: the nearest one is {}; pass --dir {} to remove it",
                    nearest.display(),
                    parent.display()
                ));
            }
            Err(anyhow!(message))
        }
        Err(err) => Err(err).with_context(|| format!("cannot remove {}", path.display())),
    }
}

fn current_dir() -> Result<PathBuf> {
    env::current_dir().context("cannot read the current directory")
}

/// `dir`, or the current directory, made absolute. Must be an existing directory.
fn target_dir(dir: Option<&Path>) -> Result<PathBuf> {
    let dir = match dir {
        Some(dir) => dir.to_owned(),
        None => current_dir()?,
    };
    let canonical =
        fs::canonicalize(&dir).with_context(|| format!("cannot open {}", dir.display()))?;
    ensure!(canonical.is_dir(), "{} is not a directory", dir.display());
    Ok(canonical)
}
