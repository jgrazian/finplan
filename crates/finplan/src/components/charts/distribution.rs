//! Distribution chart rendering components.
//!
//! Renders various distribution visualizations for return profiles.

use crate::data::profiles_data::ReturnProfileData;
use crate::util::format::format_percentage;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Block characters for sub-character precision (from empty to full)
const BIN_CHARS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Render a distribution chart for a return profile.
pub fn render_distribution(frame: &mut Frame, area: Rect, profile: &ReturnProfileData) {
    match profile {
        ReturnProfileData::None => {
            let msg = Paragraph::new("No return (0%)").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
        }
        ReturnProfileData::Fixed { rate } => {
            render_fixed_rate(frame, area, *rate);
        }
        ReturnProfileData::Normal { mean, std_dev } => {
            render_normal_distribution(frame, area, *mean, *std_dev, false);
        }
        ReturnProfileData::LogNormal { mean, std_dev } => {
            render_normal_distribution(frame, area, *mean, *std_dev, true);
        }
        ReturnProfileData::StudentT { mean, scale, df } => {
            render_student_t_distribution(frame, area, *mean, *scale, *df);
        }
        ReturnProfileData::RegimeSwitching {
            bull_mean,
            bull_std_dev,
            bear_mean,
            bear_std_dev,
            ..
        } => {
            render_regime_switching_distribution(
                frame,
                area,
                *bull_mean,
                *bull_std_dev,
                *bear_mean,
                *bear_std_dev,
            );
        }
        ReturnProfileData::Bootstrap { preset } => {
            let history = ReturnProfileData::get_historical_returns(preset);
            render_historical_histogram(frame, area, &history);
        }
    }
}

/// Render a simple fixed rate indicator.
fn render_fixed_rate(frame: &mut Frame, area: Rect, rate: f64) {
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Fixed Rate: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format_percentage(rate), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::Cyan)),
            Span::styled(" ▲", Style::default().fg(Color::Yellow)),
        ]),
    ];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Render a normal or lognormal distribution histogram.
