// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The single-writer lock and the refusal of network filesystems.
//!
//! On Unix the lock is a bounded poll of `flock` on `<ledger>.lock`; the kernel drops it when
//! the holder dies, so there is nothing stale to clean. Elsewhere the lock is a file holding the
//! holder's pid, and a file whose holder is gone is broken before waiting.

use super::LedgerError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The exclusive writer lock; released when dropped.
pub(super) struct WriterLock {
    #[cfg(unix)]
    _file: std::fs::File,
    #[cfg(not(unix))]
    path: PathBuf,
}

pub(super) fn lock_path(ledger: &Path) -> PathBuf {
    let mut name = ledger.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

impl WriterLock {
    #[cfg(unix)]
    pub(super) fn acquire(ledger: &Path, wait: Duration) -> Result<Self, LedgerError> {
        use rustix::fs::{FlockOperation, flock};
        let path = lock_path(ledger);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| LedgerError::Io {
                path: path.clone(),
                source,
            })?;
        let started = Instant::now();
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(Self { _file: file }),
                Err(rustix::io::Errno::WOULDBLOCK) if started.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    return Err(LedgerError::LockTimeout {
                        path: ledger.to_owned(),
                        waited: started.elapsed(),
                    });
                }
                Err(errno) => {
                    return Err(LedgerError::Io {
                        path,
                        source: errno.into(),
                    });
                }
            }
        }
    }

    #[cfg(not(unix))]
    pub(super) fn acquire(ledger: &Path, wait: Duration) -> Result<Self, LedgerError> {
        use std::io::Write;
        let path = lock_path(ledger);
        let started = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if break_if_stale(&path, &pid_alive).unwrap_or(false) {
                        continue;
                    }
                    if started.elapsed() >= wait {
                        return Err(LedgerError::LockTimeout {
                            path: ledger.to_owned(),
                            waited: started.elapsed(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(source) => return Err(LedgerError::Io { path, source }),
            }
        }
    }
}

#[cfg(not(unix))]
impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Removes the lock file when its holder is gone. The file is renamed to a name only this
/// process uses before it is deleted, so two breakers cannot remove a fresh lock between them.
/// A file with no readable pid is stale only after ten seconds, so a holder that has created
/// the file but not yet written its pid is left alone.
#[cfg(any(not(unix), test))]
pub(super) fn break_if_stale(path: &Path, alive: &dyn Fn(u32) -> bool) -> std::io::Result<bool> {
    let text = std::fs::read_to_string(path)?;
    let stale = match text.trim().parse::<u32>() {
        Ok(pid) => !alive(pid),
        Err(_) => std::fs::metadata(path)?
            .modified()?
            .elapsed()
            .is_ok_and(|age| age > Duration::from_secs(10)),
    };
    if !stale {
        return Ok(false);
    }
    let mut claimed = path.as_os_str().to_owned();
    claimed.push(format!(".stale.{}", std::process::id()));
    let claimed = PathBuf::from(claimed);
    match std::fs::rename(path, &claimed) {
        Ok(()) => {
            let _ = std::fs::remove_file(&claimed);
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// Whether a process with this pid exists.
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map_or(true, |out| {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .any(|word| word == pid.to_string())
        })
}

/// Without a way to ask, the holder is assumed alive and the lock is never broken.
#[cfg(all(not(unix), not(windows)))]
fn pid_alive(_pid: u32) -> bool {
    true
}

/// Filesystem types that cannot give a SQLite database safe cross-process locking.
pub(super) fn is_network_fstype(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "nfs"
            | "nfs3"
            | "nfs4"
            | "cifs"
            | "smb"
            | "smb2"
            | "smb3"
            | "smbfs"
            | "9p"
            | "drvfs"
            | "afs"
            | "afpfs"
            | "webdav"
            | "ceph"
            | "glusterfs"
            | "lustre"
            | "fuse.sshfs"
    )
}

/// The filesystem type of the longest mount point in `mountinfo` (Linux
/// `/proc/self/mountinfo`) that is a prefix of `dir`.
#[cfg(any(target_os = "linux", test))]
pub(super) fn mountinfo_fstype(mountinfo: &str, dir: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount) = left.split_whitespace().nth(4) else {
            continue;
        };
        let Some(fstype) = right.split_whitespace().next() else {
            continue;
        };
        let mount = mount.replace("\\040", " ");
        if dir.starts_with(&mount) && best.as_ref().is_none_or(|(len, _)| mount.len() >= *len) {
            best = Some((mount.len(), fstype.to_owned()));
        }
    }
    best.map(|(_, fstype)| fstype)
}

/// Refuses a ledger directory on a network filesystem or a WSL Windows drive.
pub(super) fn refuse_network(ledger: &Path) -> Result<(), LedgerError> {
    let Some(kind) = network_kind(ledger) else {
        return Ok(());
    };
    Err(LedgerError::NetworkFilesystem {
        path: ledger.to_owned(),
        kind,
    })
}

#[cfg(target_os = "linux")]
fn network_kind(ledger: &Path) -> Option<String> {
    let dir = std::fs::canonicalize(ledger.parent()?).ok()?;
    let wsl = std::fs::read_to_string("/proc/version")
        .is_ok_and(|text| text.to_ascii_lowercase().contains("microsoft"));
    if wsl && dir.starts_with("/mnt") && dir.components().count() > 2 {
        return Some("WSL Windows drive".to_owned());
    }
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    mountinfo_fstype(&mountinfo, &dir).filter(|name| is_network_fstype(name))
}

#[cfg(target_os = "macos")]
fn network_kind(ledger: &Path) -> Option<String> {
    let stat = rustix::fs::statfs(ledger.parent()?).ok()?;
    let bytes: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    let name = String::from_utf8_lossy(&bytes).into_owned();
    is_network_fstype(&name).then_some(name)
}

#[cfg(windows)]
fn network_kind(ledger: &Path) -> Option<String> {
    let text = ledger.to_string_lossy();
    (text.starts_with("\\\\") && !text.starts_with("\\\\?\\")).then(|| "UNC share".to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn network_kind(_ledger: &Path) -> Option<String> {
    None
}
