// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use ix_board::hello;

#[test]
fn hello_is_non_empty() {
    assert!(!hello().is_empty());
}
