//! Redaction, parsing, classification and the document tools, over realistic
//! snippets (a 1040, a pay stub, statements, OFX, CSV).

use super::classify::{DocumentKind, classify};
use super::data::{AccountData, Balance, DocumentData, Position, Transaction};
use super::redact::{parse_birth_date, parse_date, redact};
use super::store::{Document, DocumentStatus};
use super::tools::{self, PositionStatus};
use super::{clean_filename, ingest, ofx, tabular};
use crate::compile::rows::ScenarioGraph;

fn masked(text: &str) -> String {
    redact(text).text
}

// ── redaction ───────────────────────────────────────────────────────────────

const FORM_1040: &str = "\
Form 1040 U.S. Individual Income Tax Return 2025
Filing Status: Married filing jointly
Your first name and middle initial Jordan A.  Last name Rivera
Your social security number 512-44-9021
Date of birth 03/14/1984
If joint return, spouse's first name Casey  Last name Rivera
Spouse's social security number 601 33 7788
Spouse's date of birth: March 2, 1986
Home address (number and street). If you have a P.O. box, see instructions.
2214 N. Larkspur Ave, Apt 3B
Denver, CO 80202
Presidential Election Campaign
Dependent 1: Riley Rivera, SSN 612-45-1234, relationship: Child, DOB 09/30/2015
1a Total amount from Form(s) W-2, box 1   1a  148,250.00
9 Add lines 1z, 2b, 3b, 4b, 5b, 6b, 7, and 8. This is your total income  9  151,004.00
11 Adjusted gross income  11  138,412.00
15 Taxable income  15  109,462.00
24 Total tax  24  16,320.00
";

#[test]
fn a_1040_is_masked_where_it_identifies_and_left_where_it_informs() {
    let r = redact(FORM_1040);
    let t = &r.text;
    for leaked in [
        "512-44-9021",
        "601 33 7788",
        "612-45-1234",
        "03/14/1984",
        "March 2, 1986",
        "09/30/2015",
        "2214 N. Larkspur Ave",
    ] {
        assert!(!t.contains(leaked), "{leaked} leaked:\n{t}");
    }
    assert!(t.contains("Your social security number •••-••-••••"));
    assert!(t.contains("Spouse's social security number •••-••-••••"));
    assert!(t.contains("SSN •••-••-••••"));
    assert!(t.contains("Date of birth [birth date removed]"));
    assert!(t.contains("[address removed]"));
    // What the plan needs stays: figures, filing status, state.
    for kept in [
        "148,250.00",
        "151,004.00",
        "138,412.00",
        "109,462.00",
        "16,320.00",
        "Married filing jointly",
        "Denver, CO",
        "Form 1040",
        "2025",
    ] {
        assert!(t.contains(kept), "{kept} was masked:\n{t}");
    }
    // The first birth date found is the hint; the rest are masked without one.
    assert_eq!(r.birth_date_hint.as_deref(), Some("1984-03-14"));
    assert_eq!(r.counts.birth_dates, 3);
    assert_eq!(r.counts.ssn, 3);
}

const PAY_STUB: &str = "\
ACME ROBOTICS INC   EIN 84-1234567
Earnings Statement          Pay Period: 09/01/2026 - 09/14/2026   Pay Date: 09/19/2026
Employee: Jordan Rivera     SSN: XXX-XX-9021    Employee ID 00458812
123 Main Street, Boulder, CO 80301
Description        Hours    Rate       Current       YTD
Regular Pay        80.00    68.27      5,461.54      136,538.50
401(k) Deferral                          -546.15     -13,653.85
Federal Withholding                      -812.33     -20,308.25
Gross Pay                                5,461.54    136,538.50
Net Pay                                  3,641.20     91,030.00
Direct Deposit: Routing Number 102000021  Account Number 000123456789
Checking ending 6789
";

#[test]
fn a_pay_stub_keeps_pay_figures_and_the_last_four_of_the_account() {
    let t = masked(PAY_STUB);
    assert!(!t.contains("84-1234567"), "{t}");
    assert!(t.contains("EIN ••-•••••••"));
    assert!(!t.contains("123 Main Street"));
    assert!(t.contains("Boulder, CO"));
    assert!(!t.contains("000123456789"));
    assert!(t.contains("Account Number ••••6789"), "{t}");
    assert!(t.contains("Routing Number ••••0021"), "{t}");
    assert!(t.contains("Checking ending 6789"));
    // A partly masked SSN is left as it came: it names nothing.
    assert!(t.contains("SSN: XXX-XX-9021"));
    for kept in [
        "5,461.54",
        "136,538.50",
        "-546.15",
        "80.00",
        "68.27",
        "09/01/2026 - 09/14/2026",
        "09/19/2026",
        "Employee ID 00458812",
    ] {
        assert!(t.contains(kept), "{kept} was masked:\n{t}");
    }
}

