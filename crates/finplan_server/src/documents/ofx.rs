//! A small deterministic OFX/QFX reader: the institution, and per statement the
//! account's last four digits, balances, investment positions and cash
//! transactions.
//!
//! Both dialects parse: OFX 1.x is SGML (leaf tags never close) and 2.x is XML.
//! Rather than build a tree, [`parse`] walks the tag stream with a little state
//! (which statement, transaction, position or balance is open), which is
//! enough for the figures the tools need and is indifferent to closing tags on
//! leaves. Investment buys and sells are not read; their effect is in the
//! positions and cash.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::data::{AccountData, Balance, DocumentData, Position, Transaction, last_four};
use super::redact;

/// The tags of the stream: `<NAME>` or `</NAME>`, then the text up to the next tag.
static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(/?)([A-Za-z][A-Za-z0-9.]*)>([^<]*)").expect("ofx token"));

/// Whether `bytes` look like an OFX file (header or root element).
pub fn sniff(head: &[u8]) -> bool {
    let head = String::from_utf8_lossy(head).to_ascii_uppercase();
    head.contains("OFXHEADER") || head.contains("<OFX>") || head.contains("<OFX ")
}

/// Parse an OFX/QFX file. `None` when it has no statement in it.
pub fn parse(text: &str) -> Option<DocumentData> {
    let start = text.to_ascii_uppercase().find("<OFX")?;
    let body = &text[start..];

    let mut institution: Option<String> = None;
    let mut accounts: Vec<AccountData> = Vec::new();
    let mut account: Option<AccountData> = None;
    let mut txn: Option<Txn> = None;
    let mut position: Option<Pos> = None;
    let mut balance: Option<PendingBalance> = None;
    let mut security: Option<Sec> = None;
    let mut securities: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let mut in_fi = false;
    let mut in_invbal = false;
    // Positions carry a security id; names come from the SECLIST after them.
    let mut position_ids: Vec<(usize, usize, String)> = Vec::new();

    for cap in TOKEN.captures_iter(body) {
        let closing = !cap[1].is_empty();
        let tag = cap[2].to_ascii_uppercase();
        let value = decode(cap[3].trim());

        if closing {
            match tag.as_str() {
                "FI" => in_fi = false,
                "INVBAL" => in_invbal = false,
                "STMTRS" | "CCSTMTRS" | "INVSTMTRS" => {
                    accounts.extend(account.take());
                }
                "STMTTRN" => {
                    if let (Some(t), Some(a)) = (txn.take(), account.as_mut())
                        && let Some(t) = t.finish()
                    {
                        a.transactions.push(t);
                    }
                }
                "LEDGERBAL" | "AVAILBAL" => {
                    if let (Some(b), Some(a)) = (balance.take(), account.as_mut())
                        && let Some(b) = b.finish()
                    {
                        a.balances.push(b);
                    }
                }
                "POSSTOCK" | "POSMF" | "POSOPT" | "POSDEBT" | "POSOTHER" => {
                    if let (Some(p), Some(a)) = (position.take(), account.as_mut()) {
                        let index = a.positions.len();
                        let uniqueid = p.uniqueid.clone();
                        a.positions.push(p.finish());
                        if let Some(id) = uniqueid {
                            position_ids.push((accounts.len(), index, id));
                        }
                    }
                }
                "STOCKINFO" | "MFINFO" | "OPTINFO" | "DEBTINFO" | "OTHERINFO" => {
                    if let Some(s) = security.take()
                        && let Some(id) = s.uniqueid
                    {
                        securities.insert(id, (s.ticker, s.name));
                    }
                }
                _ => {}
            }
            continue;
        }

        match tag.as_str() {
            "FI" => in_fi = true,
            "ORG" if in_fi => institution = institution.or_else(|| non_empty(&value)),
            "STMTRS" | "CCSTMTRS" | "INVSTMTRS" => {
                accounts.extend(account.take());
                account = Some(AccountData {
                    kind: match tag.as_str() {
                        "CCSTMTRS" => Some("credit_card".into()),
                        "INVSTMTRS" => Some("investment".into()),
                        _ => None,
                    },
                    ..AccountData::default()
                });
            }
            "CURDEF" => {
                if let Some(a) = account.as_mut() {
                    a.currency = non_empty(&value);
                }
            }
            "ACCTID" => {
                if let Some(a) = account.as_mut() {
                    a.last4 = last_four(&value);
                }
            }
            "ACCTTYPE" => {
                if let Some(a) = account.as_mut() {
                    a.kind = non_empty(&value).map(|v| v.to_ascii_lowercase());
                }
            }
            "LEDGERBAL" => balance = Some(PendingBalance::new("ledger")),
            "AVAILBAL" => balance = Some(PendingBalance::new("available")),
            "INVBAL" => in_invbal = true,
            "AVAILCASH" if in_invbal => {
                if let (Some(a), Some(amount)) = (account.as_mut(), number(&value)) {
                    a.balances.push(Balance {
                        label: "cash".into(),
                        amount,
                        as_of: None,
                    });
                }
            }
            "BALAMT" => {
                if let Some(b) = balance.as_mut() {
                    b.amount = number(&value);
                }
            }
            "DTASOF" => {
                if let Some(b) = balance.as_mut() {
                    b.as_of = date(&value);
                }
            }
            "STMTTRN" => txn = Some(Txn::default()),
            "TRNAMT" => {
                if let Some(t) = txn.as_mut() {
                    t.amount = number(&value);
                }
            }
            "DTPOSTED" => {
                if let Some(t) = txn.as_mut() {
                    t.date = date(&value);
                }
            }
            "NAME" if txn.is_some() => {
                if let Some(t) = txn.as_mut() {
                    t.name = non_empty(&value);
                }
            }
            "MEMO" if txn.is_some() => {
                if let Some(t) = txn.as_mut() {
                    t.memo = non_empty(&value);
                }
            }
            "POSSTOCK" | "POSMF" | "POSOPT" | "POSDEBT" | "POSOTHER" => {
                position = Some(Pos::default());
            }
            "UNIQUEID" => {
                if let Some(p) = position.as_mut() {
                    p.uniqueid = non_empty(&value);
                } else if let Some(s) = security.as_mut() {
                    s.uniqueid = non_empty(&value);
                }
            }
            "UNITS" => {
                if let Some(p) = position.as_mut() {
                    p.units = number(&value);
                }
            }
            "UNITPRICE" => {
                if let Some(p) = position.as_mut() {
                    p.unit_price = number(&value);
                }
            }
            "MKTVAL" => {
                if let Some(p) = position.as_mut() {
                    p.market_value = number(&value);
                }
            }
            "STOCKINFO" | "MFINFO" | "OPTINFO" | "DEBTINFO" | "OTHERINFO" => {
                security = Some(Sec::default());
            }
            "SECNAME" => {
                if let Some(s) = security.as_mut() {
                    s.name = non_empty(&value);
                }
            }
            "TICKER" => {
                if let Some(s) = security.as_mut() {
                    s.ticker = non_empty(&value);
                }
            }
            _ => {}
        }
    }
    accounts.extend(account.take());

    // Name the positions from the security list (which follows the statements).
    // `position_ids` indexes accounts as they were when the position closed:
    // the count of accounts finished so far, i.e. the open account's slot.
    for (slot, index, id) in position_ids {
        let Some((ticker, name)) = securities.get(&id) else {
            continue;
        };
        if let Some(p) = accounts
            .get_mut(slot)
            .and_then(|a| a.positions.get_mut(index))
        {
            p.symbol = ticker.clone().or_else(|| Some(id.clone()));
            p.name = name.clone();
        }
    }
    let data = DocumentData {
        institution,
        accounts,
    };
    (!data.accounts.is_empty()).then_some(data)
}

