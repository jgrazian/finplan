//! `reference_facts(topic, year)`: statutory figures as a deterministic table,
//! so the model quotes them instead of recalling them.
//!
//! Every fact carries a `status`: `published` for figures the IRS or SSA has
//! announced, `projected` where the year's figure was not yet final when the
//! table was written, and `approximate` for the state table. Years 2024 to 2026
//! are covered; a year outside is refused rather than extrapolated.

use std::ops::RangeInclusive;

use serde_json::{Value, json};

/// The years the tables cover.
pub const YEARS: RangeInclusive<i32> = 2024..=2026;
/// The year a call without `year` means.
pub const DEFAULT_YEAR: i32 = 2026;

/// A filing status, as the tables key it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilingStatus {
    Single,
    MarriedJoint,
    MarriedSeparate,
    HeadOfHousehold,
}

impl FilingStatus {
    pub fn parse(text: &str) -> Option<Self> {
        match text
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "single" => Some(Self::Single),
            "married_filing_jointly" | "married_joint" | "mfj" | "joint" => {
                Some(Self::MarriedJoint)
            }
            "married_filing_separately" | "married_separate" | "mfs" => Some(Self::MarriedSeparate),
            "head_of_household" | "hoh" => Some(Self::HeadOfHousehold),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::MarriedJoint => "married_filing_jointly",
            Self::MarriedSeparate => "married_filing_separately",
            Self::HeadOfHousehold => "head_of_household",
        }
    }

    const ALL: [Self; 4] = [
        Self::Single,
        Self::MarriedJoint,
        Self::MarriedSeparate,
        Self::HeadOfHousehold,
    ];
}

// ── contribution limits ─────────────────────────────────────────────────────

/// One year's retirement and health savings limits, in dollars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// 401(k), 403(b), governmental 457(b) and TSP employee deferral.
    pub deferral: f64,
    /// Extra deferral at 50 and over.
    pub catch_up_50: f64,
    /// Extra deferral at 60 to 63 (replaces the age-50 catch-up).
    pub catch_up_60_63: f64,
    /// Employee plus employer additions to one defined-contribution plan.
    pub annual_additions: f64,
    pub ira: f64,
    pub ira_catch_up_50: f64,
    pub hsa_self: f64,
    pub hsa_family: f64,
    pub hsa_catch_up_55: f64,
    pub status: &'static str,
    pub source: &'static str,
}

pub fn limits(year: i32) -> Option<Limits> {
    Some(match year {
        2024 => Limits {
            deferral: 23_000.0,
            catch_up_50: 7_500.0,
            // SECURE 2.0's age 60-63 amount starts in 2025; 2024 has none.
            catch_up_60_63: 7_500.0,
            annual_additions: 69_000.0,
            ira: 7_000.0,
            ira_catch_up_50: 1_000.0,
            hsa_self: 4_150.0,
            hsa_family: 8_300.0,
            hsa_catch_up_55: 1_000.0,
            status: "published",
            source: "IRS Notice 2023-75; Rev. Proc. 2023-23",
        },
        2025 => Limits {
            deferral: 23_500.0,
            catch_up_50: 7_500.0,
            catch_up_60_63: 11_250.0,
            annual_additions: 70_000.0,
            ira: 7_000.0,
            ira_catch_up_50: 1_000.0,
            hsa_self: 4_300.0,
            hsa_family: 8_550.0,
            hsa_catch_up_55: 1_000.0,
            status: "published",
            source: "IRS Notice 2024-80; Rev. Proc. 2024-25",
        },
        2026 => Limits {
            deferral: 24_500.0,
            catch_up_50: 8_000.0,
            catch_up_60_63: 11_250.0,
            annual_additions: 72_000.0,
            ira: 7_500.0,
            ira_catch_up_50: 1_100.0,
            hsa_self: 4_400.0,
            hsa_family: 8_750.0,
            hsa_catch_up_55: 1_000.0,
            status: "published",
            source: "IRS Notice 2025-67; Rev. Proc. 2025-19",
        },
        _ => return None,
    })
}