#[test]
fn statement_account_numbers_keep_only_their_last_four() {
    let text = "\
Chase Total Checking
Account Number: 000000123456789
Checking Summary  Statement Period 01/01/2026 through 01/31/2026
Beginning Balance $12,450.10
Ending Balance $13,004.55
Card 4111 1111 1111 1234 ending
Acct # 9876543210123 Routing: 021000021
Wire to account 1234567890 for $2,500.00
";
    let r = redact(text);
    let t = &r.text;
    assert!(t.contains("Account Number: ••••6789"), "{t}");
    assert!(t.contains("Acct # ••••0123"), "{t}");
    assert!(t.contains("Routing: ••••0021"), "{t}");
    assert!(t.contains("account ••••7890"), "{t}");
    assert!(t.contains("••••1234 ending"), "{t}");
    for gone in [
        "000000123456789",
        "9876543210123",
        "4111 1111",
        "1234567890",
    ] {
        assert!(!t.contains(gone), "{gone} leaked:\n{t}");
    }
    for kept in [
        "$12,450.10",
        "$13,004.55",
        "$2,500.00",
        "01/01/2026 through 01/31/2026",
        "Chase Total Checking",
    ] {
        assert!(t.contains(kept), "{kept} was masked:\n{t}");
    }
    assert!(r.counts.account_numbers >= 5);
}

#[test]
fn a_masked_number_is_left_masked() {
    let text = "Account Number: XXXXXX6789 and ••••1234 and Account ending in 4321.";
    let t = masked(text);
    assert!(t.contains("Account Number: ••••6789"), "{t}");
    assert!(t.contains("••••1234"));
    assert!(t.contains("Account ending in 4321"));
    // Idempotent.
    assert_eq!(masked(&t), t);
}

#[test]
fn nine_digits_are_an_ssn_or_routing_number_only_with_reason() {
    let t = masked("SSN 123456789 and Routing 021000021 and Balance 123456789 shares 987654321");
    assert!(t.contains("SSN •••-••-••••"));
    assert!(t.contains("Routing ••••0021"));
    // Bare nine-digit figures that fail the routing checksum are amounts.
    assert!(t.contains("Balance 123456789"), "{t}");
    assert!(t.contains("shares 987654321"), "{t}");
    // A bare number that passes the ABA checksum is a routing number.
    assert!(masked("Send to 021000021 today").contains("••••0021"));
}

#[test]
fn amounts_dates_and_counts_are_never_masked() {
    let text = "\
Deposit 01/15/2026 $1,234,567.89
Total 12345678901.50 and -$9,876,543.21
Ending balance 2026-01-31: 250000.00
Shares 1,234.5678 @ 708.6000 = 874,671.90
Ref 20260115120000 posted 20260115
Price on 3/4/25 was 1234567890.12
Phone-free: 555 shares of VTI, 2026 tax year, 401(k) limit 24500
Ratios 60/40 and 3-2-4 and 4-2-1 and 2024-01-15
Transaction #4471 at 11:45 PM, Fund $5 Way
";
    let t = masked(text);
    assert_eq!(t, text, "nothing here identifies anyone:\n{t}");
}

#[test]
fn birth_dates_are_masked_in_every_written_form_but_only_when_labelled() {
    for (text, iso) in [
        ("DOB: 07/04/1990", "1990-07-04"),
        ("D.O.B. 7/4/90", "1990-07-04"),
        ("Date of Birth (MM/DD/YYYY): 1990-07-04", "1990-07-04"),
        ("Birth date - July 4, 1990", "1990-07-04"),
        ("Born on 4 July 1990", "1990-07-04"),
        ("Birthdate: 04.07.1990", "1990-04-07"),
        ("DOB 31/12/1985", "1985-12-31"),
    ] {
        let r = redact(text);
        assert!(
            r.text.contains("[birth date removed]"),
            "{text} -> {}",
            r.text
        );
        assert_eq!(r.birth_date_hint.as_deref(), Some(iso), "{text}");
    }
    // The statement date next to nothing about birth is not a birth date.
    let r = redact("Statement date 07/04/1990 issued");
    assert_eq!(r.text, "Statement date 07/04/1990 issued");
    assert_eq!(r.birth_date_hint, None);
    // Nor is a word that only contains "dob".
    assert_eq!(
        masked("Dobson & Co paid 01/02/2020"),
        "Dobson & Co paid 01/02/2020"
    );
}

#[test]
fn street_addresses_and_boxes_go_but_amounts_next_to_words_do_not() {
    let t = masked(
        "Mail to: 1600 Pennsylvania Avenue NW\n77 Ocean View Drive Apt 4B\n\
         9 E Elm St, Suite 200\nP.O. Box 1234\nRent $1,200 Street parking $5 Ave. Fee 1,500 Court fees",
    );
    assert!(!t.contains("Pennsylvania"), "{t}");
    assert!(!t.contains("Ocean View"), "{t}");
    assert!(!t.contains("Elm St"), "{t}");
    assert!(!t.contains("Box 1234"), "{t}");
    assert!(t.contains("Rent $1,200 Street parking $5 Ave."), "{t}");
    assert!(t.contains("1,500 Court fees"), "{t}");
}

#[test]
fn email_and_phone_are_masked() {
    let t = masked("Call (303) 555-0142 or 720.555.0199, jordan.rivera+tax@example.com");
    assert!(!t.contains("555-0142") && !t.contains("555.0199") && !t.contains("@example"));
    assert!(t.contains("[phone removed]") && t.contains("[email removed]"));
}

