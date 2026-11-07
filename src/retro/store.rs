// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The append-only JSONL store outside git (FR-025).

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use thiserror::Error as ThisError;

use super::catalog::{Record, SCHEMA_VERSION};

/// Environment variable naming the store root.
pub const HOME_VARIABLE: &str = "TEAM_RETRO_HOME";
/// Store file name inside the team directory.
pub const FILE_NAME: &str = "snapshots.jsonl";
/// Default store root below the home directory.
const DEFAULT_ROOT: &str = ".local/state/team-retro";
/// Most symbolic links followed while resolving a store path.
const LINK_LIMIT: usize = 40;

/// Failures resolving, reading or appending the store.
#[derive(Debug, ThisError)]
pub enum StoreError {
    /// The team name cannot be a single directory name.
    #[error("team {0:?} cannot name a store directory")]
    TeamName(String),
    /// Neither `TEAM_RETRO_HOME` nor a home directory is available.
    #[error("cannot resolve the retro store: set {HOME_VARIABLE} or HOME")]
    NoHome,
    /// The resolved store path lies inside a git work tree.
    #[error(
        "refusing to write retro data to {path}: it resolves inside a git work tree (found {git})"
    )]
    InsideGit {
        /// The offending resolved path.
        path: PathBuf,
        /// The `.git` entry that was found.
        git: PathBuf,
    },
    /// A line of the store does not parse.
    #[error("{path}: line {line} does not parse: {message}")]
    Line {
        /// Store file.
        path: PathBuf,
        /// One-based line number.
        line: usize,
        /// Decoder message.
        message: String,
    },
    /// A record carries a newer schema version than this build understands.
    #[error(
        "{path}: line {line} has schema_version {found}; this build understands up to {SCHEMA_VERSION}"
    )]
    NewerSchema {
        /// Store file.
        path: PathBuf,
        /// One-based line number.
        line: usize,
        /// Version found.
        found: u64,
    },
    /// The file does not end in a newline, so an append would corrupt its last line.
    #[error("{0}: the last byte is not a newline; refusing to append")]
    MissingFinalNewline(PathBuf),
    /// Filesystem failure.
    #[error("{path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
}

/// The store of one team.
#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// Resolves `$TEAM_RETRO_HOME/<team>/snapshots.jsonl` from the process environment.
    pub fn from_env(team: &str) -> Result<Self, StoreError> {
        Self::resolve(
            team,
            std::env::var_os(HOME_VARIABLE),
            std::env::var_os("HOME"),
        )
    }

    /// Resolves the store path from explicit environment values.
    pub fn resolve(
        team: &str,
        retro_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<Self, StoreError> {
        let mut parts = Path::new(team).components();
        let single = matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        );
        if !single || team.contains(['/', '\\']) {
            return Err(StoreError::TeamName(team.to_owned()));
        }
        let root = match retro_home.filter(|value| !value.is_empty()) {
            Some(root) => PathBuf::from(root),
            None => PathBuf::from(
                home.filter(|value| !value.is_empty())
                    .ok_or(StoreError::NoHome)?,
            )
            .join(DEFAULT_ROOT),
        };
        Ok(Self {
            path: root.join(team).join(FILE_NAME),
        })
    }

    /// The unresolved store file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads every record ordered by `taken_at`, file order breaking ties.
    ///
    /// A missing file is an empty store. Older schema versions are read as stored.
    pub fn read_all(&self) -> Result<Vec<Record>, StoreError> {
        let text = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(self.io(source)),
        };
        let mut keyed = Vec::new();
        for (index, line) in text.split(|byte| *byte == b'\n').enumerate() {
            let number = index + 1;
            if line.is_empty() {
                continue;
            }
            let line_error = |message: String| StoreError::Line {
                path: self.path.clone(),
                line: number,
                message,
            };
            let value: serde_json::Value =
                serde_json::from_slice(line).map_err(|error| line_error(error.to_string()))?;
            if let Some(found) = value
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)
                && found > u64::from(SCHEMA_VERSION)
            {
                return Err(StoreError::NewerSchema {
                    path: self.path.clone(),
                    line: number,
                    found,
                });
            }
            let record: Record =
                serde_json::from_value(value).map_err(|error| line_error(error.to_string()))?;
            let taken = crate::drift::timestamp_epoch(&record.taken_at)
                .map_err(|error| line_error(error.to_string()))?;
            keyed.push((taken, record));
        }
        keyed.sort_by(|left, right| left.0.total_cmp(&right.0));
        Ok(keyed.into_iter().map(|(_, record)| record).collect())
    }

    /// Refuses when the resolved store path is inside a git work tree. Creates nothing.
    pub fn check_writable(&self) -> Result<(), StoreError> {
        let resolved = resolve_links(&self.path).map_err(|source| self.io(source))?;
        refuse_inside_git(&resolved)
    }

    /// Appends one record as a single write to a file opened in append mode.
    ///
    /// Refuses before creating anything when the resolved path is inside a git work tree or
    /// the file does not end in a newline.
    pub fn append(&self, record: &Record) -> Result<(), StoreError> {
        let resolved = resolve_links(&self.path).map_err(|source| self.io(source))?;
        refuse_inside_git(&resolved)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        let mut line = serde_json::to_vec(record).map_err(|error| StoreError::Line {
            path: self.path.clone(),
            line: 0,
            message: error.to_string(),
        })?;
        line.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&resolved)
            .map_err(|source| self.io(source))?;
        let length = file.metadata().map_err(|source| self.io(source))?.len();
        if length > 0 {
            let mut last = [0_u8; 1];
            file.seek(SeekFrom::Start(length - 1))
                .and_then(|_| file.read_exact(&mut last))
                .map_err(|source| self.io(source))?;
            if last[0] != b'\n' {
                return Err(StoreError::MissingFinalNewline(self.path.clone()));
            }
        }
        file.write_all(&line).map_err(|source| self.io(source))
    }

    fn io(&self, source: std::io::Error) -> StoreError {
        StoreError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

/// Resolves `path` through every symbolic link, including links that dangle and components
/// that do not exist yet.
fn resolve_links(path: &Path) -> std::io::Result<PathBuf> {
    let mut pending = vec![path.to_owned()];
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

fn refuse_inside_git(resolved: &Path) -> Result<(), StoreError> {
    for directory in resolved.ancestors().skip(1) {
        let git = directory.join(".git");
        if std::fs::symlink_metadata(&git).is_ok() {
            return Err(StoreError::InsideGit {
                path: resolved.to_owned(),
                git,
            });
        }
    }
    Ok(())
}
