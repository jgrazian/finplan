//! Redaction: what is masked in a document's text before anything is stored or
//! shown to a model.
//!
//! Masked: Social Security and taxpayer identification numbers (SSN, ITIN),
//! employer identification numbers, full account, card and routing numbers
//! (only the last four survive, as `••••1234`, so an account can still be
//! matched), dates of birth, street addresses and P.O. boxes, e-mail addresses
//! and phone numbers.
//!
//! One birth date is kept out of band: the plan needs the owner's age, so the
//! first birth date found is returned as [`Redacted::birth_date_hint`] (ISO)
//! while every birth date in the text itself is masked.
//!
//! Left alone, on purpose: dollar amounts, ordinary dates (statement and
//! transaction dates), share counts, tickers, and the city, state and ZIP of
//! an address (the state drives the tax config). The rules are conservative
//! about what looks like an identifier: a bare 9-digit number is masked only
//! when it follows an SSN or routing label or passes the ABA routing-number
//! checksum, and long digit runs only when they are not part of a decimal
//! amount or a timestamp.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// Text with identifiers masked, and what was found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Redacted {
    pub text: String,
    /// The first date of birth found, as `YYYY-MM-DD`.
    pub birth_date_hint: Option<String>,
    pub counts: RedactionCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RedactionCounts {
    pub ssn: u32,
    pub ein: u32,
    pub account_numbers: u32,
    pub birth_dates: u32,
    pub addresses: u32,
    pub contact: u32,
}

impl RedactionCounts {
    pub fn total(&self) -> u32 {
        self.ssn
            + self.ein
            + self.account_numbers
            + self.birth_dates
            + self.addresses
            + self.contact
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("redaction pattern compiles")
}

/// `123-45-6789`.
static SSN_DASHED: LazyLock<Regex> = LazyLock::new(|| re(r"\b\d{3}-\d{2}-\d{4}\b"));

/// An SSN, ITIN or TIN label, then nine digits with optional separators.
static SSN_LABELLED: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(\bssn\b|\bss#|\bsocial\s+security(?:\s+(?:number|no\.?|num\.?|#))?|\bitin\b|\btin\b|\btaxpayer\s+identification(?:\s+number)?)([^\n\d]{0,25}?)\b(\d{3}[ -]?\d{2}[ -]?\d{4})\b",
    )
});

/// `12-3456789`.
static EIN: LazyLock<Regex> = LazyLock::new(|| re(r"\b\d{2}-\d{7}\b"));

/// An account-like label followed by a number, possibly partly masked.
static ACCOUNT_LABELLED: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b((?:account|acct|a/c|routing|aba|rtn|member|card|policy|iban)\b(?:\s+(?:number|no\.?|num\.?|#|id))?\.?)([ \t]*[:#\-]?[ \t]*)([\dXx*•][\dXx*•\- ]*)",
    )
});

/// Sixteen digits in groups of four, as on a card.
static CARD_GROUPED: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:\d{4}[ -]){3}(\d{4})\b"));

/// A bare run of ten to seventeen digits, or nine that may be a routing number.
static DIGIT_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"\d{9,17}"));

const MONTHS: &str = "jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?";

/// A birth-date label and the date after it, in the common written forms.
static DOB: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"(?i)\b(date\s+of\s+birth|birth\s*date|birthdate|d\.?o\.?b\b\.?|born\b(?:\s+on)?)([ \t]*(?:\([^)\n]*\))?[ \t]*[:\-]?[ \t]*)(\d{{1,2}}[/.\-]\d{{1,2}}[/.\-]\d{{2,4}}|\d{{4}}-\d{{2}}-\d{{2}}|(?:{MONTHS})\.?[ \t]+\d{{1,2}}(?:st|nd|rd|th)?,?[ \t]+\d{{4}}|\d{{1,2}}[ \t]+(?:{MONTHS})\.?,?[ \t]+\d{{4}})"
    ))
});

const STREET_SUFFIX: &str = "street|st|avenue|ave|road|rd|boulevard|blvd|drive|dr|lane|ln|court|ct|way|place|pl|terrace|ter|circle|cir|parkway|pkwy|highway|hwy|trail|trl|square|sq|alley|aly";

