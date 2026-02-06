//! Required Minimum Distribution (RMD) tables and calculations
//!
//! The IRS requires minimum withdrawals from tax-deferred accounts
//! starting at age 73 (as of 2024).

use serde::{Deserialize, Serialize};

/// IRS Uniform Lifetime Table for calculating Required Minimum Distributions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RmdTable {
    pub entries: Vec<RmdTableEntry>,
}

/// Single entry in the RMD table mapping age to IRS divisor
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RmdTableEntry {
    pub age: u8,
    pub divisor: f64,
}

impl RmdTable {
    /// IRS Uniform Lifetime Table (2024)
    #[must_use]
    pub fn irs_uniform_lifetime_2024() -> Self {
        RmdTable {
            entries: vec![
                RmdTableEntry {
                    age: 73,
                    divisor: 26.5,
                },
                RmdTableEntry {
                    age: 74,
                    divisor: 25.5,
                },
                RmdTableEntry {
                    age: 75,
                    divisor: 24.6,
                },
                RmdTableEntry {
                    age: 76,
                    divisor: 23.7,
                },
                RmdTableEntry {
                    age: 77,
                    divisor: 22.9,
                },
                RmdTableEntry {
                    age: 78,
                    divisor: 22.0,
                },
                RmdTableEntry {
                    age: 79,
                    divisor: 21.1,
                },
                RmdTableEntry {
                    age: 80,
                    divisor: 20.2,
                },
                RmdTableEntry {
                    age: 81,
                    divisor: 19.4,
                },
                RmdTableEntry {
                    age: 82,
                    divisor: 18.5,
                },
                RmdTableEntry {
                    age: 83,
                    divisor: 17.7,
                },
                RmdTableEntry {
                    age: 84,
                    divisor: 16.8,
                },
                RmdTableEntry {
                    age: 85,
                    divisor: 16.0,
                },
                RmdTableEntry {
                    age: 86,
                    divisor: 15.2,
                },
                RmdTableEntry {
                    age: 87,
                    divisor: 14.4,
                },
                RmdTableEntry {
                    age: 88,
                    divisor: 13.7,
                },
                RmdTableEntry {
                    age: 89,
                    divisor: 12.9,
                },
                RmdTableEntry {
                    age: 90,
                    divisor: 12.2,
                },
                RmdTableEntry {
                    age: 91,
                    divisor: 11.5,
                },
                RmdTableEntry {
                    age: 92,
                    divisor: 10.8,
                },
                RmdTableEntry {
                    age: 93,
                    divisor: 10.1,
                },
                RmdTableEntry {
                    age: 94,
                    divisor: 9.5,
                },
                RmdTableEntry {
                    age: 95,
                    divisor: 8.9,
                },
                RmdTableEntry {
                    age: 96,
                    divisor: 8.4,
                },
                RmdTableEntry {
                    age: 97,
                    divisor: 7.8,
                },
                RmdTableEntry {
                    age: 98,
                    divisor: 7.3,
                },
                RmdTableEntry {
                    age: 99,
                    divisor: 6.8,
                },
                RmdTableEntry {
                    age: 100,
                    divisor: 6.4,
                },
                RmdTableEntry {
                    age: 101,
                    divisor: 6.0,
                },
                RmdTableEntry {
                    age: 102,
                    divisor: 5.6,
                },
                RmdTableEntry {
                    age: 103,
                    divisor: 5.2,
                },
                RmdTableEntry {
                    age: 104,
                    divisor: 4.9,
                },
                RmdTableEntry {
                    age: 105,
                    divisor: 4.6,
                },
                RmdTableEntry {
                    age: 106,
                    divisor: 4.3,
                },
                RmdTableEntry {
                    age: 107,
                    divisor: 4.1,
                },
                RmdTableEntry {
                    age: 108,
                    divisor: 3.9,
                },
                RmdTableEntry {
                    age: 109,
                    divisor: 3.7,
                },
                RmdTableEntry {
                    age: 110,
                    divisor: 3.5,
                },
                RmdTableEntry {
                    age: 111,
                    divisor: 3.4,
                },
                RmdTableEntry {
                    age: 112,
                    divisor: 3.3,
                },
                RmdTableEntry {
                    age: 113,
                    divisor: 3.1,
                },
                RmdTableEntry {
                    age: 114,
                    divisor: 3.0,
                },
                RmdTableEntry {
                    age: 115,
                    divisor: 2.9,
                },
                RmdTableEntry {
                    age: 116,
                    divisor: 2.8,
                },
                RmdTableEntry {
                    age: 117,
                    divisor: 2.7,
                },
                RmdTableEntry {
                    age: 118,
                    divisor: 2.5,
                },
                RmdTableEntry {
                    age: 119,
                    divisor: 2.3,
                },
                RmdTableEntry {
                    age: 120,
                    divisor: 2.0,
                },
            ],
        }
    }

    /// Get divisor for a specific age
    #[must_use]
    pub fn divisor_for_age(&self, age: u8) -> Option<f64> {
        self.entries
            .iter()
            .find(|e| e.age == age)
            .map(|e| e.divisor)
    }
}