#[test]
fn dates_parse_the_forms_statements_use() {
    assert_eq!(parse_date("03/14/1984").as_deref(), Some("1984-03-14"));
    assert_eq!(parse_date("1/5/24").as_deref(), Some("2024-01-05"));
    assert_eq!(parse_date("2024-02-29").as_deref(), Some("2024-02-29"));
    assert_eq!(parse_date("2023-02-29"), None);
    assert_eq!(parse_date("Jan 5, 2024").as_deref(), Some("2024-01-05"));
    assert_eq!(parse_date("15 August 2023").as_deref(), Some("2023-08-15"));
    assert_eq!(parse_date("13/45/2020"), None);
    assert_eq!(parse_birth_date("01/01/1850"), None);
}

// ── OFX ─────────────────────────────────────────────────────────────────────

const OFX_BANK_SGML: &str = "\
OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<SIGNONMSGSRSV1><SONRS><STATUS><CODE>0<SEVERITY>INFO</STATUS>
<DTSERVER>20260131120000<LANGUAGE>ENG
<FI><ORG>USAA<FID>67811</FI></SONRS></SIGNONMSGSRSV1>
<BANKMSGSRSV1><STMTTRNRS><TRNUID>1<STATUS><CODE>0<SEVERITY>INFO</STATUS>
<STMTRS><CURDEF>USD
<BANKACCTFROM><BANKID>314074269<ACCTID>00012345671234<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST><DTSTART>20260101<DTEND>20260131
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260105120000[0:GMT]<TRNAMT>-1450.00<FITID>1<NAME>RENT - SUNSET APTS<MEMO>Jan rent</STMTTRN>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20260115<TRNAMT>3641.20<FITID>2<NAME>ACME ROBOTICS PAYROLL</STMTTRN>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260118<TRNAMT>-82.15<FITID>3<NAME>KROGER #221 &amp; FUEL</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>12,004.55<DTASOF>20260131120000</LEDGERBAL>
<AVAILBAL><BALAMT>11900.00<DTASOF>20260131120000</AVAILBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>
";

#[test]
fn ofx_sgml_bank_statement_parses() {
    assert!(ofx::sniff(OFX_BANK_SGML.as_bytes()));
    let data = ofx::parse(OFX_BANK_SGML).expect("statement");
    assert_eq!(data.institution.as_deref(), Some("USAA"));
    let [account] = data.accounts.as_slice() else {
        panic!("one account");
    };
    assert_eq!(account.last4.as_deref(), Some("1234"));
    assert_eq!(account.kind.as_deref(), Some("checking"));
    assert_eq!(account.currency.as_deref(), Some("USD"));
    assert_eq!(
        account.balances,
        vec![
            Balance {
                label: "ledger".into(),
                amount: 12004.55,
                as_of: Some("2026-01-31".into())
            },
            Balance {
                label: "available".into(),
                amount: 11900.0,
                as_of: Some("2026-01-31".into())
            },
        ]
    );
    assert_eq!(account.headline_balance(), Some(12004.55));
    assert_eq!(account.transactions.len(), 3);
    assert_eq!(account.transactions[0].date, "2026-01-05");
    assert_eq!(account.transactions[0].amount, -1450.0);
    assert_eq!(
        account.transactions[0].description,
        "RENT - SUNSET APTS Jan rent"
    );
    assert_eq!(account.transactions[2].description, "KROGER #221 & FUEL");
}

const OFX_INVEST_XML: &str = r#"<?xml version="1.0"?>
<?OFX OFXHEADER="200" VERSION="220"?>
<OFX>
<SIGNONMSGSRSV1><SONRS><FI><ORG>Fidelity</ORG></FI></SONRS></SIGNONMSGSRSV1>
<INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS>
<CURDEF>USD</CURDEF>
<INVACCTFROM><BROKERID>fidelity.com</BROKERID><ACCTID>Z12345678</ACCTID></INVACCTFROM>
<INVPOSLIST>
<POSMF><INVPOS><SECID><UNIQUEID>315911750</UNIQUEID><UNIQUEIDTYPE>CUSIP</UNIQUEIDTYPE></SECID>
<HELDINACCT>CASH</HELDINACCT><POSTYPE>LONG</POSTYPE><UNITS>123.4560</UNITS><UNITPRICE>708.60</UNITPRICE><MKTVAL>87,477.40</MKTVAL></INVPOS></POSMF>
<POSSTOCK><INVPOS><SECID><UNIQUEID>02079K305</UNIQUEID><UNIQUEIDTYPE>CUSIP</UNIQUEIDTYPE></SECID>
<UNITS>10</UNITS><UNITPRICE>337.12</UNITPRICE><MKTVAL>3371.20</MKTVAL></INVPOS></POSSTOCK>
</INVPOSLIST>
<INVBAL><AVAILCASH>1500.25</AVAILCASH></INVBAL>
</INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1>
<SECLISTMSGSRSV1><SECLIST>
<MFINFO><SECINFO><SECID><UNIQUEID>315911750</UNIQUEID><UNIQUEIDTYPE>CUSIP</UNIQUEIDTYPE></SECID><SECNAME>VANGUARD 500 INDEX ADMIRAL</SECNAME><TICKER>VFIAX</TICKER></SECINFO></MFINFO>
<STOCKINFO><SECINFO><SECID><UNIQUEID>02079K305</UNIQUEID><UNIQUEIDTYPE>CUSIP</UNIQUEIDTYPE></SECID><SECNAME>ALPHABET INC CL C</SECNAME><TICKER>GOOG</TICKER></SECINFO></STOCKINFO>
</SECLIST></SECLISTMSGSRSV1>
</OFX>"#;

