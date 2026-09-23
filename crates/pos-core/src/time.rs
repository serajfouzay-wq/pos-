//! Timestamps in the one wire/storage format the whole system uses:
//! UTC, millisecond precision, `Z` suffix — `2026-09-23T10:15:30.123Z`.
//!
//! Fixed width means lexical order equals chronological order, which
//! last-write-wins conflict resolution relies on (see `TimestampSchema`).

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Duration, SubsecRound, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    pub fn from_datetime(value: DateTime<Utc>) -> Self {
        Self(value.trunc_subsecs(3))
    }

    pub fn from_unix_seconds(seconds: i64) -> Option<Self> {
        Utc.timestamp_opt(seconds, 0).single().map(Self)
    }

    pub fn unix_seconds(self) -> i64 {
        self.0.timestamp()
    }

    pub fn datetime(self) -> DateTime<Utc> {
        self.0
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0.checked_add_signed(duration).map(Self)
    }

    pub fn signed_duration_since(self, earlier: Timestamp) -> Duration {
        self.0.signed_duration_since(earlier.0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format(FORMAT))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected a UTC timestamp like 2026-09-23T10:15:30.123Z, got {0:?}")]
pub struct TimestampParseError(String);

impl FromStr for Timestamp {
    type Err = TimestampParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Exact format only: `YYYY-MM-DDTHH:MM:SS.mmmZ` (chrono alone would
        // also accept a space separator or other offsets).
        let b = s.as_bytes();
        if b.len() != 24 || b[10] != b'T' || b[19] != b'.' || b[23] != b'Z' {
            return Err(TimestampParseError(s.to_owned()));
        }
        DateTime::parse_from_rfc3339(s)
            .map(|dt| Self(dt.with_timezone(&Utc)))
            .map_err(|_| TimestampParseError(s.to_owned()))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Source of "now", injectable so time-dependent rules (license expiry,
/// offline grace) are testable.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_datetime(Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_fixed_format() {
        let ts: Timestamp = "2026-09-23T10:15:30.123Z".parse().expect("valid");
        assert_eq!(ts.to_string(), "2026-09-23T10:15:30.123Z");
        let whole = Timestamp::from_unix_seconds(0).expect("epoch");
        assert_eq!(whole.to_string(), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn rejects_other_shapes() {
        for bad in [
            "2026-09-23T10:15:30Z",
            "2026-09-23T10:15:30.123+03:00",
            "2026-09-23 10:15:30.123Z",
        ] {
            assert!(bad.parse::<Timestamp>().is_err(), "{bad}");
        }
    }
}