/// The employee 401(k) deferral limit for `year`; what guided setup caps a
/// contribution at.
pub fn employee_deferral_limit(year: i32) -> Option<f64> {
    limits(year).map(|l| l.deferral)
}

// ── standard deduction and brackets ─────────────────────────────────────────

/// The federal standard deduction.
pub fn standard_deduction(year: i32, status: FilingStatus) -> Option<f64> {
    let (single, joint, head) = match year {
        2024 => (14_600.0, 29_200.0, 21_900.0),
        // As amended by the July 2025 tax act.
        2025 => (15_750.0, 31_500.0, 23_625.0),
        2026 => (16_100.0, 32_200.0, 24_150.0),
        _ => return None,
    };
    Some(match status {
        FilingStatus::Single | FilingStatus::MarriedSeparate => single,
        FilingStatus::MarriedJoint => joint,
        FilingStatus::HeadOfHousehold => head,
    })
}

/// The extra standard deduction at 65 and over: (unmarried, married, per person).
pub fn age_65_extra(year: i32) -> Option<(f64, f64)> {
    match year {
        2024 => Some((1_950.0, 1_550.0)),
        2025 => Some((2_000.0, 1_600.0)),
        2026 => Some((2_050.0, 1_650.0)),
        _ => None,
    }
}

/// Federal ordinary-income brackets: (threshold of taxable income, rate),
/// ascending from zero.
pub fn federal_brackets(year: i32, status: FilingStatus) -> Option<Vec<(f64, f64)>> {
    const RATES: [f64; 7] = [0.10, 0.12, 0.22, 0.24, 0.32, 0.35, 0.37];
    // Upper edges of the first six brackets.
    let edges: [f64; 6] = match (year, status) {
        (2024, FilingStatus::Single) => [11_600., 47_150., 100_525., 191_950., 243_725., 609_350.],
        (2024, FilingStatus::MarriedSeparate) => {
            [11_600., 47_150., 100_525., 191_950., 243_725., 365_600.]
        }
        (2024, FilingStatus::MarriedJoint) => {
            [23_200., 94_300., 201_050., 383_900., 487_450., 731_200.]
        }
        (2024, FilingStatus::HeadOfHousehold) => {
            [16_550., 63_100., 100_500., 191_950., 243_700., 609_350.]
        }
        (2025, FilingStatus::Single) => [11_925., 48_475., 103_350., 197_300., 250_525., 626_350.],
        (2025, FilingStatus::MarriedSeparate) => {
            [11_925., 48_475., 103_350., 197_300., 250_525., 375_800.]
        }
        (2025, FilingStatus::MarriedJoint) => {
            [23_850., 96_950., 206_700., 394_600., 501_050., 751_600.]
        }
        (2025, FilingStatus::HeadOfHousehold) => {
            [17_000., 64_850., 103_350., 197_300., 250_500., 626_350.]
        }
        (2026, FilingStatus::Single) => [12_400., 50_400., 105_700., 201_775., 256_225., 640_600.],
        (2026, FilingStatus::MarriedSeparate) => {
            [12_400., 50_400., 105_700., 201_775., 256_225., 384_350.]
        }
        (2026, FilingStatus::MarriedJoint) => {
            [24_800., 100_800., 211_400., 403_550., 512_450., 768_700.]
        }
        (2026, FilingStatus::HeadOfHousehold) => {
            [17_700., 67_450., 105_700., 201_750., 256_200., 640_600.]
        }
        _ => return None,
    };
    let mut out = vec![(0.0, RATES[0])];
    out.extend(edges.iter().zip(&RATES[1..]).map(|(t, r)| (*t, *r)));
    Some(out)
}

/// `published` for the years whose brackets were final, `projected` for 2026.
pub fn bracket_status(year: i32) -> &'static str {
    if year >= 2026 {
        "projected"
    } else {
        "published"
    }
}

// ── Social Security ─────────────────────────────────────────────────────────