pub fn render_normal_distribution(
    frame: &mut Frame,
    area: Rect,
    mean: f64,
    std_dev: f64,
    is_lognormal: bool,
) {
    let num_bins = (area.width as usize).saturating_sub(4).max(10);
    let height = area.height.saturating_sub(2) as usize;

    if height < 3 || area.width < 20 {
        let msg = Paragraph::new("Area too small").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    // Calculate bin boundaries
    let (min_val, max_val) = if is_lognormal {
        let log_mean = (1.0 + mean).ln() - std_dev * std_dev / 2.0;
        let log_std = std_dev;
        let lower = (log_mean - 3.0 * log_std).exp() - 1.0;
        let upper = (log_mean + 3.0 * log_std).exp() - 1.0;
        (lower.max(-0.5), upper.min(1.0))
    } else {
        (mean - 3.0 * std_dev, mean + 3.0 * std_dev)
    };

    let bin_size = (max_val - min_val) / num_bins as f64;
    let pi = std::f64::consts::PI;
    let mut pdf_values = Vec::with_capacity(num_bins);

    for i in 0..num_bins {
        let x = min_val + (i as f64 + 0.5) * bin_size;

        let pdf = if is_lognormal {
            let growth = 1.0 + x;
            if growth > 0.0 {
                let log_mean = (1.0 + mean).ln() - std_dev * std_dev / 2.0;
                let log_x = growth.ln();
                let exponent = -(log_x - log_mean).powi(2) / (2.0 * std_dev * std_dev);
                (1.0 / (growth * std_dev * (2.0 * pi).sqrt())) * exponent.exp()
            } else {
                0.0
            }
        } else {
            let exponent = -(x - mean).powi(2) / (2.0 * std_dev * std_dev);
            (1.0 / (std_dev * (2.0 * pi).sqrt())) * exponent.exp()
        };

        pdf_values.push(pdf);
    }

    let max_pdf = pdf_values.iter().cloned().fold(0.0_f64, f64::max);
    if max_pdf == 0.0 {
        let msg =
            Paragraph::new("Invalid distribution").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let height_units = height * 8;
    let bar_heights: Vec<usize> = pdf_values
        .iter()
        .map(|&pdf| ((pdf / max_pdf) * height_units as f64).round() as usize)
        .collect();

    let x_offset = (area.width as usize).saturating_sub(num_bins) / 2;

    for row in 0..height {
        let row_base = (height - 1 - row) * 8;
        let row_top = row_base + 8;
        let mut spans = Vec::new();

        if x_offset > 0 {
            spans.push(Span::raw(" ".repeat(x_offset)));
        }

        for (i, &bar_h) in bar_heights.iter().enumerate() {
            let x = min_val + (i as f64 + 0.5) * bin_size;

            let color = if x < mean - std_dev {
                Color::Red
            } else if x > mean + std_dev {
                Color::Green
            } else {
                Color::Yellow
            };

            let char_to_use = if bar_h >= row_top {
                "█"
            } else if bar_h > row_base {
                let fill_level = bar_h - row_base;
                BIN_CHARS[fill_level.min(8)]
            } else {
                " "
            };

            spans.push(Span::styled(char_to_use, Style::default().fg(color)));
        }

        let line = Line::from(spans);
        let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line), row_area);
    }

    // Render x-axis labels
    let label_y = area.y + height as u16;
    let label_line = Line::from(vec![
        Span::styled(
            format!("{:>6}", format_percentage(min_val)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
        Span::styled(
            format!("μ={}", format_percentage(mean)),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
        Span::styled(
            format!("{:<6}", format_percentage(max_val)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let label_area = Rect::new(area.x, label_y, area.width, 1);
    frame.render_widget(Paragraph::new(label_line), label_area);
}

/// Render Student's t distribution with fat tails.
pub fn render_student_t_distribution(
    frame: &mut Frame,
    area: Rect,
    mean: f64,
    scale: f64,
    df: f64,
) {
    let num_bins = (area.width as usize).saturating_sub(4).max(10);
    let height = area.height.saturating_sub(2) as usize;

    if height < 3 || area.width < 20 {
        let msg = Paragraph::new("Area too small").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    // For Student's t, use wider range due to fat tails
    let range_mult = 4.0;
    let min_val = mean - range_mult * scale;
    let max_val = mean + range_mult * scale;
    let bin_size = (max_val - min_val) / num_bins as f64;

    let exponent = -(df + 1.0) / 2.0;

    let mut pdf_values = Vec::with_capacity(num_bins);
    for i in 0..num_bins {
        let x = min_val + (i as f64 + 0.5) * bin_size;
        let z = (x - mean) / scale;
        let pdf = (1.0 + z * z / df).powf(exponent);
        pdf_values.push(pdf);
    }

    let max_pdf = pdf_values.iter().cloned().fold(0.0_f64, f64::max);
    if max_pdf == 0.0 {
        let msg =
            Paragraph::new("Invalid distribution").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let height_units = height * 8;
    let bar_heights: Vec<usize> = pdf_values
        .iter()
        .map(|&pdf| ((pdf / max_pdf) * height_units as f64).round() as usize)
        .collect();

    let x_offset = (area.width as usize).saturating_sub(num_bins) / 2;

    for row in 0..height {
        let row_base = (height - 1 - row) * 8;
        let row_top = row_base + 8;
        let mut spans = Vec::new();

        if x_offset > 0 {
            spans.push(Span::raw(" ".repeat(x_offset)));
        }

        for (i, &bar_h) in bar_heights.iter().enumerate() {
            let x = min_val + (i as f64 + 0.5) * bin_size;

            // Use magenta for Student's t to distinguish from Normal
            let color = if x < mean - scale {
                Color::Red
            } else if x > mean + scale {
                Color::Green
            } else {
                Color::Magenta
            };

            let char_to_use = if bar_h >= row_top {
                "█"
            } else if bar_h > row_base {
                let fill_level = bar_h - row_base;
                BIN_CHARS[fill_level.min(8)]
            } else {
                " "
            };

            spans.push(Span::styled(char_to_use, Style::default().fg(color)));
        }

        let line = Line::from(spans);
        let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line), row_area);
    }

    // Render x-axis labels with df indicator
    let label_y = area.y + height as u16;
    let label_line = Line::from(vec![
        Span::styled(
            format!("{:>6}", format_percentage(min_val)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(24) / 2)),
        Span::styled(
            format!("μ={} df={:.0}", format_percentage(mean), df),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(24) / 2)),
        Span::styled(
            format!("{:<6}", format_percentage(max_val)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let label_area = Rect::new(area.x, label_y, area.width, 1);
    frame.render_widget(Paragraph::new(label_line), label_area);
}

/// Render bimodal distribution for regime switching (bull/bear overlay).
pub fn render_regime_switching_distribution(
    frame: &mut Frame,
    area: Rect,
    bull_mean: f64,
    bull_std_dev: f64,
    bear_mean: f64,
    bear_std_dev: f64,
) {
    let num_bins = (area.width as usize).saturating_sub(4).max(10);
    let height = area.height.saturating_sub(2) as usize;

    if height < 3 || area.width < 20 {
        let msg = Paragraph::new("Area too small").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    // Calculate range to include both distributions
    let min_val = (bear_mean - 3.0 * bear_std_dev).min(bull_mean - 3.0 * bull_std_dev);
    let max_val = (bear_mean + 3.0 * bear_std_dev).max(bull_mean + 3.0 * bull_std_dev);
    let bin_size = (max_val - min_val) / num_bins as f64;

    let pi = std::f64::consts::PI;

    // Calculate PDF values for both distributions
    let mut bull_pdf = Vec::with_capacity(num_bins);
    let mut bear_pdf = Vec::with_capacity(num_bins);

    for i in 0..num_bins {
        let x = min_val + (i as f64 + 0.5) * bin_size;

        // Bull market PDF (Normal)
        let bull_exp = -(x - bull_mean).powi(2) / (2.0 * bull_std_dev * bull_std_dev);
        let bull_p = (1.0 / (bull_std_dev * (2.0 * pi).sqrt())) * bull_exp.exp();
        bull_pdf.push(bull_p);

        // Bear market PDF (Normal)
        let bear_exp = -(x - bear_mean).powi(2) / (2.0 * bear_std_dev * bear_std_dev);
        let bear_p = (1.0 / (bear_std_dev * (2.0 * pi).sqrt())) * bear_exp.exp();
        bear_pdf.push(bear_p);
    }

    let max_pdf = bull_pdf
        .iter()
        .chain(bear_pdf.iter())
        .cloned()
        .fold(0.0_f64, f64::max);

    if max_pdf == 0.0 {
        let msg =
            Paragraph::new("Invalid distribution").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let height_units = height * 8;
    let bull_heights: Vec<usize> = bull_pdf
        .iter()
        .map(|&pdf| ((pdf / max_pdf) * height_units as f64).round() as usize)
        .collect();
    let bear_heights: Vec<usize> = bear_pdf
        .iter()
        .map(|&pdf| ((pdf / max_pdf) * height_units as f64).round() as usize)
        .collect();

    let x_offset = (area.width as usize).saturating_sub(num_bins) / 2;

    for row in 0..height {
        let row_base = (height - 1 - row) * 8;
        let row_top = row_base + 8;
        let mut spans = Vec::new();

        if x_offset > 0 {
            spans.push(Span::raw(" ".repeat(x_offset)));
        }

        for i in 0..num_bins {
            let bull_h = bull_heights[i];
            let bear_h = bear_heights[i];

            // Determine which distribution is dominant at this position
            let (dom_h, dom_color) = if bull_h > bear_h {
                (bull_h, Color::Green)
            } else if bear_h > bull_h {
                (bear_h, Color::Red)
            } else if bull_h > 0 {
                (bull_h, Color::Yellow) // Overlap
            } else {
                (0, Color::DarkGray)
            };

            let char_to_use = if dom_h >= row_top {
                "█"
            } else if dom_h > row_base {
                let fill_level = dom_h - row_base;
                BIN_CHARS[fill_level.min(8)]
            } else {
                " "
            };

            spans.push(Span::styled(char_to_use, Style::default().fg(dom_color)));
        }

        let line = Line::from(spans);
        let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line), row_area);
    }

    // Render x-axis labels with regime indicator
    let label_y = area.y + height as u16;
    let label_line = Line::from(vec![
        Span::styled("Bear", Style::default().fg(Color::Red)),
        Span::raw(": "),
        Span::styled(
            format_percentage(bear_mean),
            Style::default().fg(Color::Red),
        ),
        Span::raw("  "),
        Span::styled("Bull", Style::default().fg(Color::Green)),
        Span::raw(": "),
        Span::styled(
            format_percentage(bull_mean),
            Style::default().fg(Color::Green),
        ),
    ]);
    let label_area = Rect::new(area.x, label_y, area.width, 1);
    frame.render_widget(Paragraph::new(label_line), label_area);
}

/// Render a histogram of actual historical returns.
pub fn render_historical_histogram(
    frame: &mut Frame,
    area: Rect,
    history: &finplan_core::model::HistoricalReturns,
) {
    let returns: &[f64] = &history.returns;
    if returns.is_empty() {
        let msg = Paragraph::new("No historical data").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let num_bins = (area.width as usize).saturating_sub(4).max(10);
    let height = area.height.saturating_sub(2) as usize;

    if height < 3 || area.width < 20 {
        let msg = Paragraph::new("Area too small").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    // Calculate statistics
    let min_ret = returns.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_ret = returns.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std_dev = variance.sqrt();

    // Compute histogram bins
    let range = (max_ret - min_ret).max(0.01);
    let bin_width = range / num_bins as f64;
    let mut bin_counts = vec![0usize; num_bins];

    for &ret in returns {
        let bin = ((ret - min_ret) / bin_width).floor() as usize;
        let bin = bin.min(num_bins - 1);
        bin_counts[bin] += 1;
    }

    let max_count = *bin_counts.iter().max().unwrap_or(&1);
    if max_count == 0 {
        let msg = Paragraph::new("No data in bins").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let height_units = height * 8;
    let bar_heights: Vec<usize> = bin_counts
        .iter()
        .map(|&c| ((c as f64 / max_count as f64) * height_units as f64).round() as usize)
        .collect();

    let x_offset = (area.width as usize).saturating_sub(num_bins) / 2;

    for row in 0..height {
        let row_base = (height - 1 - row) * 8;
        let row_top = row_base + 8;
        let mut spans = Vec::new();

        if x_offset > 0 {
            spans.push(Span::raw(" ".repeat(x_offset)));
        }

        for (i, &bar_h) in bar_heights.iter().enumerate() {
            let x = min_ret + (i as f64 + 0.5) * bin_width;

            // Color based on position relative to mean
            let color = if x < mean - std_dev {
                Color::Red // Below -1σ
            } else if x > mean + std_dev {
                Color::Green // Above +1σ
            } else {
                Color::Cyan // Within ±1σ
            };

            let char_to_use = if bar_h >= row_top {
                "█"
            } else if bar_h > row_base {
                let fill_level = bar_h - row_base;
                BIN_CHARS[fill_level.min(8)]
            } else {
                " "
            };

            spans.push(Span::styled(char_to_use, Style::default().fg(color)));
        }

        let line = Line::from(spans);
        let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line), row_area);
    }

    // Render x-axis labels
    let label_y = area.y + height as u16;
    let label_line = Line::from(vec![
        Span::styled(
            format!("{:>6}", format_percentage(min_ret)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
        Span::styled(
            format!("μ={}", format_percentage(mean)),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" ".repeat((area.width as usize).saturating_sub(20) / 2)),
        Span::styled(
            format!("{:<6}", format_percentage(max_ret)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let label_area = Rect::new(area.x, label_y, area.width, 1);
    frame.render_widget(Paragraph::new(label_line), label_area);
}