#[test]
fn ofx_xml_investment_statement_reads_positions_named_from_the_security_list() {
    let data = ofx::parse(OFX_INVEST_XML).unwrap();
    let account = &data.accounts[0];
    assert_eq!(account.last4.as_deref(), Some("5678"));
    assert_eq!(
        account.positions,
        vec![
            Position {
                symbol: Some("VFIAX".into()),
                name: Some("VANGUARD 500 INDEX ADMIRAL".into()),
                units: 123.456,
                unit_price: Some(708.6),
                market_value: Some(87477.4)
            },
            Position {
                symbol: Some("GOOG".into()),
                name: Some("ALPHABET INC CL C".into()),
                units: 10.0,
                unit_price: Some(337.12),
                market_value: Some(3371.2)
            },
        ]
    );
    assert_eq!(account.cash_balance(), Some(1500.25));
    // No labelled total: positions plus cash.
    let total = account.headline_balance().unwrap();
    assert!((total - (87477.4 + 3371.2 + 1500.25)).abs() < 1e-6);
}

#[test]
fn a_file_with_no_statement_is_not_ofx_data() {
    assert!(ofx::parse("<OFX><SIGNONMSGSRSV1></SIGNONMSGSRSV1></OFX>").is_none());
    assert!(ofx::parse("just words").is_none());
    assert!(!ofx::sniff(b"date,amount\n"));
}

#[test]
fn an_ofx_upload_stores_masked_text_and_structured_data() {
    let ingested = ingest("usaa_jan.qfx", OFX_BANK_SGML.as_bytes()).unwrap();
    assert_eq!(ingested.kind, DocumentKind::BankStatement);
    assert_eq!(ingested.status, DocumentStatus::Parsed);
    assert!(!ingested.hold_original);
    let text = ingested.pages.join("\n");
    assert!(text.contains("Institution: USAA"));
    assert!(text.contains("••••1234"));
    assert!(!text.contains("00012345671234"));
    assert!(text.contains("ledger balance: 12004.55 as of 2026-01-31"));
    assert!(text.contains("2026-01-05 | -1450.00 | RENT - SUNSET APTS Jan rent"));
}

#[test]
fn ofx_transaction_memos_and_file_names_are_redacted_before_storage() {
    let ofx = OFX_BANK_SGML.replace(
        "<MEMO>Jan rent",
        "<MEMO>ACH to acct 987654321012 SSN 123-45-6789",
    );
    let ingested = ingest("stmt_123-45-6789.qfx", ofx.as_bytes()).unwrap();
    let text = ingested.pages.join("\n");
    let data = serde_json::to_string(&ingested.data).unwrap();
    for stored in [&text, &data] {
        assert!(!stored.contains("987654321012"), "{stored}");
        assert!(!stored.contains("123-45-6789"), "{stored}");
    }
    assert!(text.contains("••••1012"), "{text}");
    assert_eq!(
        clean_filename("stmt_123-45-6789.qfx"),
        "stmt •••-••-••••.qfx"
    );
}

// ── CSV ─────────────────────────────────────────────────────────────────────

#[test]
fn csv_rows_handle_quotes_delimiters_and_a_bom() {
    let rows = tabular::parse_rows(
        "\u{feff}Date,Description,Amount\r\n01/02/2026,\"COSTCO, WHSE #12\",-154.20\r\n\
         01/03/2026,\"SAY \"\"HI\"\" CAFE\",-6.50\r\n\r\n",
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1][1], "COSTCO, WHSE #12");
    assert_eq!(rows[2][1], "SAY \"HI\" CAFE");
    let semi = tabular::parse_rows("a;b;c\n1;2;3\n");
    assert_eq!(semi[1], ["1", "2", "3"]);
    let tab = tabular::parse_rows("a\tb\n1\t2\n");
    assert_eq!(tab[0], ["a", "b"]);
}

#[test]
fn a_transaction_csv_becomes_transactions_with_debit_credit_columns_too() {
    let csv = "Posting Date,Description,Debit,Credit,Balance\n\
               01/05/2026,RENT SUNSET APTS,1450.00,,9000.00\n\
               01/15/2026,PAYROLL ACME,,3641.20,12641.20\n\
               not a date,junk,1,,\n";
    let ingested = ingest("jan.csv", csv.as_bytes()).unwrap();
    assert_eq!(ingested.kind, DocumentKind::Transactions);
    let data = ingested.data.unwrap();
    assert_eq!(
        data.accounts[0].transactions,
        vec![
            Transaction {
                date: "2026-01-05".into(),
                amount: -1450.0,
                description: "RENT SUNSET APTS".into()
            },
            Transaction {
                date: "2026-01-15".into(),
                amount: 3641.2,
                description: "PAYROLL ACME".into()
            },
        ]
    );
    assert_eq!(ingested.pages.len(), 1);
    assert!(ingested.pages[0].starts_with("Posting Date | Description | Debit"));
}

