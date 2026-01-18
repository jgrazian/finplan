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

/// Format a percentage value
pub fn format_percentage(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}
