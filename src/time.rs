// Copyright  (C) 2020, Hove and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
// the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
// powered by Hove (www.kisio.com).
// Help us simplify mobility and open public transport:
// a non ending quest to the responsive locomotion way of traveling!
//
// This contribution is a part of the research and development work of the
// IVA Project which aims to enhance traveler information and is carried out
// under the leadership of the Technological Research Institute SystemX,
// with the partnership and support of the transport organization authority
// Ile-De-France Mobilités (IDFM), SNCF, and public funds
// under the scope of the French Program "Investissements d’Avenir".
//
// LICENCE: This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.
//
// Stay tuned using
// twitter @navitia
// channel `#navitia` on riot https://riot.im/app/#/room/#navitia:matrix.org
// https://groups.google.com/d/forum/navitia
// www.navitia.io

use chrono::{FixedOffset, NaiveDate};
use std::{
    fmt::{Debug, Display, Formatter},
    num::TryFromIntError,
};

pub mod calendar;
pub mod days_map;
pub mod days_patterns;
pub mod timezones_patterns;
pub use timezones_patterns::TimezonesPatterns;

/// Duration since "noon minus 12 hours" on a day in a specific timezone
/// This corresponds to the "Time" notion found in gtfs/ntfs stop_times.txt
/// It should be built from a TransitModelTime.
/// This types accept only times are comprised between -48:00:00 and 48:00:00 (maximum plus/minus 2 days)
#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct SecondsSinceTimezonedDayStart {
    seconds: i32,
}

#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct SecondsSinceUTCDayStart {
    seconds: i32,
}

const SECONDS_IN_A_DAY: i32 = 24 * 60 * 60; // 24h

const MAX_SECONDS_IN_TIMEZONED_DAY: i32 = 2 * SECONDS_IN_A_DAY; // 48h

const MAX_TIMEZONE_OFFSET: i32 = SECONDS_IN_A_DAY; // 24h

pub const MAX_SECONDS_IN_UTC_DAY: i32 = MAX_SECONDS_IN_TIMEZONED_DAY + MAX_TIMEZONE_OFFSET; // 72h

/// Duration since 00:00:00 UTC in the first allowed day of minus MAX_SECONDS_IN_UTC_DAY
/// This is used in the engine to store a point in time in an unambiguous way
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct SecondsSinceDatasetUTCStart {
    seconds: u32,
}

/// Number of days since the first allowed day of the data
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct DaysSinceDatasetStart {
    pub(super) days: u16,
}

// we allow 36_600 days which is more than 100 years, and less than u16::MAX = 65_535 days
const MAX_DAYS_IN_CALENDAR: u16 = 100 * 366;

#[derive(Debug)]
pub struct Calendar {
    first_date: NaiveDate, //first date which may be allowed
    last_date: NaiveDate,  //last date (included) which may be allowed
    last_day_offset: u16,  // == (last_date - first_date).num_of_days()
                           // we allow at most MAX_DAYS_IN_CALENDAR days
}

#[derive(Eq, PartialEq, Clone, Copy, Ord, PartialOrd)]
pub struct PositiveDuration {
    pub(super) seconds: u32,
}

#[derive(Debug)]
pub enum PositiveDurationError {
    ParseIntError(std::num::ParseIntError),
    IncorrectFormat(String),
}
impl std::convert::From<std::num::ParseIntError> for PositiveDurationError {
    fn from(parse_int_error: std::num::ParseIntError) -> Self {
        PositiveDurationError::ParseIntError(parse_int_error)
    }
}
impl std::fmt::Display for PositiveDurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use PositiveDurationError::*;
        match self {
            ParseIntError(parse_int_error) => write!(f, "{}", parse_int_error),
            IncorrectFormat(incorrect_format) => {
                write!(
                    f,
                    "Unable to parse {} as a duration. Expected format is 14:35:12",
                    incorrect_format
                )
            }
        }
    }
}

impl std::error::Error for PositiveDurationError {}