/// `1234 N Main St`, `77 Ocean View Drive Apt 4B`.
static STREET: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"(?i)\b\d{{1,6}}[ \t]+(?:[NSEW]\.?[ \t]+|(?:north|south|east|west)[ \t]+)?(?:[A-Za-z0-9.'\-]+[ \t]+){{0,4}}?(?:{STREET_SUFFIX})\b\.?(?:,?[ \t]*(?:apt|apartment|unit|suite|ste|#)\.?[ \t]*[\w\-]+)?"
    ))
});

static PO_BOX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bP\.?[ \t]?O\.?[ \t]+Box[ \t]+\d+\b"));

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b"));

/// `(303) 555-1234`, `303-555-1234`, `303.555.1234`, `+1 303 555 1234`.
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?:\+1[ .\-]?)?(?:\(\d{3}\)[ .\-]?|\b\d{3}[.\-])\d{3}[.\-]\d{4}\b"));

/// Mask everything described in the module docs.
pub fn redact(text: &str) -> Redacted {
    let mut counts = RedactionCounts::default();
    let mut birth_date_hint = None;

    // Birth dates first: a masked date must not be read as anything else.
    let mut out = DOB
        .replace_all(text, |caps: &Captures| {
            counts.birth_dates += 1;
            if birth_date_hint.is_none() {
                birth_date_hint = parse_birth_date(&caps[3]);
            }
            format!("{}{}[birth date removed]", &caps[1], &caps[2])
        })
        .into_owned();

    out = SSN_LABELLED
        .replace_all(&out, |caps: &Captures| {
            counts.ssn += 1;
            format!("{}{}•••-••-••••", &caps[1], &caps[2])
        })
        .into_owned();
    out = SSN_DASHED
        .replace_all(&out, |_: &Captures| {
            counts.ssn += 1;
            "•••-••-••••"
        })
        .into_owned();
    out = EIN
        .replace_all(&out, |_: &Captures| {
            counts.ein += 1;
            "••-•••••••"
        })
        .into_owned();

    out = ACCOUNT_LABELLED
        .replace_all(&out, |caps: &Captures| {
            let value = caps[3].trim_end_matches([' ', '-']);
            match mask_number(value) {
                Some(masked) => {
                    if value.chars().filter(char::is_ascii_digit).count() > 4 {
                        counts.account_numbers += 1;
                    }
                    let trailing = &caps[3][value.len()..];
                    format!("{}{}{masked}{trailing}", &caps[1], &caps[2])
                }
                None => caps[0].to_owned(),
            }
        })
        .into_owned();
    out = CARD_GROUPED
        .replace_all(&out, |caps: &Captures| {
            counts.account_numbers += 1;
            format!("{}{}", "••••", &caps[1])
        })
        .into_owned();
    out = mask_digit_runs(&out, &mut counts);

    out = PO_BOX
        .replace_all(&out, |_: &Captures| {
            counts.addresses += 1;
            "[address removed]"
        })
        .into_owned();
    let before_streets = out.clone();
    out = STREET
        .replace_all(&before_streets, |caps: &Captures| {
            // A dollar figure or a decimal is not a house number.
            let start = caps.get(0).map_or(0, |m| m.start());
            if before_streets[..start].ends_with(['$', '.', ',']) {
                return caps[0].to_owned();
            }
            counts.addresses += 1;
            "[address removed]".to_owned()
        })
        .into_owned();

    out = EMAIL
        .replace_all(&out, |_: &Captures| {
            counts.contact += 1;
            "[email removed]"
        })
        .into_owned();
    out = PHONE
        .replace_all(&out, |_: &Captures| {
            counts.contact += 1;
            "[phone removed]"
        })
        .into_owned();

    Redacted {
        text: out,
        birth_date_hint,
        counts,
    }
}

/// `••••` and the last four digits of a number, when `value` is number-like
/// (five or more digits and mask characters); `None` when it is not one.
fn mask_number(value: &str) -> Option<String> {
    let significant = value
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, 'X' | 'x' | '*' | '•'))
        .count();
    if significant < 5 {
        return None;
    }
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    let keep = digits.len().min(4);
    Some(format!("••••{}", &digits[digits.len() - keep..]))
}

