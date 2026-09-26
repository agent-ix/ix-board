// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Keeps the Linear dependency graph honest: resolves dependencies written in ticket prose, reports every reference it cannot resolve, and never silently drops one.

/// Placeholder entry point.
pub fn hello() -> &'static str {
    "hello from ix_board"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_returns_greeting() {
        assert!(hello().contains("ix_board"));
    }
}
