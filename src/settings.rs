// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Small configuration-file loader shared by cache and transport settings.

use crate::Error;
use std::path::PathBuf;

pub(crate) fn load() -> Result<Option<toml::Value>, Error> {
    let explicit = std::env::var_os("IX_BOARD_CONFIG_PATH");
    let path = explicit.clone().map(PathBuf::from).or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|directory| directory.join("ix-board/config.toml"))
    });
    let Some(path) = path else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .map_err(|_| Error::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
            Ok(None)
        }
        Err(_) => Err(Error::InvalidConfiguration),
    }
}
