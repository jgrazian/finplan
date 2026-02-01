/// Format a currency value
pub fn format_currency(value: f64) -> String {
    // Format with thousands separators manually
    let abs_value = value.abs();
    let dollars = abs_value as i64;
    let cents = ((abs_value - dollars as f64) * 100.0).round() as i64;

    // Add thousands separators
    let dollars_str = dollars.to_string();
    let mut result = String::new();
    for (i, c) in dollars_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    let dollars_formatted: String = result.chars().rev().collect();

    if value >= 0.0 {
        format!("${}.{:02}", dollars_formatted, cents)
    } else {
        format!("-${}.{:02}", dollars_formatted, cents)
    }
}

/// Format a currency value without cents (shorter format for tight columns)
pub fn format_currency_short(value: f64) -> String {
    let abs_value = value.abs();
    let dollars = abs_value.round() as i64;

    // Add thousands separators
    let dollars_str = dollars.to_string();
    let mut result = String::new();
    for (i, c) in dollars_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    let dollars_formatted: String = result.chars().rev().collect();

    if value >= 0.0 {
        format!("${}", dollars_formatted)
    } else {
        format!("-${}", dollars_formatted)
    }
}

/// Format a percentage value
pub fn format_percentage(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}

/// Format a currency value in compact form (e.g., $2.1M, $450K, $50)
pub fn format_compact_currency(value: f64) -> String {
    let abs_value = value.abs();
    let sign = if value < 0.0 { "-" } else { "" };

    if abs_value >= 1_000_000.0 {
        format!("{}${:.1}M", sign, abs_value / 1_000_000.0)
    } else if abs_value >= 1_000.0 {
        format!("{}${:.0}K", sign, abs_value / 1_000.0)
    } else {
        format!("{}${:.0}", sign, abs_value)
    }
}
