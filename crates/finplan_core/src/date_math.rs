//! Fast date arithmetic helpers that bypass jiff's `Span` machinery.
//!
//! jiff `Span` operations (`Date - Date`, `Span::years()`, `Span::resign()`)
//! are correct but relatively heavy for a hot simulation loop. The helpers here
//! use Rata Die day-numbering to perform O(1) day-difference calculations and
//! direct calendar arithmetic for year/month offsets — no `Span` allocation or
//! normalisation involved.

use jiff::civil::Date;

/// Fast leap year check.
#[inline]
pub fn is_leap_year(year: i16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Fast inline days-in-month calculation without creating a `jiff::civil::Date`.
#[inline]
pub fn days_in_month(year: i16, month: i8) -> i8 {
    const DAYS: [i8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && is_leap_year(year) {
        29
    } else {
        DAYS[(month - 1) as usize]
    }
}

/// Convert a civil date to a Rata Die day number (days since 0001-01-01).
///
/// Uses the proleptic Gregorian calendar algorithm from Baum (2017).
/// This is an O(1) operation with no branches beyond the month adjustment.
#[inline]
fn rata_die(d: Date) -> i32 {
    let y = d.year() as i32;
    let m = d.month() as i32;
    let day = d.day() as i32;

    // Shift March = month 1 so Feb (end of "year") is month 12
    let a = (14 - m) / 12;
    let y2 = y - a;
    let m2 = m + 12 * a - 3;

    day + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 306
}

/// Compute the number of days between two dates (d2 - d1).
///
/// Positive when `d2 > d1`. This replaces `(d2 - d1).get_days()` which
/// creates an intermediate `jiff::Span` and calls the expensive `resign()`
/// normalisation path.
#[inline]
pub fn fast_days_between(d1: Date, d2: Date) -> i32 {
    rata_die(d2) - rata_die(d1)
}

/// Add `n` days to a date without going through `jiff::Span`.
///
/// Converts to Rata Die, adds, then converts back — O(1) with no
/// `Span` allocation.
#[inline]
pub fn add_days(d: Date, n: i32) -> Date {
    rd_to_date(rata_die(d) + n)
}

/// Convert a Rata Die day number back to a `jiff::civil::Date`.
///
/// Inverse of `rata_die()`, using the same proleptic Gregorian algorithm.
#[inline]
fn rd_to_date(rd: i32) -> Date {
    // Shift so day 0 = March 1, year 0
    let z = rd + 306;
    let h = 100 * z - 25;
    let a = h / 3_652_425;
    let b = a - a / 4;
    let y = (100 * b + h) / 36_525;
    let c = b + z - 365 * y - y / 4;
    let m = (5 * c + 456) / 153;
    let day = c - (153 * m - 457) / 5;

    let (year, month) = if m > 12 { (y + 1, m - 12) } else { (y, m) };

    jiff::civil::date(year as i16, month as i8, day as i8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn test_fast_days_between_same_date() {
        let d = date(2025, 6, 15);
        assert_eq!(fast_days_between(d, d), 0);
    }

    #[test]
    fn test_fast_days_between_one_day() {
        assert_eq!(fast_days_between(date(2025, 1, 1), date(2025, 1, 2)), 1);
        assert_eq!(fast_days_between(date(2025, 1, 2), date(2025, 1, 1)), -1);
    }

    #[test]
    fn test_fast_days_between_across_year() {
        // 2024 is a leap year → 366 days
        assert_eq!(fast_days_between(date(2024, 1, 1), date(2025, 1, 1)), 366);
        // 2025 is not a leap year → 365 days
        assert_eq!(fast_days_between(date(2025, 1, 1), date(2026, 1, 1)), 365);
    }

    #[test]
    fn test_fast_days_between_leap_feb() {
        assert_eq!(fast_days_between(date(2024, 2, 28), date(2024, 3, 1)), 2);
        assert_eq!(fast_days_between(date(2025, 2, 28), date(2025, 3, 1)), 1);
    }

    #[test]
    fn test_fast_days_between_matches_jiff() {
        let pairs = [
            (date(2020, 1, 1), date(2030, 6, 15)),
            (date(2024, 2, 29), date(2025, 2, 28)),
            (date(2000, 3, 1), date(2100, 3, 1)),
            (date(2025, 12, 31), date(2026, 1, 1)),
        ];
        for (d1, d2) in pairs {
            let jiff_days = (d2 - d1).get_days();
            let fast_days = fast_days_between(d1, d2);
            assert_eq!(
                fast_days, jiff_days,
                "mismatch for {d1} → {d2}: fast={fast_days}, jiff={jiff_days}"
            );
        }
    }

    #[test]
    fn test_add_days_basic() {
        assert_eq!(add_days(date(2025, 1, 1), 1), date(2025, 1, 2));
        assert_eq!(add_days(date(2025, 1, 31), 1), date(2025, 2, 1));
        assert_eq!(add_days(date(2025, 12, 31), 1), date(2026, 1, 1));
    }

    #[test]
    fn test_add_days_negative() {
        assert_eq!(add_days(date(2025, 1, 1), -1), date(2024, 12, 31));
    }

    #[test]
    fn test_add_days_leap_year() {
        assert_eq!(add_days(date(2024, 2, 28), 1), date(2024, 2, 29));
        assert_eq!(add_days(date(2024, 2, 29), 1), date(2024, 3, 1));
        assert_eq!(add_days(date(2025, 2, 28), 1), date(2025, 3, 1));
    }

    #[test]
    fn test_roundtrip() {
        let dates = [
            date(2000, 1, 1),
            date(2024, 2, 29),
            date(2025, 6, 15),
            date(2099, 12, 31),
        ];
        for d in dates {
            let rd = rata_die(d);
            let back = rd_to_date(rd);
            assert_eq!(d, back, "roundtrip failed for {d}");
        }
    }
}
