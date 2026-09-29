//! When a discount rule runs: days of the week and a time window, both in
//! the shop's local time (a "happy hour" 16:00–18:00 on weekdays, a Friday
//! offer, a late-night window that crosses midnight).

use chrono::{Datelike, NaiveDateTime, Timelike};

/// Every day (Monday = bit 0 … Sunday = bit 6).
pub const ALL_DAYS: i64 = 0b111_1111;
pub const MINUTES_PER_DAY: i64 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schedule {
    /// `None` = every day.
    pub days_mask: Option<i64>,
    /// Minutes after local midnight; `None` = from the start of the day.
    pub time_from: Option<i64>,
    /// Minutes after local midnight, exclusive; `None` = to the end of the
    /// day. Earlier than `time_from` = the window crosses midnight.
    pub time_to: Option<i64>,
}

impl Schedule {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.days_mask.is_some_and(|m| !(1..=ALL_DAYS).contains(&m)) {
            return Err("pick at least one day");
        }
        if self
            .time_from
            .is_some_and(|m| !(0..MINUTES_PER_DAY).contains(&m))
        {
            return Err("the start time is not a time of day");
        }
        if self
            .time_to
            .is_some_and(|m| !(1..=MINUTES_PER_DAY).contains(&m))
        {
            return Err("the end time is not a time of day");
        }
        if self.time_from.is_some() && self.time_from == self.time_to {
            return Err("the start and end times are the same");
        }
        Ok(())
    }

    /// Whether the schedule is on at this local date and time. A window
    /// that crosses midnight belongs to the day it starts on.
    pub fn allows(&self, local: NaiveDateTime) -> bool {
        let minute = i64::from(local.hour() * 60 + local.minute());
        let from = self.time_from.unwrap_or(0);
        let to = self.time_to.unwrap_or(MINUTES_PER_DAY);
        let day = i64::from(local.weekday().num_days_from_monday());
        let on = |day: i64| self.days_mask.unwrap_or(ALL_DAYS) & (1 << day) != 0;
        if from < to {
            on(day) && (from..to).contains(&minute)
        } else if minute >= from {
            on(day)
        } else {
            minute < to && on((day + 6) % 7)
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        // 2026-09-28 is a Monday.
        NaiveDate::from_ymd_opt(2026, 9, 28)
            .and_then(|d| d.checked_add_days(chrono::Days::new(u64::from(day))))
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .expect("date")
    }

    #[test]
    fn always_on_by_default() {
        assert!(Schedule::default().allows(at(3, 3, 0)));
        assert_eq!(Schedule::default().validate(), Ok(()));
    }

    #[test]
    fn happy_hour_on_weekdays() {
        let s = Schedule {
            days_mask: Some(0b001_1111),
            time_from: Some(16 * 60),
            time_to: Some(18 * 60),
        };
        assert!(s.allows(at(0, 16, 0)));
        assert!(s.allows(at(4, 17, 59)));
        assert!(!s.allows(at(0, 18, 0)), "the end is exclusive");
        assert!(!s.allows(at(0, 15, 59)));
        assert!(!s.allows(at(5, 17, 0)), "not on Saturday");
    }

    #[test]
    fn a_window_across_midnight_belongs_to_its_first_day() {
        // Friday nights 22:00–02:00.
        let s = Schedule {
            days_mask: Some(1 << 4),
            time_from: Some(22 * 60),
            time_to: Some(2 * 60),
        };
        assert!(s.allows(at(4, 23, 0)));
        assert!(
            s.allows(at(5, 1, 30)),
            "Saturday 01:30 is still Friday night"
        );
        assert!(!s.allows(at(5, 2, 0)));
        assert!(!s.allows(at(3, 23, 0)), "Thursday night is not");
        assert!(
            !s.allows(at(4, 1, 0)),
            "Friday 01:00 belongs to Thursday night"
        );
    }

    #[test]
    fn bad_schedules_are_refused() {
        let bad = [
            Schedule {
                days_mask: Some(0),
                ..Schedule::default()
            },
            Schedule {
                days_mask: Some(128),
                ..Schedule::default()
            },
            Schedule {
                time_from: Some(1440),
                ..Schedule::default()
            },
            Schedule {
                time_to: Some(0),
                ..Schedule::default()
            },
            Schedule {
                time_from: Some(60),
                time_to: Some(60),
                ..Schedule::default()
            },
        ];
        for s in bad {
            assert!(s.validate().is_err(), "{s:?}");
        }
    }
}