#[derive(Default)]
struct Txn {
    amount: Option<f64>,
    date: Option<String>,
    name: Option<String>,
    memo: Option<String>,
}

impl Txn {
    fn finish(self) -> Option<Transaction> {
        let description = [self.name, self.memo]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        Some(Transaction {
            date: self.date?,
            amount: self.amount?,
            description: redact::redact(&description).text,
        })
    }
}

#[derive(Default)]
struct Pos {
    uniqueid: Option<String>,
    units: Option<f64>,
    unit_price: Option<f64>,
    market_value: Option<f64>,
}

impl Pos {
    fn finish(self) -> Position {
        Position {
            symbol: self.uniqueid,
            name: None,
            units: self.units.unwrap_or(0.0),
            unit_price: self.unit_price,
            market_value: self.market_value,
        }
    }
}

#[derive(Default)]
struct Sec {
    uniqueid: Option<String>,
    ticker: Option<String>,
    name: Option<String>,
}

struct PendingBalance {
    label: &'static str,
    amount: Option<f64>,
    as_of: Option<String>,
}

impl PendingBalance {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            amount: None,
            as_of: None,
        }
    }

    fn finish(self) -> Option<Balance> {
        Some(Balance {
            label: self.label.into(),
            amount: self.amount?,
            as_of: self.as_of,
        })
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn number(value: &str) -> Option<f64> {
    value
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// `20240115120000[-5:EST]` or `20240115` as `2024-01-15`.
fn date(value: &str) -> Option<String> {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 8 {
        return None;
    }
    redact::parse_date(&format!(
        "{}-{}-{}",
        &digits[..4],
        &digits[4..6],
        &digits[6..8]
    ))
}

fn decode(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
