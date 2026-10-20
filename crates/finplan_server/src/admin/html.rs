//! Rendering for the admin pages: escaping, the page frame, tables and the
//! few number formats the pages share. No templates; every page is a string.

use std::fmt::{self, Display, Write as _};

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// Text escaped for HTML content and double-quoted attribute values.
pub struct Esc<'a>(&'a str);

impl Display for Esc<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.0.chars() {
            match c {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&#39;")?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

pub fn esc(text: &str) -> Esc<'_> {
    Esc(text)
}

/// An optional value, escaped, or a dash.
pub fn opt(text: Option<&str>) -> String {
    match text {
        Some(text) if !text.is_empty() => esc(text).to_string(),
        _ => "<span class=muted>—</span>".into(),
    }
}

/// A path segment, percent-encoded: user ids go into links and redirects.
pub fn seg(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// A link to a user's page, by email; the user may since have been deleted.
pub fn user_link(id: Option<&str>, email: Option<&str>) -> String {
    match (id, email) {
        (Some(id), Some(email)) => format!("<a href=\"/users/{}\">{}</a>", seg(id), esc(email)),
        _ => "<span class=muted>(deleted user)</span>".into(),
    }
}

/// An integer with thousands separators.
pub fn count(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// US dollars: cents above a dollar, four places below, where a single model
/// pass usually lands.
pub fn money(usd: f64) -> String {
    if usd == 0.0 {
        "$0.00".into()
    } else if usd.abs() >= 1.0 {
        let cents = (usd * 100.0).round() as i64;
        format!("${}.{:02}", count(cents / 100), (cents % 100).abs())
    } else {
        format!("${usd:.4}")
    }
}

/// US dollars to the cent, for totals; an amount too small to show as a
/// cent says so rather than reading as nothing.
pub fn cents(usd: f64) -> String {
    if usd > 0.0 && usd < 0.005 {
        "&lt;$0.01".into()
    } else if usd.abs() >= 1.0 {
        money(usd)
    } else {
        format!("${usd:.2}")
    }
}

pub fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// A table column; numeric columns align right.
pub struct Col(pub &'static str, pub bool);

pub const fn col(name: &'static str) -> Col {
    Col(name, false)
}

pub const fn num(name: &'static str) -> Col {
    Col(name, true)
}

/// A table of already-rendered cells, or `empty` when there are no rows.
pub fn table(cols: &[Col], rows: impl IntoIterator<Item = Vec<String>>, empty: &str) -> String {
    let mut body = String::new();
    for row in rows {
        body.push_str("<tr>");
        for (cell, Col(_, numeric)) in row.iter().zip(cols) {
            if *numeric {
                let _ = write!(body, "<td class=n>{cell}</td>");
            } else {
                let _ = write!(body, "<td>{cell}</td>");
            }
        }
        body.push_str("</tr>");
    }
    if body.is_empty() {
        return format!("<p class=muted>{}</p>", esc(empty));
    }
    let mut out = String::from("<div class=scroll><table><thead><tr>");
    for Col(name, numeric) in cols {
        let class = if *numeric { " class=n" } else { "" };
        let _ = write!(out, "<th{class}>{}</th>", esc(name));
    }
    let _ = write!(out, "</tr></thead><tbody>{body}</tbody></table></div>");
    out
}

/// Label/value pairs, already rendered.
pub fn facts(pairs: &[(&str, String)]) -> String {
    let mut out = String::from("<dl class=facts>");
    for (label, value) in pairs {
        let _ = write!(out, "<dt>{}</dt><dd>{value}</dd>", esc(label));
    }
    out.push_str("</dl>");
    out
}

/// A row of headline figures.
pub fn tiles(pairs: &[(&str, String)]) -> String {
    let mut out = String::from("<div class=tiles>");
    for (label, value) in pairs {
        let _ = write!(
            out,
            "<div class=tile><div class=label>{}</div><div class=value>{value}</div></div>",
            esc(label)
        );
    }
    out.push_str("</div>");
    out
}

/// Which navigation entry a page sits under.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Overview,
    Users,
    Usage,
    Subscriptions,
    Inbox,
    None,
}

const NAV: [(Nav, &str, &str); 5] = [
    (Nav::Overview, "/", "Overview"),
    (Nav::Users, "/users", "Users"),
    (Nav::Usage, "/usage", "AI spend"),
    (Nav::Subscriptions, "/subscriptions", "Subscriptions"),
    (Nav::Inbox, "/inbox", "Inbox"),
];

const STYLE: &str = "
:root{--bg:#fafafa;--fg:#1a1a1a;--muted:#6b6b6b;--line:#e2e2e2;--card:#fff;--accent:#2457c5;
--ok-bg:#e7f5ea;--ok-fg:#1d6b32;--bad-bg:#fbeaea;--bad-fg:#a12020}
@media (prefers-color-scheme:dark){:root{--bg:#141414;--fg:#e8e8e8;--muted:#9a9a9a;--line:#2e2e2e;
--card:#1c1c1c;--accent:#86a8f0;--ok-bg:#16301d;--ok-fg:#8fd6a0;--bad-bg:#3a1a1a;--bad-fg:#f0a0a0}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,-apple-system,sans-serif}
header{display:flex;gap:20px;align-items:baseline;padding:12px 24px;border-bottom:1px solid var(--line);
background:var(--card);flex-wrap:wrap}
header strong{font-size:15px}
header a{color:var(--muted);text-decoration:none}
header a.on{color:var(--fg);font-weight:600}
main{padding:20px 24px 48px;max-width:1400px}
h1{font-size:20px;margin:0 0 16px}
h2{font-size:15px;margin:28px 0 8px}
a{color:var(--accent)}
.muted{color:var(--muted)}
.scroll{overflow-x:auto}
table{border-collapse:collapse;width:100%;background:var(--card);border:1px solid var(--line)}
th,td{padding:6px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}
th{font-weight:600;color:var(--muted);font-size:12px;white-space:nowrap}
td.n,th.n{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}
.tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(170px,1fr));gap:10px}
.tile{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:10px 12px}
.tile .label{color:var(--muted);font-size:12px}
.tile .value{font-size:20px;font-variant-numeric:tabular-nums}
dl.facts{display:grid;grid-template-columns:max-content 1fr;gap:4px 16px;margin:0}
dl.facts dt{color:var(--muted)}dl.facts dd{margin:0}
.notice{padding:8px 12px;border-radius:6px;margin-bottom:16px;background:var(--ok-bg);color:var(--ok-fg)}
.error{padding:8px 12px;border-radius:6px;background:var(--bad-bg);color:var(--bad-fg)}
form.inline{display:inline}
.actions{display:flex;flex-wrap:wrap;gap:12px;align-items:flex-end}
.actions form{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:10px 12px}
.actions label{display:block;color:var(--muted);font-size:12px;margin-bottom:4px}
input,select,button{font:inherit;padding:4px 8px;border:1px solid var(--line);border-radius:4px;
background:var(--bg);color:var(--fg)}
button{cursor:pointer}
button.danger{border-color:var(--bad-fg);color:var(--bad-fg)}
.filters{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px;align-items:center}
.msg{white-space:pre-wrap;max-width:60ch}
.bar{width:120px;height:8px;background:var(--line);border-radius:4px;overflow:hidden}
.bar span{display:block;height:100%;background:var(--accent)}
";

/// A whole page: navigation, an optional notice, and `body`.
pub fn page(title: &str, nav: Nav, notice: Option<&str>, body: &str) -> Html<String> {
    let mut out = String::with_capacity(body.len() + 4096);
    let _ = write!(
        out,
        "<!doctype html><html lang=en><head><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>{} · FinPlan admin</title><style>{STYLE}</style></head><body><header>\
         <strong>FinPlan admin</strong>",
        esc(title)
    );
    for (entry, href, label) in NAV {
        let class = if entry == nav { " class=on" } else { "" };
        let _ = write!(out, "<a href=\"{href}\"{class}>{label}</a>");
    }
    out.push_str("</header><main>");
    if let Some(notice) = notice {
        let _ = write!(out, "<div class=notice>{}</div>", esc(notice));
    }
    let _ = write!(out, "<h1>{}</h1>{body}</main></body></html>", esc(title));
    Html(out)
}

/// A request the admin pages could not serve, shown as a page.
#[derive(Debug)]
pub struct Failure {
    pub status: StatusCode,
    pub message: String,
}

impl Failure {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("No such {what}."))
    }
}

impl From<sqlx::Error> for Failure {
    fn from(_: sqlx::Error) -> Self {
        // Database errors can carry SQL values; the page and the log say less.
        tracing::error!(event = "admin.failed", error_class = "database");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The database query failed.",
        )
    }
}

impl From<crate::error::ApiError> for Failure {
    fn from(error: crate::error::ApiError) -> Self {
        match error {
            crate::error::ApiError::Database(error) => error.into(),
            other => Self::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = format!(
            "<p class=error>{}</p><p><a href=\"/\">Overview</a> · \
             <a href=\"/users\">Users</a></p>",
            esc(&self.message)
        );
        (
            self.status,
            page("Something went wrong", Nav::None, None, &body),
        )
            .into_response()
    }
}

pub type PageResult<T> = Result<T, Failure>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_quotes() {
        assert_eq!(
            esc("<a href=\"x\">'&'</a>").to_string(),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn formats_counts_money_and_segments() {
        assert_eq!(count(0), "0");
        assert_eq!(count(1234567), "1,234,567");
        assert_eq!(count(-1000), "-1,000");
        assert_eq!(money(0.01234), "$0.0123");
        assert_eq!(money(1234.5), "$1,234.50");
        assert_eq!(cents(0.93), "$0.93");
        assert_eq!(cents(0.001), "&lt;$0.01");
        assert_eq!(cents(0.0), "$0.00");
        assert_eq!(cents(2.5), "$2.50");
        assert_eq!(seg("a b/c?"), "a%20b%2Fc%3F");
    }
}