/// The national average wage index, published by SSA. Years after the last
/// entry are projected by the caller.
pub const AWI: &[(i32, f64)] = &[
    (1977, 9_779.44),
    (1980, 12_513.46),
    (1981, 13_773.10),
    (1982, 14_531.34),
    (1983, 15_239.24),
    (1984, 16_135.07),
    (1985, 16_822.51),
    (1986, 17_321.82),
    (1987, 18_426.51),
    (1988, 19_334.04),
    (1989, 20_099.55),
    (1990, 21_027.98),
    (1991, 21_811.60),
    (1992, 22_935.42),
    (1993, 23_132.67),
    (1994, 23_753.53),
    (1995, 24_705.66),
    (1996, 25_913.90),
    (1997, 27_426.00),
    (1998, 28_861.44),
    (1999, 30_469.84),
    (2000, 32_154.82),
    (2001, 32_921.92),
    (2002, 33_252.09),
    (2003, 34_064.95),
    (2004, 35_648.55),
    (2005, 36_952.94),
    (2006, 38_651.41),
    (2007, 40_405.48),
    (2008, 41_334.97),
    (2009, 40_711.61),
    (2010, 41_673.83),
    (2011, 42_979.61),
    (2012, 44_321.67),
    (2013, 44_888.16),
    (2014, 46_481.52),
    (2015, 48_098.63),
    (2016, 48_642.15),
    (2017, 50_321.89),
    (2018, 52_145.80),
    (2019, 54_099.99),
    (2020, 55_628.60),
    (2021, 60_575.07),
    (2022, 63_795.13),
    (2023, 66_621.80),
    (2024, 69_846.57),
];

/// Yearly growth assumed for wage-index years SSA has not published.
pub const PROJECTED_AWI_GROWTH: f64 = 0.035;

/// The wage index for `year`, and whether it is projected.
pub fn awi(year: i32) -> (f64, bool) {
    let (last_year, last) = AWI[AWI.len() - 1];
    if year <= last_year {
        let known = AWI.iter().find(|(y, _)| *y == year).map(|(_, v)| *v);
        // Years before 1977 or between table entries never occur for callers
        // that check `awi_covers`; fall back to the nearest earlier entry.
        let value = known.unwrap_or_else(|| {
            AWI.iter()
                .rev()
                .find(|(y, _)| *y <= year)
                .map_or(AWI[0].1, |(_, v)| *v)
        });
        (value, false)
    } else {
        (
            last * (1.0 + PROJECTED_AWI_GROWTH).powi(year - last_year),
            true,
        )
    }
}

/// Whether `year` is one the index table covers (1980 on).
pub fn awi_covers(year: i32) -> bool {
    year >= 1980
}

/// PIA bend points for people first eligible (age 62) in `eligibility_year`:
/// 180 and 1,085 dollars indexed by the wage index two years earlier, against
/// its 1977 level, rounded to the dollar as SSA does. Also reports whether the
/// index it used was projected.
pub fn bend_points(eligibility_year: i32) -> ((f64, f64), bool) {
    let (index, projected) = awi(eligibility_year - 2);
    let ratio = index / AWI[0].1;
    (
        ((180.0 * ratio).round(), (1_085.0 * ratio).round()),
        projected,
    )
}

/// The contribution and benefit base (taxable maximum), by year.
pub fn wage_base(year: i32) -> Option<f64> {
    Some(match year {
        1980 => 25_900.0,
        1981 => 29_700.0,
        1982 => 32_400.0,
        1983 => 35_700.0,
        1984 => 37_800.0,
        1985 => 39_600.0,
        1986 => 42_000.0,
        1987 => 43_800.0,
        1988 => 45_000.0,
        1989 => 48_000.0,
        1990 => 51_300.0,
        1991 => 53_400.0,
        1992 => 55_500.0,
        1993 => 57_600.0,
        1994 => 60_600.0,
        1995 => 61_200.0,
        1996 => 62_700.0,
        1997 => 65_400.0,
        1998 => 68_400.0,
        1999 => 72_600.0,
        2000 => 76_200.0,
        2001 => 80_400.0,
        2002 => 84_900.0,
        2003 => 87_000.0,
        2004 => 87_900.0,
        2005 => 90_000.0,
        2006 => 94_200.0,
        2007 => 97_500.0,
        2008 => 102_000.0,
        2009..=2011 => 106_800.0,
        2012 => 110_100.0,
        2013 => 113_700.0,
        2014 => 117_000.0,
        2015 | 2016 => 118_500.0,
        2017 => 127_200.0,
        2018 => 128_400.0,
        2019 => 132_900.0,
        2020 => 137_700.0,
        2021 => 142_800.0,
        2022 => 147_000.0,
        2023 => 160_200.0,
        2024 => 168_600.0,
        2025 => 176_100.0,
        2026 => 184_500.0,
        _ => return None,
    })
}

