//! Typed, fixed inputs supplied to a simulation run.

use jiff::civil::Date;
use serde::{Deserialize, Deserializer, Serialize};

/// A calendar age. Months must be in `0..=11` when a run starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAge {
    pub years: u8,
    pub months: u8,
}

impl CalendarAge {
    #[must_use]
    pub const fn years(years: u8) -> Self {
        Self { years, months: 0 }
    }

    #[must_use]
    pub const fn new(years: u8, months: u8) -> Self {
        Self { years, months }
    }
}

/// Money and rate inputs participate in amount expressions. Date and age inputs
/// are used by event triggers and recurring schedule conditions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ParameterValue {
    Money(f64),
    Rate(f64),
    Date(Date),
    Age(CalendarAge),
}

impl ParameterValue {
    #[must_use]
    pub fn is_valid(self) -> bool {
        match self {
            Self::Money(value) | Self::Rate(value) => value.is_finite(),
            Self::Date(_) => true,
            Self::Age(age) => age.months < 12,
        }
    }
}

impl From<f64> for ParameterValue {
    fn from(value: f64) -> Self {
        Self::Money(value)
    }
}

impl<'de> Deserialize<'de> for ParameterValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        enum Tagged {
            Money(f64),
            Rate(f64),
            Date(Date),
            Age(CalendarAge),
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Compatible {
            Tagged(Tagged),
            LegacyNumber(f64),
        }
        Ok(match Compatible::deserialize(deserializer)? {
            Compatible::Tagged(Tagged::Money(value)) | Compatible::LegacyNumber(value) => {
                Self::Money(value)
            }
            Compatible::Tagged(Tagged::Rate(value)) => Self::Rate(value),
            Compatible::Tagged(Tagged::Date(value)) => Self::Date(value),
            Compatible::Tagged(Tagged::Age(value)) => Self::Age(value),
        })
    }
}
