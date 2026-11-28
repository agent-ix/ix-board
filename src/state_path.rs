// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Shared repository protection for the fleet ledger and retro store.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use ix_trace_rs::implements;

const LINK_LIMIT: usize = 40;

/// Returns the offending `.git` entry, allowing only untracked user state/data
/// in a repository rooted at HOME. Git/index errors fail closed.
#[implements("FR-025-AC-3", "FR-039-AC-5")]
pub(crate) fn forbidden_git(resolved: &Path) -> std::io::Result<Option<PathBuf>> {
    for directory in resolved.ancestors().skip(1) {
        let git = directory.join(".git");
        if std::fs::symlink_metadata(&git).is_ok() {
            if home_state_path(directory, resolved)? && untracked(directory, resolved) {
                return Ok(None);
            }
            return Ok(Some(git));
        }
    }
    Ok(None)
}

#[implements("FR-025-AC-3", "FR-039-AC-5")]
fn home_state_path(root: &Path, path: &Path) -> std::io::Result<bool> {
    let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let home = resolve_links(Path::new(&home))?;
    if root != home {
        return Ok(false);
    }
    for (variable, fallback) in [
        ("XDG_STATE_HOME", ".local/state"),
        ("XDG_DATA_HOME", ".local/share"),
    ] {
        let base = std::env::var_os(variable)
            .map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .unwrap_or_else(|| home.join(fallback));
        if path.starts_with(resolve_links(&base)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[implements("FR-025-AC-3", "FR-039-AC-5")]
fn untracked(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    Command::new("git")
        .args(["--literal-pathspecs", "-C"])
        .arg(root)
        .args(["ls-files", "--cached", "--error-unmatch", "--"])
        .arg(relative)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .is_ok_and(|output| output.status.code() == Some(1))
}

/// Resolves `path` through every symbolic link, including links that dangle and components
/// that do not exist yet.
#[implements("FR-025-AC-3", "FR-039-AC-5")]
pub(crate) fn resolve_links(path: &Path) -> std::io::Result<PathBuf> {
    let mut pending = vec![std::path::absolute(path)?];
    let mut followed = 0;
    while let Some(candidate) = pending.pop() {
        let mut existing = candidate.clone();
        let mut tail = Vec::new();
        loop {
            match std::fs::canonicalize(&existing) {
                Ok(resolved) => {
                    let mut full = resolved;
                    full.extend(tail.iter().rev());
                    return Ok(full);
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                Err(_) => {}
            }
            // A dangling link is not canonicalizable; follow it by hand.
            if std::fs::symlink_metadata(&existing).is_ok_and(|meta| meta.is_symlink()) {
                followed += 1;
                if followed > LINK_LIMIT {
                    return Err(std::io::Error::other("too many symbolic links"));
                }
                let target = std::fs::read_link(&existing)?;
                let base = existing.parent().unwrap_or(Path::new("/")).join(target);
                let mut next = base;
                next.extend(tail.iter().rev());
                pending.push(next);
                break;
            }
            let Some(name) = existing.file_name().map(OsString::from) else {
                return Err(std::io::Error::other("path has no existing ancestor"));
            };
            tail.push(name);
            if !existing.pop() {
                return Err(std::io::Error::other("path has no existing ancestor"));
            }
        }
    }
    Err(std::io::Error::other("path did not resolve"))
}