/// The wage base for any year: the table, else the last one grown with the
/// projected wage index (rounded to 300 as SSA does). The flag says projected.
pub fn wage_base_any(year: i32) -> (f64, bool) {
    match wage_base(year) {
        Some(v) => (v, false),
        None => {
            let ratio = awi(year - 2).0 / awi(2024).0;
            ((184_500.0 * ratio / 300.0).round() * 300.0, true)
        }
    }
}

/// Full retirement age for a birth year, as (years, months).
pub fn full_retirement_age(birth_year: i32) -> (u32, u32) {
    match birth_year {
        ..=1937 => (65, 0),
        1938 => (65, 2),
        1939 => (65, 4),
        1940 => (65, 6),
        1941 => (65, 8),
        1942 => (65, 10),
        1943..=1954 => (66, 0),
        1955 => (66, 2),
        1956 => (66, 4),
        1957 => (66, 6),
        1958 => (66, 8),
        1959 => (66, 10),
        _ => (67, 0),
    }
}

/// The age (in years, as a decimal) required minimum distributions start, and
/// the rule behind it, by birth year.
pub fn rmd_start(birth_year: i32) -> (f64, &'static str) {
    match birth_year {
        ..=1948 => (
            70.5,
            "born before July 1, 1949: age 70 and a half (born from July 1, 1949 through 1950: age 72)",
        ),
        1949 | 1950 => (
            72.0,
            "born 1949 through 1950: age 72 (age 70 and a half if born before July 1, 1949)",
        ),
        1951..=1959 => (73.0, "born 1951 through 1959: age 73"),
        _ => (75.0, "born 1960 or later: age 75"),
    }
}

// ── state income tax ────────────────────────────────────────────────────────

/// How a state taxes wages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    /// No tax on wages.
    None,
    Flat,
    Graduated,
}