#[test]
fn a_long_csv_pages_with_its_header_on_every_page() {
    let mut csv = String::from("Date,Description,Amount\n");
    for i in 0..250 {
        csv.push_str(&format!(
            "01/{:02}/2026,ROW {i},-{}.00\n",
            1 + i % 28,
            10 + i
        ));
    }
    let ingested = ingest("big.csv", csv.as_bytes()).unwrap();
    assert_eq!(ingested.pages.len(), 3);
    assert!(
        ingested
            .pages
            .iter()
            .all(|p| p.starts_with("Date | Description | Amount"))
    );
    assert_eq!(ingested.page_units, 1, "a text file counts one page");
}

#[test]
fn a_holdings_csv_becomes_positions() {
    let csv = "Symbol,Description,Quantity,Price,Current Value\n\
               VTI,VANGUARD TOTAL STOCK MKT,120.5,\"$310.25\",\"$37,385.13\"\n\
               BND,VANGUARD TOTAL BOND,80,72.10,5768.00\n";
    let ingested = ingest("holdings.csv", csv.as_bytes()).unwrap();
    assert_eq!(ingested.kind, DocumentKind::BrokerageStatement);
    let positions = &ingested.data.unwrap().accounts[0].positions;
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].symbol.as_deref(), Some("VTI"));
    assert_eq!(positions[0].units, 120.5);
    assert_eq!(positions[0].market_value, Some(37385.13));
}

#[test]
fn csv_text_is_redacted_before_it_is_stored() {
    let csv = "Date,Description,Amount\n01/02/2026,ZELLE TO 512-44-9021,-40.00\n";
    let ingested = ingest("x.csv", csv.as_bytes()).unwrap();
    assert!(!ingested.pages.join("").contains("512-44-9021"));
    let d = &ingested.data.unwrap().accounts[0].transactions[0].description;
    assert!(!d.contains("512-44-9021"), "{d}");
}

// ── other formats and classification ────────────────────────────────────────

#[test]
fn plain_text_is_redacted_paged_and_classified() {
    let ingested = ingest("Paystub_Sept.txt", PAY_STUB.as_bytes()).unwrap();
    assert_eq!(ingested.kind, DocumentKind::PayStub);
    assert_eq!(ingested.status, DocumentStatus::Parsed);
    assert!(!ingested.pages.join("").contains("000123456789"));
    let tax = ingest("return.txt", FORM_1040.as_bytes()).unwrap();
    assert_eq!(tax.kind, DocumentKind::TaxReturn);
    assert_eq!(tax.birth_date_hint.as_deref(), Some("1984-03-14"));
}

#[test]
fn documents_are_told_apart() {
    let bank = "Chase Total Checking\nStatement Period 01/01 - 01/31\nBeginning Balance $1\nEnding Balance $2\nDeposits and Additions\nWithdrawals\nAvailable balance";
    assert_eq!(classify("stmt.pdf", bank), DocumentKind::BankStatement);
    let retirement = "Fidelity NetBenefits 401(k) Plan\nVested balance $80,000\nEmployer match $3,000\nEmployee deferral";
    assert_eq!(
        classify("q4.pdf", retirement),
        DocumentKind::RetirementStatement
    );
    let brokerage = "Vanguard Brokerage Account\nHoldings\nSymbol Shares Market Value\nCost basis  Unrealized gain\nDividends";
    assert_eq!(
        classify("x.pdf", brokerage),
        DocumentKind::BrokerageStatement
    );
    assert_eq!(
        classify("2025_1040.pdf", ""),
        DocumentKind::TaxReturn,
        "the filename is enough"
    );
    assert_eq!(classify("scan0001.pdf", ""), DocumentKind::Other);
    assert_eq!(
        classify("2025_1040.pdf", "Adjusted gross income"),
        DocumentKind::TaxReturn
    );
    assert_eq!(
        classify("notes.txt", "groceries: milk"),
        DocumentKind::Other
    );
}

#[test]
fn unsupported_and_unreadable_files() {
    assert!(ingest("archive.zip", b"PK\x03\x04....").is_err());
    assert!(ingest("sheet.xlsx", b"PK\x03\x04....").is_err());
    let bad = ingest("broken.pdf", b"%PDF-1.4 not really").unwrap();
    assert_eq!(bad.status, DocumentStatus::Failed);
    assert!(bad.note.unwrap().contains("could not be read"));
}

