//! What kind of document an upload is, from its text and filename: keyword
//! scores per kind, the best one winning above a floor.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DocumentKind {
    BankStatement,
    BrokerageStatement,
    RetirementStatement,
    PayStub,
    TaxReturn,
    /// A transaction export (CSV/OFX) with no statement around it.
    Transactions,
    Image,
    Other,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentKind::BankStatement => "bank_statement",
            DocumentKind::BrokerageStatement => "brokerage_statement",
            DocumentKind::RetirementStatement => "retirement_statement",
            DocumentKind::PayStub => "pay_stub",
            DocumentKind::TaxReturn => "tax_return",
            DocumentKind::Transactions => "transactions",
            DocumentKind::Image => "image",
            DocumentKind::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "bank_statement" => DocumentKind::BankStatement,
            "brokerage_statement" => DocumentKind::BrokerageStatement,
            "retirement_statement" => DocumentKind::RetirementStatement,
            "pay_stub" => DocumentKind::PayStub,
            "tax_return" => DocumentKind::TaxReturn,
            "transactions" => DocumentKind::Transactions,
            "image" => DocumentKind::Image,
            "other" => DocumentKind::Other,
            _ => return None,
        })
    }
}

const TAX_RETURN: &[&str] = &[
    "form 1040",
    "1040-sr",
    "individual income tax return",
    "adjusted gross income",
    "taxable income",
    "schedule c",
    "schedule 1",
    "wages, salaries, tips",
    "standard deduction",
    "filing status",
    "total tax",
    "form w-2",
    "wage and tax statement",
];
const PAY_STUB: &[&str] = &[
    "gross pay",
    "net pay",
    "earnings statement",
    "pay period",
    "pay date",
    "ytd",
    "year to date",
    "federal withholding",
    "fed tax",
    "pay stub",
    "paystub",
    "hours worked",
    "regular pay",
    "direct deposit",
];
const RETIREMENT: &[&str] = &[
    "401(k)",
    "401k",
    "403(b)",
    "457(b)",
    "roth ira",
    "traditional ira",
    "rollover ira",
    "retirement",
    "vested balance",
    "employer match",
    "employee deferral",
    "employer contribution",
    "beneficiary",
    "plan balance",
];
const BROKERAGE: &[&str] = &[
    "brokerage",
    "holdings",
    "positions",
    "securities",
    "cost basis",
    "unrealized",
    "portfolio",
    "shares",
    "cusip",
    "symbol",
    "market value",
    "dividends",
    "asset allocation",
];
const BANK: &[&str] = &[
    "checking",
    "savings",
    "beginning balance",
    "ending balance",
    "deposits",
    "withdrawals",
    "statement period",
    "routing",
    "available balance",
    "overdraft",
    "debit card",
    "account summary",
];

fn score(haystack: &str, words: &[&str]) -> usize {
    words.iter().filter(|w| haystack.contains(**w)).count()
}

/// Classify a text document. `filename` counts as a hint; the text is looked
/// at through its first 30,000 characters.
pub fn classify(filename: &str, text: &str) -> DocumentKind {
    let head: String = text.chars().take(30_000).collect::<String>().to_lowercase();
    let name = filename.to_lowercase();
    let hay = format!("{name}\n{head}");
    let mut scored = [
        (DocumentKind::TaxReturn, score(&hay, TAX_RETURN)),
        (DocumentKind::PayStub, score(&hay, PAY_STUB)),
        (DocumentKind::RetirementStatement, score(&hay, RETIREMENT)),
        (DocumentKind::BrokerageStatement, score(&hay, BROKERAGE)),
        (DocumentKind::BankStatement, score(&hay, BANK)),
    ];
    // Filenames say it plainly.
    let by_name = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if by_name(&["1040", "w2", "w-2", "taxreturn", "tax_return", "tax-return"]) {
        scored[0].1 += 3;
    }
    if by_name(&["paystub", "pay_stub", "pay-stub", "payslip", "earnings"]) {
        scored[1].1 += 3;
    }
    if by_name(&["401k", "401(k)", "ira", "retirement", "403b"]) {
        scored[2].1 += 3;
    }
    if by_name(&["brokerage", "portfolio", "holdings", "positions"]) {
        scored[3].1 += 3;
    }
    if by_name(&["checking", "savings", "bank"]) {
        scored[4].1 += 3;
    }
    // An investment statement mentioning shares is not a retirement one unless
    // the retirement words lead, so ties go to the more specific kind (earlier).
    scored
        .iter()
        .filter(|(_, s)| *s >= 2)
        .max_by(|a, b| a.1.cmp(&b.1).then(std::cmp::Ordering::Greater))
        .map(|(k, _)| *k)
        .unwrap_or(DocumentKind::Other)
}
