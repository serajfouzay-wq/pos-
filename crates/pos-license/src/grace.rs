//! Offline grace period.
//!
//! Cloud validation only ever *moves `last_seen_at` forward*; it is not
//! required for every launch. A device may trade offline for
//! [`GRACE_DAYS`] after the later of token issuance and the last successful
//! cloud check. Anchoring on `iat` means wiping the local database does not
//! buy a fresh grace window.

use chrono::Duration;
use pos_core::time::Timestamp;

pub const GRACE_DAYS: i64 = 7;
const DAY_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grace {
    /// No cloud configured for this client: nothing to validate against.
    NotEnforced,
    Within {
        /// Whole days left, rounded up (7 right after a successful check).
        days_remaining: u32,
        deadline: Timestamp,
    },
    Exhausted {
        deadline: Timestamp,
    },
}

/// Wall clocks can be wound back to stretch the grace window. The till keeps
/// the highest time it has ever observed (reset to server time on every
/// successful cloud check) and never evaluates against anything earlier.
pub fn effective_now(wall_clock: Timestamp, high_water: Option<Timestamp>) -> Timestamp {
    high_water.map_or(wall_clock, |hw| hw.max(wall_clock))
}

pub fn evaluate(
    cloud_enabled: bool,
    issued_at: Timestamp,
    last_seen_at: Option<Timestamp>,
    now: Timestamp,
) -> Grace {
    if !cloud_enabled {
        return Grace::NotEnforced;
    }
    let anchor = last_seen_at.map_or(issued_at, |seen| seen.max(issued_at));
    let deadline = anchor
        .checked_add(Duration::days(GRACE_DAYS))
        .unwrap_or(anchor);
    if now >= deadline {
        return Grace::Exhausted { deadline };
    }
    let seconds_left = deadline.signed_duration_since(now).num_seconds();
    let days = (seconds_left + DAY_SECONDS - 1) / DAY_SECONDS;
    Grace::Within {
        days_remaining: u32::try_from(days).unwrap_or(u32::MAX),
        deadline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().expect("timestamp")
    }

    #[test]
    fn not_enforced_without_cloud() {
        let issued = at("2026-01-01T00:00:00.000Z");
        assert_eq!(
            evaluate(false, issued, None, at("2030-01-01T00:00:00.000Z")),
            Grace::NotEnforced
        );
    }

    #[test]
    fn counts_down_from_the_latest_anchor() {
        let issued = at("2026-01-01T00:00:00.000Z");
        let seen = at("2026-01-05T12:00:00.000Z");
        let grace = evaluate(true, issued, Some(seen), at("2026-01-05T12:00:00.000Z"));
        assert!(matches!(
            grace,
            Grace::Within {
                days_remaining: 7,
                ..
            }
        ));
        let grace = evaluate(true, issued, Some(seen), at("2026-01-12T11:00:00.000Z"));
        assert!(matches!(
            grace,
            Grace::Within {
                days_remaining: 1,
                ..
            }
        ));
        let grace = evaluate(true, issued, Some(seen), at("2026-01-12T12:00:00.000Z"));
        assert!(matches!(grace, Grace::Exhausted { .. }));
    }

    #[test]
    fn falls_back_to_issuance_when_never_seen_online() {
        let issued = at("2026-01-01T00:00:00.000Z");
        let grace = evaluate(true, issued, None, at("2026-01-08T00:00:00.001Z"));
        assert!(matches!(grace, Grace::Exhausted { .. }));
    }

    #[test]
    fn stale_last_seen_cannot_predate_issuance() {
        // A renewed token re-anchors even if the old last_seen was long ago.
        let issued = at("2026-03-01T00:00:00.000Z");
        let seen = at("2026-01-01T00:00:00.000Z");
        let grace = evaluate(true, issued, Some(seen), at("2026-03-02T00:00:00.000Z"));
        assert!(matches!(
            grace,
            Grace::Within {
                days_remaining: 6,
                ..
            }
        ));
    }

    #[test]
    fn winding_the_clock_back_does_not_help() {
        let high_water = at("2026-02-01T00:00:00.000Z");
        let wound_back = at("2026-01-02T00:00:00.000Z");
        assert_eq!(effective_now(wound_back, Some(high_water)), high_water);
    }
}
