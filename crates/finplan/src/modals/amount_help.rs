//! Scrollable language reference that leaves the underlying amount form intact.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::{FieldType, FormModal, helpers::render_modal_frame};

#[derive(Debug, Clone, Default)]
pub struct AmountHelp {
    scroll: u16,
    max_scroll: u16,
    page_height: u16,
}

/// Allow help throughout amount forms, but keep '?' as text in other text fields.
pub fn available(modal: &FormModal) -> bool {
    modal
        .fields
        .iter()
        .any(|field| field.amount_input.is_some())
        && (!modal.editing
            || modal.fields.get(modal.focused_field).is_some_and(|field| {
                field.amount_input.is_some()
                    || matches!(field.field_type, FieldType::Select | FieldType::ReadOnly)
            }))
}

/// Return true when the help overlay consumes the key (including opening/closing).
pub fn handle_key(key: KeyEvent, modal: &mut FormModal) -> bool {
    if let Some(help) = &mut modal.amount_help {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => modal.amount_help = None,
            KeyCode::Down | KeyCode::Char('j') => {
                help.scroll = help.scroll.saturating_add(1).min(help.max_scroll);
            }
            KeyCode::Up | KeyCode::Char('k') => help.scroll = help.scroll.saturating_sub(1),
            KeyCode::PageDown => {
                help.scroll = help
                    .scroll
                    .saturating_add(help.page_height.max(1))
                    .min(help.max_scroll);
            }
            KeyCode::PageUp => {
                help.scroll = help.scroll.saturating_sub(help.page_height.max(1));
            }
            KeyCode::Home => help.scroll = 0,
            KeyCode::End => help.scroll = help.max_scroll,
            _ => {}
        }
        return true;
    }
    if key.code == KeyCode::Char('?')
        && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        && available(modal)
    {
        modal.amount_help = Some(AmountHelp::default());
        return true;
    }
    false
}

const EXAMPLES: &[&str] = &[
    "inflation($Spending)",
    "$WithdrawalRate * balance(\"Vanguard\")",
    "min(cash(source), 5000)",
    "if(age() >= 65, $\"Retirement spending\", $Spending)",
    "net(top_up(inflation($Spending)))",
];

const BALANCES: &[(&str, &str)] = &[
    (
        "net_worth()",
        "Total value of all accounts, including property and negative debt.",
    ),
    (
        "balance(account)",
        "Total account value: cash plus holdings. Example: balance(\"Vanguard\").",
    ),
    (
        "cash(account)",
        "Cash only. The account must support cash. Example: cash(source).",
    ),
    (
        "holding(account, \"VTI\")",
        "Value of the named asset in that account, across all lots.",
    ),
    (
        "endpoint_balance(source) / endpoint_balance(target)",
        "Value of the effect's specific cash or asset endpoint.",
    ),
    (
        "source_balance() / target_balance()",
        "Short forms of endpoint_balance(source) / endpoint_balance(target).",
    ),
];

const AMOUNTS: &[(&str, &str)] = &[
    (
        "inflation(amount)",
        "Convert simulation-start dollars to current nominal dollars. Balances are already nominal.",
    ),
    (
        "min(a, b) / max(a, b)",
        "Smaller / larger of two compatible values. Exactly two arguments.",
    ),
    (
        "clamp(value, lower, upper)",
        "Limit a value to the range. The lower bound must not exceed the upper bound.",
    ),
    ("abs(value)", "Absolute value."),
    (
        "if(condition, then_value, else_value)",
        "Evaluate only the selected branch. Both branches must have compatible types.",
    ),
    (
        "top_up(desired)",
        "Amount needed to reach the target endpoint's desired balance, floored at zero.",
    ),
    (
        "payoff()",
        "Amount needed to clear the target account's negative debt balance, floored at zero.",
    ),
];

const CALENDAR: &[(&str, &str)] = &[
    (
        "age()",
        "Calendar age in years plus completed months / 12. Requires a scenario birth date.",
    ),
    (
        "age_years($RetirementAge)",
        "Convert a named Age parameter to years plus months / 12.",
    ),
    ("year() / month()", "Current simulated year / month (1-12)."),
    ("years_since_start()", "Elapsed simulated days / 365.2425."),
    (
        "days_until(date) / years_until(date)",
        "Signed days until the date, or days / 365.2425. Use \"2035-01-01\" or a named Date parameter such as $RetirementDate.",
    ),
];

