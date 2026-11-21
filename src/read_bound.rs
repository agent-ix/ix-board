// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Opt-in policy and operation-wide deadline for bounded workspace reads.

use std::time::{Duration, Instant};

use crate::{CACHE_TTL_SECONDS, Clock, Error};

/// Caller-chosen limits for one bounded read.
///
/// Legacy read methods take no policy and keep their five-minute entry lifetime,
/// 30-second per-request timeout and absence of a deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadPolicy {
    /// Oldest acceptable cached data, measured from when its source read began.
    /// It may be at most the five-minute entry lifetime.
    pub max_age: Duration,
    /// Bound on the whole operation, from the moment the bounded read begins.
    /// It must be greater than zero.
    pub deadline: Duration,
}

impl ReadPolicy {
    /// Checks the policy without reading anything.
    pub fn validate(&self) -> Result<(), Error> {
        let lifetime = Duration::from_secs(CACHE_TTL_SECONDS.unsigned_abs());
        if self.max_age > lifetime {
            return Err(Error::InvalidReadPolicy {
                reason: "max_age exceeds the five-minute cache entry lifetime",
            });
        }
        if self.deadline.is_zero() {
            return Err(Error::InvalidReadPolicy {
                reason: "deadline must be greater than zero",
            });
        }
        Ok(())
    }
}

/// The operation-wide deadline handed to a source that honors a bound.
///
/// A source calls [`Deadline::request_timeout`] before every request, including
/// each page and each nested read, and uses the returned value as that request's
/// timeout. Once the deadline has expired no request may start.
pub struct Deadline<'a> {
    clock: &'a dyn Clock,
    start: Instant,
    limit: Duration,
}

impl<'a> Deadline<'a> {
    /// Starts a deadline of `limit` measured by `clock` from now.
    pub(crate) fn start(clock: &'a dyn Clock, limit: Duration) -> Self {
        Self {
            clock,
            start: clock.monotonic_now(),
            limit,
        }
    }

    /// The bound this deadline was started with.
    pub fn limit(&self) -> Duration {
        self.limit
    }

    /// Time spent since the deadline started.
    pub fn elapsed(&self) -> Duration {
        self.clock
            .monotonic_now()
            .saturating_duration_since(self.start)
    }

    /// Time left, or [`Error::ReadTimeout`] once none is left.
    pub fn remaining(&self) -> Result<Duration, Error> {
        match self.limit.checked_sub(self.elapsed()) {
            Some(left) if !left.is_zero() => Ok(left),
            _ => Err(Error::ReadTimeout {
                deadline: self.limit,
            }),
        }
    }

    /// The timeout for one request: the lesser of its legacy `per_request` timeout and
    /// the time remaining. Fails with [`Error::ReadTimeout`] when nothing remains.
    pub fn request_timeout(&self, per_request: Duration) -> Result<Duration, Error> {
        Ok(per_request.min(self.remaining()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SystemClock;

    /// Trace: FR-004-AC-12
    #[test]
    fn policy_rejects_an_age_above_the_entry_lifetime_and_a_zero_deadline() {
        let valid = ReadPolicy {
            max_age: Duration::from_secs(300),
            deadline: Duration::from_secs(1),
        };
        assert!(valid.validate().is_ok());
        for invalid in [
            ReadPolicy {
                max_age: Duration::from_secs(301),
                ..valid
            },
            ReadPolicy {
                deadline: Duration::ZERO,
                ..valid
            },
        ] {
            assert!(matches!(
                invalid.validate(),
                Err(Error::InvalidReadPolicy { .. })
            ));
        }
    }

    /// Trace: FR-004-AC-14
    #[test]
    fn request_timeout_is_clamped_to_the_time_remaining() {
        let deadline = Deadline::start(&SystemClock, Duration::from_secs(5));
        let timeout = deadline
            .request_timeout(Duration::from_secs(30))
            .expect("time remains");
        assert!(timeout <= Duration::from_secs(5));
        let generous = Deadline::start(&SystemClock, Duration::from_secs(600));
        assert_eq!(
            generous
                .request_timeout(Duration::from_secs(30))
                .expect("time remains"),
            Duration::from_secs(30)
        );
    }
}