impl std::str::FromStr for PositiveDuration {
    type Err = PositiveDurationError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut t = s.split(':');
        let (hours, minutes, seconds) = match (t.next(), t.next(), t.next(), t.next()) {
            (Some(h), Some(m), Some(s), None) => (h, m, s),
            _ => {
                return Err(PositiveDurationError::IncorrectFormat(s.to_owned()));
            }
        };
        let hours: u32 = hours.parse()?;
        let minutes: u32 = minutes.parse()?;
        let seconds: u32 = seconds.parse()?;
        if minutes > 59 || seconds > 59 {
            return Err(PositiveDurationError::IncorrectFormat(s.to_owned()));
        }
        Ok(PositiveDuration::from_hms(hours, minutes, seconds))
    }
}

impl TryFrom<i32> for PositiveDuration {
    type Error = TryFromIntError;

    fn try_from(seconds: i32) -> Result<Self, Self::Error> {
        let seconds_u32 = u32::try_from(seconds)?;
        let result = PositiveDuration {
            seconds: seconds_u32,
        };
        Ok(result)
    }
}

impl<'de> serde::Deserialize<'de> for PositiveDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use std::str::FromStr;
        let s = String::deserialize(deserializer)?;

        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for PositiveDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        serializer.serialize_str(&self.to_hms_string())
    }
}

impl PositiveDuration {
    pub fn zero() -> Self {
        Self { seconds: 0 }
    }

    pub const fn from_hms(hours: u32, minutes: u32, seconds: u32) -> PositiveDuration {
        let total_seconds = seconds + 60 * minutes + 60 * 60 * hours;
        PositiveDuration {
            seconds: total_seconds,
        }
    }

    pub fn total_seconds(&self) -> u64 {
        u64::from(self.seconds)
    }

    pub fn total_seconds_u32(&self) -> u32 {
        self.seconds
    }

    pub fn to_hms_string(&self) -> String {
        let hours = self.seconds / (60 * 60);
        let minutes_in_secs = self.seconds % (60 * 60);
        let minutes = minutes_in_secs / 60;
        let seconds = minutes_in_secs % 60;
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
}

impl Display for PositiveDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let hours = self.seconds / (60 * 60);
        let minutes_in_secs = self.seconds % (60 * 60);
        let minutes = minutes_in_secs / 60;
        let seconds = minutes_in_secs % 60;
        if hours != 0 {
            write!(f, "{}h{:02}m{:02}s", hours, minutes, seconds)
        } else if minutes != 0 {
            write!(f, "{}m{:02}s", minutes, seconds)
        } else {
            write!(f, "{}s", seconds)
        }
    }
}

impl Debug for PositiveDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl SecondsSinceTimezonedDayStart {
    pub fn zero() -> Self {
        Self { seconds: 0 }
    }

    pub fn max() -> Self {
        Self {
            seconds: MAX_SECONDS_IN_TIMEZONED_DAY,
        }
    }

    pub fn min() -> Self {
        Self {
            seconds: -MAX_SECONDS_IN_TIMEZONED_DAY,
        }
    }

    pub fn total_seconds(&self) -> i32 {
        self.seconds
    }

    pub fn to_utc(&self, offset: &FixedOffset) -> SecondsSinceUTCDayStart {
        SecondsSinceUTCDayStart {
            seconds: self.seconds + offset.utc_minus_local(),
        }
    }

    pub fn from_seconds(seconds: i32) -> Option<Self> {
        if (-MAX_SECONDS_IN_TIMEZONED_DAY..=MAX_SECONDS_IN_TIMEZONED_DAY).contains(&seconds) {
            let result = Self { seconds };
            Some(result)
        } else {
            None
        }
    }

    pub fn from_seconds_i64(seconds_i64: i64) -> Option<Self> {
        let max_i64 = i64::from(MAX_SECONDS_IN_TIMEZONED_DAY);
        if seconds_i64 > max_i64 || seconds_i64 < -max_i64 {
            None
        } else {
            // since  :
            //  - seconds_i64 belongs to [-MAX_SECONDS_SINCE_TIMEZONED_DAY_START, MAX_SECONDS_SINCE_TIMEZONED_DAY_START]
            //  - MAX_SECONDS_SINCE_TIMEZONED_DAY_START <= i32::MAX
            // we can safely cas seconds_i64 to i32
            let seconds_i32 = seconds_i64 as i32;
            let result = Self {
                seconds: seconds_i32,
            };
            Some(result)
        }
    }
}