/// Mask bare digit runs that look like account numbers, cards or routing
/// numbers, but not decimal amounts, timestamps or parts of longer tokens.
fn mask_digit_runs(text: &str, counts: &mut RedactionCounts) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in DIGIT_RUN.find_iter(text) {
        let (start, end) = (m.start(), m.end());
        let run = m.as_str();
        let before = start.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(end).copied();
        // Part of a decimal, a grouped number, or a longer alphanumeric token.
        let joined_before = matches!(before, Some(b'.' | b',' | b'$' | b'-'))
            || before.is_some_and(|b| b.is_ascii_alphanumeric());
        let joined_after = matches!(after, Some(b'.' | b','))
            && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
            || after.is_some_and(|b| b.is_ascii_alphanumeric());
        if joined_before || joined_after || run.len() > 17 {
            continue;
        }
        let mask = match run.len() {
            9 => aba_checksum(run),
            14 => !looks_like_timestamp(run),
            _ => run.len() >= 10,
        };
        if !mask {
            continue;
        }
        out.push_str(&text[last..start]);
        out.push_str("••••");
        out.push_str(&run[run.len() - 4..]);
        last = end;
        counts.account_numbers += 1;
    }
    out.push_str(&text[last..]);
    out
}

/// The ABA routing-number checksum: 3(d1+d4+d7) + 7(d2+d5+d8) + (d3+d6+d9) ≡ 0
/// (mod 10), and a Federal Reserve prefix (not 00, and at most 12, 21–32,
/// 61–72 or 80).
fn aba_checksum(run: &str) -> bool {
    let d: Vec<u32> = run.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 9 {
        return false;
    }
    let prefix = d[0] * 10 + d[1];
    let fed = matches!(prefix, 1..=12 | 21..=32 | 61..=72 | 80);
    let sum = 3 * (d[0] + d[3] + d[6]) + 7 * (d[1] + d[4] + d[7]) + (d[2] + d[5] + d[8]);
    fed && sum.is_multiple_of(10)
}

/// `YYYYMMDDHHMMSS`, as OFX and some CSV exports stamp times.
fn looks_like_timestamp(run: &str) -> bool {
    let n = |a: usize, b: usize| run[a..b].parse::<u32>().unwrap_or(99);
    (run.starts_with("19") || run.starts_with("20"))
        && (1..=12).contains(&n(4, 6))
        && (1..=31).contains(&n(6, 8))
        && n(8, 10) < 24
        && n(10, 12) < 60
        && n(12, 14) < 60
}

/// The ISO date of a written date of birth, when it is a plausible one
/// (US month-first for numeric forms unless the first part cannot be a month).
pub fn parse_birth_date(text: &str) -> Option<String> {
    let date = parse_date(text)?;
    let year: i32 = date[..4].parse().ok()?;
    (1900..=2100).contains(&year).then_some(date)
}

/// A date in `M/D/YYYY`, `M/D/YY`, `YYYY-MM-DD`, `Month D, YYYY` or
/// `D Month YYYY` form, as `YYYY-MM-DD`. Two-digit years above 30 are 19xx.
pub fn parse_date(text: &str) -> Option<String> {
    let text = text.trim();
    let iso = |y: i32, m: u32, d: u32| {
        let valid = (1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&d);
        valid.then(|| format!("{y:04}-{m:02}-{d:02}"))
    };
    let year_of = |s: &str| -> Option<i32> {
        let y: i32 = s.parse().ok()?;
        Some(if s.len() <= 2 {
            if y > 30 { 1900 + y } else { 2000 + y }
        } else {
            y
        })
    };
    let parts: Vec<&str> = text.split(['/', '.', '-']).collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
    {
        if parts[0].len() == 4 {
            return iso(
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            );
        }
        let (a, b): (u32, u32) = (parts[0].parse().ok()?, parts[1].parse().ok()?);
        let year = year_of(parts[2])?;
        // Month first, unless the first part cannot be a month.
        return if a > 12 {
            iso(year, b, a)
        } else {
            iso(year, a, b)
        };
    }
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() == 3 {
        let strip = |w: &str| {
            w.trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .to_owned()
        };
        let (month_first, day_word, year_word) = if month_number(words[0]).is_some() {
            (true, words[1], words[2])
        } else {
            (false, words[0], words[2])
        };
        let month = if month_first {
            month_number(words[0])?
        } else {
            month_number(words[1])?
        };
        let day: u32 = strip(day_word).parse().ok()?;
        return iso(year_word.parse().ok()?, month, day);
    }
    None
}

fn month_number(word: &str) -> Option<u32> {
    let word = word.trim_end_matches('.').to_ascii_lowercase();
    if word.len() < 3 {
        return None;
    }
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    NAMES
        .iter()
        .position(|name| name.starts_with(&word))
        .map(|i| i as u32 + 1)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
