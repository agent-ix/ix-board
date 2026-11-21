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

/// An optional share of a bounded read's deadline for work that runs after the read.
///
/// Evaluation loops call [`Budget::check`] cooperatively, so they stop with
/// [`Error::ReadTimeout`] once the one operation-wide deadline has expired. The
/// unbounded budget never expires, which keeps every existing caller unchanged.
#[derive(Clone, Copy)]
pub(crate) struct Budget<'d>(Option<&'d Deadline<'d>>);

impl<'d> Budget<'d> {
    /// No bound: [`Budget::check`] always succeeds.
    pub(crate) const fn unbounded() -> Self {
        Self(None)
    }

    /// Shares `deadline`, the same one the read used, rather than starting another.
    pub(crate) const fn within(deadline: &'d Deadline<'d>) -> Self {
        Self(Some(deadline))
    }

    /// Fails with [`Error::ReadTimeout`] once the deadline has expired.
    pub(crate) fn check(&self) -> Result<(), Error> {
        match self.0 {
            Some(deadline) => deadline.remaining().map(|_| ()),
            None => Ok(()),
        }
    }
}

/// How many decoded bytes pass between deadline checks.
const DECODE_CHECK_INTERVAL: usize = 16 * 1024;

/// Serves a slice and fails with a timed-out error once the deadline has expired,
/// checking it every [`DECODE_CHECK_INTERVAL`] bytes so a large payload is bounded.
struct BudgetedSlice<'a, 'd> {
    bytes: &'a [u8],
    since_check: usize,
    budget: Budget<'d>,
}

impl std::io::Read for BudgetedSlice<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.since_check >= DECODE_CHECK_INTERVAL {
            self.since_check = 0;
            self.budget
                .check()
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        }
        let served = std::io::Read::read(&mut self.bytes, buf)?;
        self.since_check += served;
        Ok(served)
    }
}

/// Decodes a cached payload without outrunning the deadline: it is checked before the
/// decode, every [`DECODE_CHECK_INTERVAL`] bytes during it and after it.
pub(crate) fn decode_within<T: serde::de::DeserializeOwned>(
    payload: &[u8],
    deadline: &Deadline<'_>,
) -> Result<T, Error> {
    deadline.remaining()?;
    let reader = BudgetedSlice {
        bytes: payload,
        since_check: 0,
        budget: Budget::within(deadline),
    };
    let decoded = serde_json::from_reader(reader).map_err(|error| {
        if error.io_error_kind() == Some(std::io::ErrorKind::TimedOut) {
            Error::ReadTimeout {
                deadline: deadline.limit(),
            }
        } else {
            Error::CacheDecode(error)
        }
    })?;
    deadline.remaining()?;
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SystemClock;
    use ix_trace_rs::trace;

    #[trace("FR-004-AC-12")]
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

    #[trace("FR-004-AC-14")]
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