fn heading(lines: &mut Vec<Line<'static>>, title: &'static str) {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.push(Line::styled(
        title,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
}

fn functions(lines: &mut Vec<Line<'static>>, entries: &[(&'static str, &'static str)]) {
    for (signature, description) in entries {
        lines.push(Line::styled(*signature, Style::default().fg(Color::Cyan)));
        lines.push(Line::from(format!("  {description}")));
    }
}

fn content() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    heading(&mut lines, "Editing amounts");
    lines.extend([
        Line::from("Enter edits the selected amount. x switches Static / Expression mode."),
        Line::from("While editing an expression, x is text. Press Enter, then x to switch back."),
        Line::from("Tab completes $parameters; Tab / Shift+Tab cycle matches."),
        Line::from(
            "Enter finishes the field; Esc reverts the edit. F10 / Ctrl+S submits the form.",
        ),
        Line::from("? opens this help. Closing it preserves your draft and cursor."),
    ]);
    heading(&mut lines, "Examples");
    lines.push(Line::from(
        "Replace sample names with your Parameters, accounts, and assets.",
    ));
    lines.extend(
        EXAMPLES
            .iter()
            .map(|source| Line::styled(*source, Style::default().fg(Color::Cyan))),
    );
    heading(&mut lines, "Syntax and variables");
    lines.extend([
        Line::from("Type an expression directly: 5000, 2.5, or 1e3. No leading =, currency symbol, or commas in numbers."),
        Line::from("Use $Spending or $\"Retirement spending\" for names with spaces or punctuation. Names are case-sensitive."),
        Line::from("Arithmetic: + - * /, unary +/-, and parentheses. * and / come before + and -. 4% means 0.04; % is not modulo."),
        Line::from("Compare: < <= > >= == !=. Combine conditions with and, or, not; true and false are Boolean literals."),
        Line::from("Comparisons bind before not, then and, then or. Use parentheses to group. Write 50 <= age() and age() < 65 rather than chaining comparisons."),
        Line::from("The result must be Money. Multiply a Rate by Money; Money + Rate and Money * Money are invalid. Date and Age parameters need calendar helpers."),
        Line::from("Account and asset names use double quotes. Inside quoted names, use \\\" for a quote and \\\\ for a backslash."),
    ]);
    heading(&mut lines, "Built-in functions: balances");
    functions(&mut lines, BALANCES);
    heading(&mut lines, "Built-in functions: amounts and conditions");
    functions(&mut lines, AMOUNTS);
    heading(&mut lines, "Built-in functions: calendar");
    functions(&mut lines, CALENDAR);
    heading(&mut lines, "Gross / net annotations");
    functions(
        &mut lines,
        &[(
            "gross(amount) / net(amount)",
            "Wrap the entire expression to set Amount Type for Income, Asset Sale, or Sweep. They cannot be nested or used inside arithmetic; other effects do not support them.",
        )],
    );
    heading(&mut lines, "Source and target");
    lines.extend([
        Line::from("account can be a quoted account name, source, or target. Bare source / target refer to this effect; \"source\" is a literal account name."),
        Line::from("Income has a target; Expense has a source; transfers have both. Balance adjustments have a target."),
        Line::from("For purchases, the target endpoint is the purchased holding. For sales, the source endpoint is the selected holding, and the target is cash in the selling account."),
        Line::from("balance(source) always reads the whole account. source_balance() reads only the specific cash or holding endpoint."),
        Line::from("Account-wide sales or sweeps have no single source endpoint: use balance(source) - cash(source) for invested value. A strategy or custom-list sweep has no single source account."),
        Line::from("Unavailable source / target context is an error. For a debt target, use payoff() or balance(target); a liability has no cash endpoint."),
    ]);
    lines
}

pub fn render(frame: &mut Frame, help: &mut AmountHelp) {
    let height = frame.area().height.saturating_sub(2).min(36);
    let modal = render_modal_frame(
        frame,
        "Amount Expression Help",
        86,
        height,
        Color::Cyan,
        &[
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(2),
        ],
    );
    let body = modal.chunks[0];
    let paragraph = Paragraph::new(content()).wrap(Wrap { trim: false });
    let line_count = paragraph.line_count(body.width) as u16;
    help.page_height = body.height;
    help.max_scroll = line_count.saturating_sub(body.height);
    help.scroll = help.scroll.min(help.max_scroll);
    frame.render_widget(paragraph.scroll((help.scroll, 0)), body);
    frame.render_widget(
        Paragraph::new(format!(
            "Lines {}-{} of {}",
            (help.scroll + 1).min(line_count),
            (help.scroll + body.height).min(line_count),
            line_count,
        ))
        .style(Style::default().fg(Color::DarkGray)),
        modal.chunks[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("[Up/Down/j/k]", Style::default().fg(Color::Cyan)),
                Span::raw(" Scroll  [PgUp/PgDn] Page  [Home/End] Jump"),
            ]),
            Line::from(vec![
                Span::styled("[Esc/Enter/?]", Style::default().fg(Color::Green)),
                Span::raw(" Back to amount form"),
            ]),
        ]),
        modal.chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        data::events_data::AmountData,
        modals::{FormField, ModalAction},
    };
    use finplan_core::{SimulationBuilder, expression::compile_amount, model::ParameterValue};

    #[test]
    fn help_examples_compile_with_the_documented_parameter_types() {
        let (config, metadata) = SimulationBuilder::new()
            .bank("Vanguard", 1000.0)
            .parameter("Spending", 500.0)
            .parameter("Retirement spending", 700.0)
            .parameter("WithdrawalRate", ParameterValue::Rate(0.04))
            .build();
        for source in EXAMPLES {
            assert!(
                compile_amount(source, &metadata, &config.parameters).is_ok(),
                "{source}"
            );
        }
    }

    #[test]
    fn wrapped_help_scrolls_to_the_end_and_clamps_after_resize() {
        let mut form = FormModal::new(
            "Amount",
            vec![FormField::amount("Amount", AmountData::fixed(0.0))],
            ModalAction::ADD_EFFECT,
        );
        assert!(handle_key(KeyEvent::from(KeyCode::Char('?')), &mut form));
        for (width, height) in [(32, 12), (70, 24), (100, 40)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, form.amount_help.as_mut().unwrap()))
                .unwrap();
            handle_key(KeyEvent::from(KeyCode::Home), &mut form);
            assert_eq!(form.amount_help.as_ref().unwrap().scroll, 0);
            handle_key(KeyEvent::from(KeyCode::PageDown), &mut form);
            let help = form.amount_help.as_ref().unwrap();
            assert_eq!(help.scroll, help.page_height);
            handle_key(KeyEvent::from(KeyCode::Up), &mut form);
            let help = form.amount_help.as_ref().unwrap();
            assert_eq!(help.scroll, help.page_height - 1);
            handle_key(KeyEvent::from(KeyCode::PageUp), &mut form);
            assert_eq!(form.amount_help.as_ref().unwrap().scroll, 0);
            handle_key(KeyEvent::from(KeyCode::End), &mut form);
            terminal
                .draw(|frame| render(frame, form.amount_help.as_mut().unwrap()))
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(rendered.contains("Amount Expression Help"));
            assert!(
                rendered.contains("cash endpoint."),
                "last wrapped line is reachable at {width}x{height}"
            );
            assert!(rendered.contains("[Esc/Enter/?]"));
            let help = form.amount_help.as_ref().unwrap();
            assert_eq!(help.scroll, help.max_scroll);
            handle_key(KeyEvent::from(KeyCode::Down), &mut form);
            let help = form.amount_help.as_ref().unwrap();
            assert_eq!(help.scroll, help.max_scroll);
        }
        for (width, height) in [(1, 1), (10, 5), (70, 35)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, form.amount_help.as_mut().unwrap()))
                .unwrap();
            let help = form.amount_help.as_ref().unwrap();
            assert!(help.scroll <= help.max_scroll);
        }
    }
}