#[test]
fn images_are_held_not_read() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0; 32]);
    let i = ingest("shot.png", &png).unwrap();
    assert_eq!(i.kind, DocumentKind::Image);
    assert_eq!(i.status, DocumentStatus::ImageUnredacted);
    assert!(i.hold_original);
    assert_eq!(i.page_units, 1);
    assert_eq!(i.pages, vec![String::new()]);
    let mut heic = vec![0, 0, 0, 24];
    heic.extend_from_slice(b"ftypheic");
    heic.extend_from_slice(&[0; 16]);
    assert_eq!(ingest("IMG_1.HEIC", &heic).unwrap().mime, "image/heic");
    let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpg.extend_from_slice(&[0; 16]);
    assert_eq!(ingest("a.jpg", &jpg).unwrap().mime, "image/jpeg");
}

/// A minimal one-page-per-string PDF with a text layer, one line per `\n`.
pub(crate) fn pdf_with_pages(pages: &[&str]) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for text in pages {
        let mut ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 10.into()]),
            Operation::new("TL", vec![14.into()]),
            Operation::new("Td", vec![50.into(), 750.into()]),
        ];
        for line in text.lines() {
            ops.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            ops.push(Operation::new("T*", vec![]));
        }
        ops.push(Operation::new("ET", vec![]));
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        });
        kids.push(Object::from(page));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[test]
fn a_pdf_with_a_text_layer_is_read_page_by_page_and_redacted() {
    let pdf = pdf_with_pages(&[
        "Fidelity Investments\nBrokerage Account Statement\nAccount Number: 123456789\nTotal Account Value $52,310.44 as of 01/31/2026\nHoldings Symbol Shares Market Value\nCost basis unrealized",
        "SSN 512-44-9021\nDate of birth 03/14/1984\nDividends $12.00",
    ]);
    let i = ingest("stmt.pdf", &pdf).unwrap();
    assert_eq!(i.status, DocumentStatus::Parsed, "{:?}", i.note);
    assert_eq!(i.pages.len(), 2);
    assert_eq!(i.page_units, 2);
    assert!(i.pages[0].contains("Fidelity Investments"), "{:?}", i.pages);
    assert!(
        i.pages[0].contains("Account Number: ••••6789"),
        "{:?}",
        i.pages
    );
    assert!(!i.pages[1].contains("512-44-9021"));
    assert_eq!(i.birth_date_hint.as_deref(), Some("1984-03-14"));
    assert_eq!(i.kind, DocumentKind::BrokerageStatement);
    let data = i.data.expect("hints");
    assert_eq!(data.institution.as_deref(), Some("Fidelity"));
    assert_eq!(data.accounts[0].last4.as_deref(), Some("6789"));
    assert_eq!(data.accounts[0].balances[0].label, "total");
    assert_eq!(data.accounts[0].balances[0].amount, 52310.44);
    assert_eq!(
        data.accounts[0].balances[0].as_of.as_deref(),
        Some("2026-01-31")
    );
}

#[test]
fn a_pdf_page_without_text_is_noted_and_a_textless_pdf_is_held() {
    let blank = pdf_with_pages(&["", ""]);
    let i = ingest("scan.pdf", &blank).unwrap();
    assert_eq!(i.status, DocumentStatus::NeedsOcr);
    assert!(i.hold_original);
    assert_eq!(i.page_units, 2);

    let mixed = pdf_with_pages(&["Ending balance $10.00", ""]);
    let i = ingest("mixed.pdf", &mixed).unwrap();
    assert_eq!(i.status, DocumentStatus::Parsed);
    assert!(i.note.unwrap().contains("1 of 2 pages"));
    assert!(!i.hold_original);
}

// ── tools ───────────────────────────────────────────────────────────────────

fn doc(kind: DocumentKind, data: DocumentData, text: &str) -> Document {
    Document {
        id: 7,
        scenario_id: 1,
        filename: "doc".into(),
        mime: "text/plain".into(),
        sha256: "00".into(),
        pages: 1,
        kind,
        status: DocumentStatus::Parsed,
        note: None,
        birth_date_hint: None,
        text: text.into(),
        data,
    }
}

fn txn(date: &str, amount: f64, description: &str) -> Transaction {
    Transaction {
        date: date.into(),
        amount,
        description: description.into(),
    }
}

fn transactions_doc(txns: Vec<Transaction>) -> Document {
    doc(
        DocumentKind::Transactions,
        DocumentData {
            institution: None,
            accounts: vec![AccountData {
                transactions: txns,
                ..AccountData::default()
            }],
        },
        "",
    )
}

/// Six full months of steady spending, with a flight in August.
fn six_months() -> Document {
    let mut t = Vec::new();
    for m in 3..=8 {
        let mm = format!("2026-{m:02}");
        t.push(txn(&format!("{mm}-01"), -1450.0, "RENT SUNSET APTS"));
        t.push(txn(&format!("{mm}-15"), 5200.0, "ACME ROBOTICS PAYROLL"));
        t.push(txn(&format!("{mm}-28"), 5200.0, "ACME ROBOTICS PAYROLL"));
        for (d, a) in [(4, 92.0), (11, 78.5), (19, 101.0), (26, 84.0)] {
            t.push(txn(&format!("{mm}-{d:02}"), -a, "KROGER #221"));
        }
        for (d, a) in [(6, 32.0), (14, 41.0), (22, 28.0)] {
            t.push(txn(&format!("{mm}-{d:02}"), -a, "CHIPOTLE 1123"));
        }
        t.push(txn(&format!("{mm}-20"), -1500.0, "TRANSFER TO SAVINGS"));
    }
    t.push(txn("2026-08-09", -1380.0, "DELTA AIR LINES 0062"));
    t.push(txn("2026-04-12", -320.0, "DELTA AIR LINES 0062"));
    t.push(txn("2026-06-03", -410.0, "MARRIOTT HOTEL"));
    t.push(txn("2026-07-03", -390.0, "HILTON HOTEL"));
    transactions_doc(t)
}