impl SecondsSinceUTCDayStart {
    pub fn from_seconds_i64(seconds_i64: i64) -> Option<Self> {
        let max_i64 = i64::from(MAX_SECONDS_IN_UTC_DAY);
        if seconds_i64 > max_i64 || seconds_i64 < -max_i64 {
            None
        } else {
            // since  :
            //  - seconds_i64 belongs to [-MAX_SECONDS_IN_UTC_DAY, MAX_SECONDS_IN_UTC_DAY]
            //  - MAX_SECONDS_IN_UTC_DAY <= i32::MAX
            // we can safely cas seconds_i64 to i32
            let seconds_i32 = seconds_i64 as i32;
            let result = Self {
                seconds: seconds_i32,
            };
            Some(result)
        }
    }

    fn new_unchecked(seconds_i32: i32) -> Self {
        debug_assert!(seconds_i32 >= -MAX_SECONDS_IN_UTC_DAY);
        debug_assert!(seconds_i32 <= MAX_SECONDS_IN_UTC_DAY);
        Self {
            seconds: seconds_i32,
        }
    }

    pub fn max() -> Self {
        Self {
            seconds: MAX_SECONDS_IN_UTC_DAY,
        }
    }

    pub fn min() -> Self {
        Self {
            seconds: -MAX_SECONDS_IN_UTC_DAY,
        }
    }
}

impl Display for SecondsSinceTimezonedDayStart {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (sign, abs) = if self.seconds < 0 {
            ("-", -self.seconds)
        } else {
            ("", self.seconds)
        };
        write!(
            f,
            "{}{:02}:{:02}:{:02}_tz",
            sign,
            abs / 60 / 60,
            abs / 60 % 60,
            abs % 60
        )
    }
}

impl Debug for SecondsSinceTimezonedDayStart {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl SecondsSinceDatasetUTCStart {
    pub fn duration_since(
        &self,
        start_datetime: &SecondsSinceDatasetUTCStart,
    ) -> Option<PositiveDuration> {
        self.seconds
            .checked_sub(start_datetime.seconds)
            .map(|seconds| PositiveDuration { seconds })
    }
}

impl Display for SecondsSinceUTCDayStart {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (sign, abs) = if self.seconds < 0 {
            ("-", -self.seconds)
        } else {
            ("", self.seconds)
        };
        write!(
            f,
            "{}{:02}:{:02}:{:02}_utc",
            sign,
            abs / 60 / 60,
            abs / 60 % 60,
            abs % 60
        )
    }
}

impl Debug for SecondsSinceUTCDayStart {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl std::ops::Add for PositiveDuration {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            seconds: self.seconds + rhs.seconds,
        }
    }
}

impl std::ops::Add<PositiveDuration> for SecondsSinceDatasetUTCStart {
    type Output = Self;

    fn add(self, rhs: PositiveDuration) -> Self::Output {
        Self {
            seconds: self.seconds + rhs.seconds,
        }
    }
}

impl std::ops::Sub<PositiveDuration> for SecondsSinceDatasetUTCStart {
    type Output = Self;

    fn sub(self, rhs: PositiveDuration) -> Self::Output {
        Self {
            seconds: self.seconds - rhs.seconds,
        }
    }
}

impl std::ops::Mul<u32> for PositiveDuration {
    type Output = Self;

    fn mul(self, rhs: u32) -> Self::Output {
        PositiveDuration {
            seconds: self.seconds * rhs,
        }
    }
}

impl std::ops::Mul<u16> for PositiveDuration {
    type Output = Self;

    fn mul(self, rhs: u16) -> Self::Output {
        PositiveDuration {
            seconds: self.seconds * u32::from(rhs),
        }
    }
}

impl std::ops::Mul<PositiveDuration> for u32 {
    type Output = PositiveDuration;

    fn mul(self, rhs: PositiveDuration) -> Self::Output {
        PositiveDuration {
            seconds: self * rhs.seconds,
        }
    }
}

impl std::ops::Mul<PositiveDuration> for u16 {
    type Output = PositiveDuration;

    fn mul(self, rhs: PositiveDuration) -> Self::Output {
        PositiveDuration {
            seconds: u32::from(self) * rhs.seconds,
        }
    }
}