/// (code, name, kind, top marginal or flat rate in percent). Approximations of
/// 2025 rates, before local taxes; brackets, credits and surtaxes are ignored.
pub const STATES: &[(&str, &str, StateKind, f64)] = &[
    ("AL", "Alabama", StateKind::Graduated, 5.0),
    ("AK", "Alaska", StateKind::None, 0.0),
    ("AZ", "Arizona", StateKind::Flat, 2.5),
    ("AR", "Arkansas", StateKind::Graduated, 3.9),
    ("CA", "California", StateKind::Graduated, 13.3),
    ("CO", "Colorado", StateKind::Flat, 4.4),
    ("CT", "Connecticut", StateKind::Graduated, 6.99),
    ("DE", "Delaware", StateKind::Graduated, 6.6),
    ("DC", "District of Columbia", StateKind::Graduated, 10.75),
    ("FL", "Florida", StateKind::None, 0.0),
    ("GA", "Georgia", StateKind::Flat, 5.19),
    ("HI", "Hawaii", StateKind::Graduated, 11.0),
    ("ID", "Idaho", StateKind::Flat, 5.3),
    ("IL", "Illinois", StateKind::Flat, 4.95),
    ("IN", "Indiana", StateKind::Flat, 3.0),
    ("IA", "Iowa", StateKind::Flat, 3.8),
    ("KS", "Kansas", StateKind::Graduated, 5.58),
    ("KY", "Kentucky", StateKind::Flat, 4.0),
    ("LA", "Louisiana", StateKind::Flat, 3.0),
    ("ME", "Maine", StateKind::Graduated, 7.15),
    ("MD", "Maryland", StateKind::Graduated, 5.75),
    ("MA", "Massachusetts", StateKind::Flat, 5.0),
    ("MI", "Michigan", StateKind::Flat, 4.25),
    ("MN", "Minnesota", StateKind::Graduated, 9.85),
    ("MS", "Mississippi", StateKind::Flat, 4.4),
    ("MO", "Missouri", StateKind::Graduated, 4.7),
    ("MT", "Montana", StateKind::Graduated, 5.9),
    ("NE", "Nebraska", StateKind::Graduated, 5.2),
    ("NV", "Nevada", StateKind::None, 0.0),
    ("NH", "New Hampshire", StateKind::None, 0.0),
    ("NJ", "New Jersey", StateKind::Graduated, 10.75),
    ("NM", "New Mexico", StateKind::Graduated, 5.9),
    ("NY", "New York", StateKind::Graduated, 10.9),
    ("NC", "North Carolina", StateKind::Flat, 4.25),
    ("ND", "North Dakota", StateKind::Graduated, 2.5),
    ("OH", "Ohio", StateKind::Graduated, 3.125),
    ("OK", "Oklahoma", StateKind::Graduated, 4.75),
    ("OR", "Oregon", StateKind::Graduated, 9.9),
    ("PA", "Pennsylvania", StateKind::Flat, 3.07),
    ("RI", "Rhode Island", StateKind::Graduated, 5.99),
    ("SC", "South Carolina", StateKind::Graduated, 6.0),
    ("SD", "South Dakota", StateKind::None, 0.0),
    ("TN", "Tennessee", StateKind::None, 0.0),
    ("TX", "Texas", StateKind::None, 0.0),
    ("UT", "Utah", StateKind::Flat, 4.5),
    ("VT", "Vermont", StateKind::Graduated, 8.75),
    ("VA", "Virginia", StateKind::Graduated, 5.75),
    ("WA", "Washington", StateKind::None, 0.0),
    ("WV", "West Virginia", StateKind::Graduated, 4.82),
    ("WI", "Wisconsin", StateKind::Graduated, 7.65),
    ("WY", "Wyoming", StateKind::None, 0.0),
];

/// A state by two-letter code or full name.
pub fn state(query: &str) -> Option<&'static (&'static str, &'static str, StateKind, f64)> {
    let q = query.trim();
    STATES
        .iter()
        .find(|s| s.0.eq_ignore_ascii_case(q) || s.1.eq_ignore_ascii_case(q))
}

fn kind_str(kind: StateKind) -> &'static str {
    match kind {
        StateKind::None => "no_wage_tax",
        StateKind::Flat => "flat",
        StateKind::Graduated => "graduated_top_rate",
    }
}

// ── the tool ────────────────────────────────────────────────────────────────

pub const TOPICS: &[&str] = &[
    "401k",
    "ira",
    "hsa",
    "contribution_limits",
    "rmd",
    "standard_deduction",
    "tax_brackets",
    "social_security",
    "full_retirement_age",
    "state_tax",
];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "topic": {"type": "string", "enum": TOPICS, "description": "401k (also 403(b), 457(b), TSP): employee deferral and catch-ups. ira, hsa: limits. contribution_limits: all of them. rmd: start age by birth_year. standard_deduction and tax_brackets: federal, by filing status. social_security: bend points, wage base, full retirement ages. full_retirement_age: by birth_year. state_tax: state income tax rates (approximate)."},
            "year": {"type": "integer", "description": "2024, 2025 or 2026 (default 2026). Not used by rmd, full_retirement_age or state_tax."},
            "birth_year": {"type": "integer", "description": "rmd and full_retirement_age: the birth year."},
            "filing_status": {"type": "string", "enum": ["single", "married_filing_jointly", "married_filing_separately", "head_of_household"], "description": "standard_deduction and tax_brackets: one status; omit for all."},
            "state": {"type": "string", "description": "state_tax: a two-letter code or name; omit for the whole table."}
        },
        "required": ["topic"]
    })
}