#[test]
fn transactions_summarize_by_month_and_category_with_outliers() {
    let s = tools::summarize_transactions(&six_months(), None);
    assert_eq!(s.document_id, 7);
    assert_eq!(s.months.len(), 6);
    assert_eq!(s.first_date.as_deref(), Some("2026-03-01"));
    assert_eq!(
        s.transfers_excluded, 6,
        "savings transfers are neither spending nor income"
    );
    let march = &s.months[0];
    assert_eq!(march.month, "2026-03");
    assert_eq!(march.income, 10400.0);
    // Rent 1450 + groceries 355.5 + dining 101.
    assert_eq!(march.spending, 1906.5);
    assert!(!march.partial);
    let groceries = march
        .by_category
        .iter()
        .find(|c| c.category == "groceries")
        .unwrap();
    assert_eq!((groceries.total, groceries.count), (355.5, 4));
    let august = s.months.iter().find(|m| m.month == "2026-08").unwrap();
    assert_eq!(august.spending, 1906.5 + 1380.0);

    // The August flight stands out from the other travel.
    assert_eq!(s.outliers.len(), 1, "{:?}", s.outliers);
    let o = &s.outliers[0];
    assert_eq!(
        (o.date.as_str(), o.category.as_str(), o.amount),
        ("2026-08-09", "travel", 1380.0)
    );
    assert_eq!(o.category_median, 400.0);
    assert!(o.ratio > tools::OUTLIER_RATIO);

    // Averages are over the six full months.
    let total: f64 = s.months.iter().map(|m| m.spending).sum();
    assert!((s.average_monthly_spending - (total / 6.0 * 100.0).round() / 100.0).abs() < 0.01);
    assert_eq!(s.average_monthly_income, 10400.0);
    assert_eq!(s.category_averages[0].category, "housing");
}

#[test]
fn months_limits_the_summary_to_the_latest_calendar_months() {
    let s = tools::summarize_transactions(&six_months(), Some(2));
    let months: Vec<&str> = s.months.iter().map(|m| m.month.as_str()).collect();
    assert_eq!(months, ["2026-07", "2026-08"]);
}

#[test]
fn a_month_the_statement_only_partly_covers_is_marked_and_left_out_of_averages() {
    let t = vec![
        txn("2026-01-20", -100.0, "KROGER"),
        txn("2026-02-01", -300.0, "KROGER"),
        txn("2026-02-27", -300.0, "KROGER"),
        txn("2026-03-01", -300.0, "KROGER"),
        txn("2026-03-29", -300.0, "KROGER"),
        txn("2026-04-02", -50.0, "KROGER"),
    ];
    let s = tools::summarize_transactions(&transactions_doc(t), None);
    let partial: Vec<bool> = s.months.iter().map(|m| m.partial).collect();
    assert_eq!(partial, [true, false, false, true]);
    assert_eq!(s.average_monthly_spending, 600.0);
}

#[test]
fn refunds_net_against_spending_and_nothing_flags_without_a_sample() {
    let t = vec![
        txn("2026-01-05", -200.0, "AMAZON MKTP"),
        txn("2026-01-09", 50.0, "AMAZON MKTP REFUND"),
        txn("2026-01-10", -900.0, "BEST BUY"),
    ];
    let s = tools::summarize_transactions(&transactions_doc(t), None);
    assert_eq!(s.months[0].spending, 1050.0);
    assert!(
        s.outliers.is_empty(),
        "two purchases are too few to call one unusual"
    );
    assert_eq!(tools::categorize("PAYROLL", 100.0), "income");
    assert_eq!(tools::categorize("MYSTERY", 100.0), "income");
    assert_eq!(tools::categorize("MYSTERY", -100.0), "other");
}

fn graph() -> ScenarioGraph {
    serde_json::from_str(include_str!(
        "../../../finplan_plan/testdata/default_snapshot.json"
    ))
    .unwrap()
}

fn statement(kind: DocumentKind, institution: &str, account: AccountData, text: &str) -> Document {
    doc(
        kind,
        DocumentData {
            institution: Some(institution.into()),
            accounts: vec![account],
        },
        text,
    )
}

