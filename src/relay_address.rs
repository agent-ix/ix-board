// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Shared validation for ix-relay's three-part address contract.

use ix_trace_rs::implements;

/// Returns whether an address has exactly three non-empty colon-separated segments.
#[implements("FR-027-AC-12")]
#[implements("FR-049-AC-5")]
pub(crate) fn is_valid(address: &str) -> bool {
    let mut segments = address.split(':');
    (0..3).all(|_| segments.next().is_some_and(|part| !part.trim().is_empty()))
        && segments.next().is_none()
}
