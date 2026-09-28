//! Figures read from statement *text* (PDF text layers, plain text): the
//! institution, the account's last four digits and its labelled balances.
//! Best effort and conservative, so the tools have something to match on when
//! there is no OFX; the model still reads the text itself.

use std::sync::LazyLock;

use regex::Regex;

use super::data::{AccountData, Balance, DocumentData};
use super::redact;

const INSTITUTIONS: &[&str] = &[
    "Bank of America",
    "Wells Fargo",
    "Chase",
    "JPMorgan",
    "Citibank",
    "Citi",
    "Capital One",
    "US Bank",
    "U.S. Bank",
    "PNC",
    "TD Bank",
    "Truist",
    "Ally",
    "Discover",
    "American Express",
    "Marcus",
    "SoFi",
    "USAA",
    "Navy Federal",
    "Fidelity",
    "Vanguard",
    "Charles Schwab",
    "Schwab",
    "E*TRADE",
    "Merrill",
    "Robinhood",
    "T. Rowe Price",
    "TIAA",
    "Principal",
    "Empower",
    "Voya",
    "Prudential",
    "Edward Jones",
    "Morgan Stanley",
    "Betterment",
    "Wealthfront",
    "Interactive Brokers",
    "Lincoln Financial",
    "John Hancock",
    "Nationwide",
];

static LAST4: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:account(?:\s+(?:number|no\.?|#))?|acct\.?|ending\s+in|ending)\s*[:#]?\s*(?:••••|[Xx*]{2,}[\s-]*|\.\.\.)?[\s-]*(\d{4})\b|••••(\d{4})")
        .expect("last4 pattern")
});

static BALANCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(ending|closing|beginning|opening|new|current|available|total(?:\s+account)?|account|ledger|cash|portfolio)\s+(?:balance|value)\b[^\d$\-(\n]{0,40}(-?\$?\s?\(?-?[\d,]+\.\d{2}\)?)")
        .expect("balance pattern")
});

static AS_OF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:as\s+of|through|to|ending)\s+((?:\d{1,2}/\d{1,2}/\d{2,4})|(?:[A-Za-z]{3,9}\.?\s+\d{1,2},?\s+\d{4}))")
        .expect("as-of pattern")
});

/// Read hints out of `text`; `None` when nothing turned up.
pub fn from_text(text: &str) -> Option<DocumentData> {
    let head: String = text.chars().take(6_000).collect();
    let institution = INSTITUTIONS
        .iter()
        .filter_map(|name| {
            let lower = head.to_lowercase();
            lower.find(&name.to_lowercase()).map(|at| (at, *name))
        })
        .min_by_key(|(at, _)| *at)
        .map(|(_, name)| name.to_owned());

    let last4 = LAST4
        .captures(text)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_owned());

    let mut balances: Vec<Balance> = Vec::new();
    for cap in BALANCE.captures_iter(text) {
        let label = cap[1].to_lowercase();
        // "beginning" and "opening" are not where the account stands now.
        if matches!(label.as_str(), "beginning" | "opening") {
            continue;
        }
        let label = match label.as_str() {
            "total account" => "total".to_owned(),
            "account" | "portfolio" | "new" => "total".to_owned(),
            other => other.to_owned(),
        };
        let Some(amount) = super::tabular::parse_amount(&cap[2]) else {
            continue;
        };
        if balances.iter().any(|b| b.label == label) {
            continue;
        }
        let (start, end) = cap.get(0).map_or((0, 0), |m| (m.start(), m.end()));
        let line_end = end + text[end..].find('\n').unwrap_or(text.len() - end);
        let as_of = AS_OF
            .captures(&text[start..line_end])
            .and_then(|c| redact::parse_date(&c[1]));
        balances.push(Balance {
            label,
            amount,
            as_of,
        });
    }

    if institution.is_none() && last4.is_none() && balances.is_empty() {
        return None;
    }
    Some(DocumentData {
        institution,
        accounts: vec![AccountData {
            last4,
            balances,
            ..AccountData::default()
        }],
    })
}