/// Serve the tool. `Err` is the message the model reads.
pub fn run(input: &Value) -> Result<Value, String> {
    let topic = input
        .get("topic")
        .and_then(Value::as_str)
        .ok_or("topic is required")?
        .trim()
        .to_ascii_lowercase();
    let year = match input.get("year") {
        None | Some(Value::Null) => DEFAULT_YEAR,
        Some(v) => v
            .as_i64()
            .and_then(|y| i32::try_from(y).ok())
            .ok_or("year must be an integer")?,
    };
    let needs_year = !matches!(topic.as_str(), "rmd" | "full_retirement_age" | "state_tax");
    if needs_year && !YEARS.contains(&year) {
        return Err(format!(
            "the tables cover {} to {}; {year} is outside them",
            YEARS.start(),
            YEARS.end()
        ));
    }
    let birth_year = || -> Result<i32, String> {
        input
            .get("birth_year")
            .and_then(Value::as_i64)
            .and_then(|y| i32::try_from(y).ok())
            .filter(|y| (1900..=2100).contains(y))
            .ok_or_else(|| "birth_year is required (four digits)".to_owned())
    };
    let filing = match input.get("filing_status").and_then(Value::as_str) {
        None => None,
        Some(s) => Some(FilingStatus::parse(s).ok_or_else(|| {
            "filing_status must be single, married_filing_jointly, married_filing_separately or head_of_household".to_owned()
        })?),
    };

    match topic.as_str() {
        "401k" | "ira" | "hsa" | "contribution_limits" => {
            let l = limits(year).expect("year checked");
            let k401 = json!({
                "employee_deferral": l.deferral,
                "catch_up_age_50_plus": l.catch_up_50,
                "catch_up_age_60_to_63": l.catch_up_60_63,
                "total_annual_additions_limit": l.annual_additions,
                "applies_to": "401(k), 403(b), governmental 457(b), TSP; the deferral limit is per person across all such plans",
            });
            let ira = json!({
                "contribution": l.ira,
                "catch_up_age_50_plus": l.ira_catch_up_50,
                "note": "Traditional and Roth combined; Roth eligibility phases out with income.",
            });
            let hsa = json!({
                "self_only": l.hsa_self,
                "family": l.hsa_family,
                "catch_up_age_55_plus": l.hsa_catch_up_55,
            });
            let facts = match topic.as_str() {
                "401k" => json!({"401k": k401}),
                "ira" => json!({"ira": ira}),
                "hsa" => json!({"hsa": hsa}),
                _ => json!({"401k": k401, "ira": ira, "hsa": hsa}),
            };
            Ok(json!({
                "topic": topic, "year": year, "status": l.status, "source": l.source,
                "facts": facts,
                "notes": if year == 2024 { "There is no separate age 60-63 catch-up in 2024; the age-50 amount applies." } else { "At 60 to 63 the age-60 catch-up replaces the age-50 catch-up." },
            }))
        }
        "standard_deduction" => {
            let statuses: Vec<FilingStatus> =
                filing.map_or(FilingStatus::ALL.to_vec(), |f| vec![f]);
            let by_status: serde_json::Map<String, Value> = statuses
                .iter()
                .filter_map(|s| {
                    standard_deduction(year, *s).map(|v| (s.as_str().to_owned(), json!(v)))
                })
                .collect();
            let (unmarried, married) = age_65_extra(year).expect("year checked");
            Ok(json!({
                "topic": topic, "year": year, "status": "published",
                "source": "IRS Rev. Proc. 2023-34, 2024-40 and 2025-32, and the July 2025 tax act for 2025",
                "facts": {
                    "standard_deduction": by_status,
                    "additional_age_65_plus_or_blind": {"unmarried": unmarried, "married_per_person": married},
                },
                "notes": "Amounts are the base deduction; the additional deduction applies per qualifying person. The 2025 to 2028 senior deduction of $6,000 under the July 2025 tax act is separate and phases out with income.",
            }))
        }
        "tax_brackets" => {
            let statuses: Vec<FilingStatus> =
                filing.map_or(FilingStatus::ALL.to_vec(), |f| vec![f]);
            let by_status: serde_json::Map<String, Value> = statuses
                .iter()
                .filter_map(|s| {
                    federal_brackets(year, *s).map(|b| {
                        (
                            s.as_str().to_owned(),
                            json!(
                                b.iter()
                                    .map(|(t, r)| json!({"from_taxable_income": t, "rate": r}))
                                    .collect::<Vec<_>>()
                            ),
                        )
                    })
                })
                .collect();
            Ok(json!({
                "topic": topic, "year": year, "status": bracket_status(year),
                "source": "IRS Rev. Proc. 2023-34, 2024-40, 2025-32",
                "facts": {"ordinary_income_brackets": by_status},
                "notes": "Thresholds apply to taxable income (after the standard deduction).",
            }))
        }
        "social_security" => {
            let ((b1, b2), projected) = bend_points(year);
            let base = wage_base(year).expect("year checked");
            let frs: Vec<Value> = [1937, 1938, 1939, 1940, 1941, 1942, 1943, 1955, 1956, 1957, 1958, 1959, 1960]
                .iter()
                .map(|b| {
                    let (y, m) = full_retirement_age(*b);
                    json!({"birth_year": b, "full_retirement_age": format!("{y} years{}", if m > 0 { format!(" {m} months") } else { String::new() })})
                })
                .collect();
            Ok(json!({
                "topic": topic, "year": year,
                "status": if projected { "projected" } else { "published" },
                "source": "SSA fact sheets and the national average wage index",
                "facts": {
                    "pia_bend_points_first_eligible_in_year": {"first": b1, "second": b2, "formula": "PIA = 90% of AIME up to the first, plus 32% between, plus 15% above the second"},
                    "contribution_and_benefit_base": base,
                    "payroll_tax_employee": {"social_security": 0.062, "medicare": 0.0145, "additional_medicare_over_200k_single_250k_joint": 0.009},
                    "full_retirement_age_by_birth_year": frs,
                    "early_claiming": "benefit reduced 5/9 of 1% per month for the first 36 months before full retirement age and 5/12 of 1% per month beyond that",
                    "delayed_claiming": "benefit rises 2/3 of 1% per month (8% a year) after full retirement age, to age 70, for births in 1943 or later",
                },
            }))
        }
        "full_retirement_age" => {
            let b = birth_year()?;
            let (y, m) = full_retirement_age(b);
            Ok(json!({
                "topic": topic, "status": "published", "source": "SSA",
                "facts": {"birth_year": b, "years": y, "months": m, "as_decimal_years": f64::from(y) + f64::from(m) / 12.0},
            }))
        }
        "rmd" => {
            let b = birth_year()?;
            let (age, rule) = rmd_start(b);
            Ok(json!({
                "topic": topic, "status": "published", "source": "SECURE Act and SECURE 2.0 Act",
                "facts": {"birth_year": b, "rmd_start_age": age, "rule": rule, "first_distribution_deadline": "April 1 of the year after reaching that age; later years by December 31"},
            }))
        }
        "state_tax" => {
            let row = |s: &(&str, &str, StateKind, f64)| json!({"code": s.0, "name": s.1, "kind": kind_str(s.2), "rate_percent": s.3});
            let facts = match input.get("state").and_then(Value::as_str) {
                Some(q) => {
                    let s = state(q)
                        .ok_or_else(|| format!("no state matches `{q}`; use a two-letter code"))?;
                    json!([row(s)])
                }
                None => json!(STATES.iter().map(row).collect::<Vec<_>>()),
            };
            Ok(json!({
                "topic": topic, "status": "approximate",
                "source": "state statutes, rounded; 2025 rates, assumed unchanged for 2026",
                "facts": facts,
                "notes": "APPROXIMATIONS. `flat` is the wage tax rate; `graduated_top_rate` is the top marginal rate, which overstates what most households pay; `no_wage_tax` states may still tax other income. Local income taxes, credits and deductions are not included. Use the rate as a labelled estimate.",
            }))
        }
        other => Err(format!(
            "unknown topic `{other}`; use one of {}",
            TOPICS.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_limits_match_the_published_figures() {
        let l = limits(2025).unwrap();
        assert_eq!(
            (l.deferral, l.catch_up_50, l.catch_up_60_63),
            (23_500.0, 7_500.0, 11_250.0)
        );
        assert_eq!(
            (l.ira, l.hsa_self, l.hsa_family),
            (7_000.0, 4_300.0, 8_550.0)
        );
        let l = limits(2026).unwrap();
        assert_eq!(
            (l.deferral, l.catch_up_50, l.ira, l.ira_catch_up_50),
            (24_500.0, 8_000.0, 7_500.0, 1_100.0)
        );
        assert_eq!(employee_deferral_limit(2026), Some(24_500.0));
        assert_eq!(employee_deferral_limit(2027), None);
    }

    #[test]
    fn bend_points_follow_the_wage_index_and_match_ssa() {
        // Published: 2024 $1,174 / $7,078; 2025 $1,226 / $7,391; 2026 $1,286 / $7,749.
        assert_eq!(bend_points(2024), ((1_174.0, 7_078.0), false));
        assert_eq!(bend_points(2025), ((1_226.0, 7_391.0), false));
        assert_eq!(bend_points(2026), ((1_286.0, 7_749.0), false));
        let (later, projected) = bend_points(2030);
        assert!(projected && later.0 > 1_286.0);
    }

    #[test]
    fn retirement_ages_by_birth_year() {
        assert_eq!(full_retirement_age(1950), (66, 0));
        assert_eq!(full_retirement_age(1957), (66, 6));
        assert_eq!(full_retirement_age(1960), (67, 0));
        assert_eq!(rmd_start(1955).0, 73.0);
        assert_eq!(rmd_start(1960).0, 75.0);
        assert_eq!(rmd_start(1950).0, 72.0);
    }

    #[test]
    fn brackets_start_at_zero_and_ascend() {
        for year in YEARS {
            for status in FilingStatus::ALL {
                let b = federal_brackets(year, status).unwrap();
                assert_eq!(b.len(), 7);
                assert_eq!(b[0].0, 0.0);
                assert!(b.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
            }
        }
        assert_eq!(
            standard_deduction(2026, FilingStatus::MarriedJoint),
            Some(32_200.0)
        );
        assert_eq!(standard_deduction(2023, FilingStatus::Single), None);
    }

    #[test]
    fn the_tool_answers_and_refuses() {
        let out = run(&json!({"topic": "401k", "year": 2025})).unwrap();
        assert_eq!(out["facts"]["401k"]["employee_deferral"], 23_500.0);
        assert_eq!(out["status"], "published");
        assert!(
            run(&json!({"topic": "401k", "year": 2030}))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            run(&json!({"topic": "rmd"}))
                .unwrap_err()
                .contains("birth_year")
        );
        let rmd = run(&json!({"topic": "rmd", "birth_year": 1962})).unwrap();
        assert_eq!(rmd["facts"]["rmd_start_age"], 75.0);
        let co = run(&json!({"topic": "state_tax", "state": "co"})).unwrap();
        assert_eq!(co["facts"][0]["kind"], "flat");
        assert!(co["notes"].as_str().unwrap().contains("APPROXIMATIONS"));
        assert_eq!(
            run(&json!({"topic": "state_tax", "state": "TX"})).unwrap()["facts"][0]["rate_percent"],
            0.0
        );
        assert!(run(&json!({"topic": "nope"})).is_err());
        let ss = run(&json!({"topic": "social_security", "year": 2026})).unwrap();
        assert_eq!(ss["facts"]["contribution_and_benefit_base"], 184_500.0);
        assert_eq!(
            ss["facts"]["pia_bend_points_first_eligible_in_year"]["first"],
            1_286.0
        );
    }
}
