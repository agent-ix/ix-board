// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Lane focus derived on read from hook events (FR-037): the ticket named by the branch, else
//! by the worktree directory, of the lane's recent tool-call events. No self-report is read and
//! nothing is stored.

use serde::Serialize;

/// One hook event of a lane with the location it touched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusEvent {
    /// Relay server time, Unix milliseconds.
    pub server_ms: i64,
    /// The working directory of the event.
    pub cwd: String,
    /// The checked-out branch of the working directory, or empty.
    pub branch: String,
}

/// The focus of a lane at one moment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Focus {
    /// Exactly one ticket in the window.
    Ticket {
        /// The ticket identifier.
        ticket: String,
    },
    /// Two or more distinct tickets in the window.
    Ambiguous {
        /// The most recent ticket.
        recent: String,
        /// Every ticket in the window, oldest first.
        tickets: Vec<String>,
    },
    /// No ticket could be derived.
    None {
        /// `no_ticket` or `no_worktree`.
        reason: &'static str,
    },
}

impl Focus {
    /// The focused ticket when the focus is unambiguous.
    #[must_use]
    pub fn ticket(&self) -> Option<&str> {
        match self {
            Self::Ticket { ticket } => Some(ticket),
            _ => None,
        }
    }

    /// A short stable text for the ledger and the report.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Ticket { ticket } => ticket.clone(),
            Self::Ambiguous { recent, .. } => format!("ambiguous:{recent}"),
            Self::None { reason } => (*reason).to_owned(),
        }
    }
}

/// The first identifier of one of `keys` in `text`, upper-cased (`ix-board-swm-211` with key
/// `SWM` gives `SWM-211`). The key must start a word and the number must end it.
#[must_use]
pub fn ticket_in(text: &str, keys: &[String]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<(usize, String)> = None;
    for key in keys {
        let key = key.to_ascii_lowercase();
        let mut from = 0;
        while let Some(found) = lower[from..].find(&key) {
            let start = from + found;
            let after = start + key.len();
            from = start + 1;
            if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
                continue;
            }
            if bytes.get(after) != Some(&b'-') {
                continue;
            }
            let digits = lower[after + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            let end = after + 1 + digits;
            if digits == 0 || bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
                continue;
            }
            let ticket = format!(
                "{}-{}",
                key.to_ascii_uppercase(),
                &lower[after + 1..after + 1 + digits]
            );
            if best.as_ref().is_none_or(|(at, _)| start < *at) {
                best = Some((start, ticket));
            }
            break;
        }
    }
    best.map(|(_, ticket)| ticket)
}

/// The ticket of one event. An event whose working directory is not a git worktree (no branch)
/// names no ticket: FR-037 reads the directory name only for a worktree.
fn event_ticket(event: &FocusEvent, keys: &[String]) -> Option<String> {
    if event.branch.is_empty() {
        return None;
    }
    ticket_in(&event.branch, keys).or_else(|| {
        event
            .cwd
            .rsplit('/')
            .find_map(|component| ticket_in(component, keys))
    })
}

/// The focus at `at_ms` over the events inside `window_ms` before it, and not before
/// `not_before_ms` when given (confirmation of an order considers only the events after the
/// order was sent, so a lane that left another ticket is not held ambiguous by the window).
#[must_use]
pub fn derive(
    events: &[FocusEvent],
    at_ms: i64,
    window_ms: i64,
    not_before_ms: Option<i64>,
    keys: &[String],
) -> Focus {
    let floor = (at_ms - window_ms).max(not_before_ms.map_or(i64::MIN, |ms| ms - 1));
    let mut inside: Vec<&FocusEvent> = events
        .iter()
        .filter(|e| e.server_ms <= at_ms && e.server_ms > floor)
        .collect();
    inside.sort_by_key(|e| e.server_ms);
    let mut tickets: Vec<String> = Vec::new();
    for event in &inside {
        if let Some(ticket) = event_ticket(event, keys) {
            tickets.retain(|t| *t != ticket);
            tickets.push(ticket);
        }
    }
    match tickets.as_slice() {
        [] => {
            let in_worktree = inside.iter().any(|e| !e.branch.is_empty());
            Focus::None {
                reason: if in_worktree {
                    "no_ticket"
                } else {
                    "no_worktree"
                },
            }
        }
        [one] => Focus::Ticket {
            ticket: one.clone(),
        },
        [.., recent] => Focus::Ambiguous {
            recent: recent.clone(),
            tickets: tickets.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;

    fn keys() -> Vec<String> {
        vec!["SWM".into()]
    }

    fn event(ms: i64, cwd: &str, branch: &str) -> FocusEvent {
        FocusEvent {
            server_ms: ms,
            cwd: cwd.into(),
            branch: branch.into(),
        }
    }

    #[trace("FR-037-AC-1")]
    #[test]
    fn branch_beats_directory_and_words_are_bounded() {
        let e = event(5, "/w/ix-board-swm-300", "swm-211-fleet-assign");
        assert_eq!(event_ticket(&e, &keys()).as_deref(), Some("SWM-211"));
        let by_directory = event(5, "/w/ix-board-swm-208", "main");
        assert_eq!(
            event_ticket(&by_directory, &keys()).as_deref(),
            Some("SWM-208")
        );
        assert_eq!(ticket_in("xswm-1", &keys()), None);
        assert_eq!(ticket_in("swm-", &keys()), None);
        assert_eq!(ticket_in("swm-211abc", &keys()), None);
        assert_eq!(ticket_in("swm-211-x", &keys()), Some("SWM-211".into()));
        assert_eq!(
            ticket_in("/w/ix-board-swm-300/src", &keys()),
            Some("SWM-300".into())
        );
    }

    #[trace("FR-037-AC-2")]
    #[test]
    fn two_tickets_in_the_window_are_ambiguous_and_old_ones_drop() {
        let events = [event(1, "/w", "swm-1-a"), event(100, "/w", "swm-2-b")];
        assert!(matches!(
            derive(&events, 100, 1_000, None, &keys()),
            Focus::Ambiguous { ref recent, ref tickets }
                if recent == "SWM-2" && tickets == &["SWM-1", "SWM-2"]
        ));
        assert_eq!(
            derive(&events, 100, 50, None, &keys()).ticket(),
            Some("SWM-2")
        );
        assert_eq!(
            derive(&[event(1, "", "")], 1, 10, None, &keys()),
            Focus::None {
                reason: "no_worktree"
            }
        );
        assert_eq!(
            derive(&[event(1, "/tmp/swm-9", "")], 1, 10, None, &keys()),
            Focus::None {
                reason: "no_worktree"
            }
        );
        assert_eq!(
            derive(&[event(1, "/tmp", "main")], 1, 10, None, &keys()),
            Focus::None {
                reason: "no_ticket"
            }
        );
    }

    #[trace("FR-037-AC-2")]
    #[test]
    fn a_floor_clips_the_window_to_events_after_an_order() {
        let events = [event(10, "/w", "swm-999-x"), event(200, "/w", "swm-211-y")];
        assert!(matches!(
            derive(&events, 300, 600_000, None, &keys()),
            Focus::Ambiguous { .. }
        ));
        assert_eq!(
            derive(&events, 300, 600_000, Some(100), &keys()).ticket(),
            Some("SWM-211")
        );
    }
}
