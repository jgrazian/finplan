//! Delimited exports (CSV, TSV, semicolon-separated): read into rows, rendered
//! as a text table, and mined for transactions or holdings when the header
//! says that is what they hold.

use super::data::{AccountData, DocumentData, Position, Transaction};
use super::redact;

/// The rows of a delimited file, quotes resolved.
pub fn parse_rows(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let delimiter = detect_delimiter(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            c if c == delimiter && !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                push_row(&mut rows, std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        push_row(&mut rows, row);
    }
    rows
}

fn push_row(rows: &mut Vec<Vec<String>>, row: Vec<String>) {
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row.into_iter().map(|f| f.trim().to_owned()).collect());
    }
}

/// The delimiter that splits the first lines most consistently.
fn detect_delimiter(text: &str) -> char {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(8)
        .collect();
    [',', '\t', ';', '|']
        .into_iter()
        .max_by_key(|d| {
            let counts: Vec<usize> = lines.iter().map(|l| l.matches(*d).count()).collect();
            let first = counts.first().copied().unwrap_or(0);
            let consistent = counts.iter().filter(|c| **c == first).count();
            if first == 0 {
                0
            } else {
                consistent * 100 + first.min(99)
            }
        })
        .unwrap_or(',')
}

/// Rows per rendered page: a long export is read a page at a time.
pub const ROWS_PER_PAGE: usize = 100;

/// The rows as text pages (`a | b | c`), the header repeated on each.
pub fn render_pages(rows: &[Vec<String>]) -> Vec<String> {
    let Some((header, body)) = rows.split_first() else {
        return vec![String::new()];
    };
    let line = |r: &Vec<String>| r.join(" | ");
    if body.is_empty() {
        return vec![line(header)];
    }
    body.chunks(ROWS_PER_PAGE)
        .map(|chunk| {
            let mut page = line(header);
            for row in chunk {
                page.push('\n');
                page.push_str(&line(row));
            }
            page
        })
        .collect()
}

/// Transactions or holdings found in the rows, when the header names them.
pub fn mine(rows: &[Vec<String>]) -> Option<DocumentData> {
    let (header_at, header) =
        rows.iter().enumerate().take(12).find(|(_, r)| {
            r.len() >= 3 && r.iter().filter(|f| looks_like_header(f)).count() >= 2
        })?;
    let cols: Vec<String> = header.iter().map(|h| h.to_ascii_lowercase()).collect();
    let body = &rows[header_at + 1..];

    if let Some(data) = transactions(&cols, body) {
        return Some(data);
    }
    holdings(&cols, body)
}

fn looks_like_header(field: &str) -> bool {
    let f = field.to_ascii_lowercase();
    f.len() < 40
        && [
            "date",
            "amount",
            "description",
            "payee",
            "memo",
            "debit",
            "credit",
            "symbol",
            "ticker",
            "quantity",
            "shares",
            "price",
            "value",
            "balance",
            "details",
            "type",
        ]
        .iter()
        .any(|k| f.contains(k))
}

fn column(cols: &[String], names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|n| cols.iter().position(|c| c == n))
        .or_else(|| {
            names
                .iter()
                .find_map(|n| cols.iter().position(|c| c.contains(n)))
        })
}

fn transactions(cols: &[String], body: &[Vec<String>]) -> Option<DocumentData> {
    let date = column(
        cols,
        &["date", "posted date", "posting date", "transaction date"],
    )?;
    let description = column(
        cols,
        &[
            "description",
            "payee",
            "merchant",
            "name",
            "details",
            "memo",
        ],
    )?;
    let amount = column(cols, &["amount"]);
    let debit = column(cols, &["debit", "withdrawal", "withdrawals"]);
    let credit = column(cols, &["credit", "deposit", "deposits"]);
    if amount.is_none() && debit.is_none() && credit.is_none() {
        return None;
    }
    let mut out = Vec::new();
    for row in body {
        let get = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let Some(day) = redact::parse_date(get(date)) else {
            continue;
        };
        let value = match amount {
            Some(i) => parse_amount(get(i)),
            None => {
                let d = debit.and_then(|i| parse_amount(get(i)));
                let c = credit.and_then(|i| parse_amount(get(i)));
                match (d, c) {
                    (Some(d), _) if d != 0.0 => Some(-d.abs()),
                    (_, Some(c)) => Some(c.abs()),
                    _ => None,
                }
            }
        };
        let Some(value) = value else { continue };
        out.push(Transaction {
            date: day,
            amount: value,
            description: get(description).to_owned(),
        });
    }
    if out.is_empty() {
        return None;
    }
    Some(DocumentData {
        institution: None,
        accounts: vec![AccountData {
            transactions: out,
            ..AccountData::default()
        }],
    })
}

fn holdings(cols: &[String], body: &[Vec<String>]) -> Option<DocumentData> {
    let symbol = column(cols, &["symbol", "ticker"])?;
    let units = column(cols, &["quantity", "shares", "units"])?;
    let name = column(cols, &["description", "name", "security"]);
    let price = column(cols, &["price", "last price"]);
    let value = column(cols, &["market value", "current value", "value"]);
    let mut out = Vec::new();
    for row in body {
        let get = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let Some(units) = parse_amount(get(units)) else {
            continue;
        };
        if get(symbol).is_empty() {
            continue;
        }
        out.push(Position {
            symbol: Some(get(symbol).to_ascii_uppercase()),
            name: name.map(|i| get(i).to_owned()).filter(|n| !n.is_empty()),
            units,
            unit_price: price.and_then(|i| parse_amount(get(i))),
            market_value: value.and_then(|i| parse_amount(get(i))),
        });
    }
    if out.is_empty() {
        return None;
    }
    Some(DocumentData {
        institution: None,
        accounts: vec![AccountData {
            positions: out,
            ..AccountData::default()
        }],
    })
}

/// `1,234.56`, `-$50.00`, `(50.00)`, `$1 234,50` is not supported.
pub fn parse_amount(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let negative = t.starts_with('(') && t.ends_with(')') || t.starts_with('-') || t.ends_with('-');
    let cleaned: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = cleaned.parse().ok()?;
    Some(if negative { -value } else { value })
}