#[test]
fn a_bank_statement_matches_the_account_by_institution_and_balance() {
    let d = statement(
        DocumentKind::BankStatement,
        "USAA",
        AccountData {
            last4: Some("1234".into()),
            balances: vec![Balance {
                label: "ledger".into(),
                amount: 251_000.0,
                as_of: None,
            }],
            ..AccountData::default()
        },
        "USAA Federal Savings Bank",
    );
    let m = tools::match_account(&d, &graph());
    let account = &m.accounts[0];
    assert_eq!(account.best_account_id, Some(6));
    let top = &account.candidates[0];
    assert_eq!((top.account_id, top.name.as_str()), (6, "USAA"));
    assert!(top.score >= 0.7, "{top:?}");
    assert!(top.reasons.iter().any(|r| r.contains("institution USAA")));
    assert!(top.reasons.iter().any(|r| r.contains("within 0.5%")));
    assert_eq!(top.plan_value, 250_000.0);
    // Nothing else comes close.
    assert!(
        account
            .candidates
            .iter()
            .skip(1)
            .all(|c| c.score < top.score - 0.15)
    );
}

#[test]
fn an_ambiguous_statement_names_candidates_but_no_best() {
    // "Vanguard" is the name of two plan accounts; nothing else separates them.
    let d = statement(
        DocumentKind::BrokerageStatement,
        "Vanguard",
        AccountData::default(),
        "",
    );
    let m = tools::match_account(&d, &graph());
    let ids: Vec<i64> = m.accounts[0]
        .candidates
        .iter()
        .map(|c| c.account_id)
        .collect();
    assert!(ids.contains(&1) && ids.contains(&2), "{ids:?}");
    assert_eq!(m.accounts[0].best_account_id, None);
}

#[test]
fn a_statement_matching_nothing_has_no_candidates() {
    let d = statement(
        DocumentKind::BankStatement,
        "Zeta Credit Union",
        AccountData {
            balances: vec![Balance {
                label: "ledger".into(),
                amount: 17.0,
                as_of: None,
            }],
            ..AccountData::default()
        },
        "",
    );
    let m = tools::match_account(&d, &graph());
    assert!(m.accounts[0].candidates.is_empty());
    let r = tools::reconcile(&d, &graph());
    assert_eq!(r.accounts[0].account_id, None);
}

#[test]
fn reconcile_diffs_balance_and_positions_against_the_plan_account() {
    // Plan: Vanguard Roth IRA holds 900 VFIAX @ 708.60 and 1000 VFIFX @ 66.83.
    let d = statement(
        DocumentKind::RetirementStatement,
        "Vanguard",
        AccountData {
            balances: vec![Balance {
                label: "cash".into(),
                amount: 12.0,
                as_of: Some("2026-01-31".into()),
            }],
            positions: vec![
                Position {
                    symbol: Some("VFIAX".into()),
                    name: Some("Vanguard 500 Index Admiral".into()),
                    units: 950.0,
                    unit_price: Some(708.6),
                    market_value: Some(673_170.0),
                },
                Position {
                    symbol: Some("VFIFX".into()),
                    name: None,
                    units: 1000.0,
                    unit_price: Some(66.83),
                    market_value: Some(66_830.0),
                },
                Position {
                    symbol: Some("VTSAX".into()),
                    name: None,
                    units: 10.0,
                    unit_price: Some(140.0),
                    market_value: Some(1_400.0),
                },
            ],
            ..AccountData::default()
        },
        "Vanguard Roth IRA statement",
    );
    let r = tools::reconcile(&d, &graph());
    let a = &r.accounts[0];
    assert_eq!(a.account_id, Some(2), "{a:?}");
    assert_eq!(a.account_name.as_deref(), Some("Vanguard Roth IRA"));
    let by_symbol = |s: &str| {
        a.positions
            .iter()
            .find(|p| p.symbol.as_deref() == Some(s))
            .unwrap()
    };
    let vfiax = by_symbol("VFIAX");
    assert_eq!(vfiax.status, PositionStatus::UnitsDiffer);
    assert_eq!(vfiax.units_difference, Some(50.0));
    assert_eq!(vfiax.plan_units, Some(900.0));
    assert_eq!(by_symbol("VFIFX").status, PositionStatus::Matches);
    assert_eq!(by_symbol("VTSAX").status, PositionStatus::MissingInPlan);
    let balance = a.balance.as_ref().unwrap();
    assert_eq!(balance.plan, 704_570.0);
    assert_eq!(balance.document, 673_170.0 + 66_830.0 + 1_400.0 + 12.0);
    assert!(balance.significant);
    assert_eq!(balance.as_of.as_deref(), Some("2026-01-31"));
    assert_eq!(a.cash.as_ref().unwrap().plan, 0.0);
    assert_eq!(a.cash.as_ref().unwrap().difference, 12.0);
}

#[test]
fn a_plan_holding_the_statement_lacks_is_reported() {
    let d = statement(
        DocumentKind::RetirementStatement,
        "Vanguard",
        AccountData {
            positions: vec![Position {
                symbol: Some("VFIAX".into()),
                name: None,
                units: 900.0,
                unit_price: None,
                market_value: Some(637_740.0),
            }],
            ..AccountData::default()
        },
        "Vanguard Roth IRA",
    );
    let r = tools::reconcile(&d, &graph());
    let a = &r.accounts[0];
    assert_eq!(a.account_id, Some(2));
    let missing: Vec<&PositionStatus> = a.positions.iter().map(|p| &p.status).collect();
    assert!(
        missing.contains(&&PositionStatus::MissingInDocument),
        "{:?}",
        a.positions
    );
    assert!(missing.contains(&&PositionStatus::Matches));
}
