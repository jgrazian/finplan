//! Source helpers shared by persisted amounts and the inline editor.
use super::events_data::{AmountData, EffectData};

pub fn quote(name: &str) -> String {
    format!(
        "\"{}\"",
        name.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

pub fn parameter_source(name: &str) -> String {
    let mut chars = name.chars();
    let identifier = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    format!(
        "${}",
        if identifier {
            name.to_owned()
        } else {
            quote(name)
        }
    )
}

impl EffectData {
    pub fn amount(&self) -> Option<&AmountData> {
        match self {
            Self::Income { amount, .. }
            | Self::Expense { amount, .. }
            | Self::AssetPurchase { amount, .. }
            | Self::AssetSale { amount, .. }
            | Self::Sweep { amount, .. }
            | Self::AdjustBalance { amount, .. }
            | Self::CashTransfer { amount, .. } => Some(amount),
            _ => None,
        }
    }

    pub fn amount_mut(&mut self) -> Option<&mut AmountData> {
        match self {
            Self::Income { amount, .. }
            | Self::Expense { amount, .. }
            | Self::AssetPurchase { amount, .. }
            | Self::AssetSale { amount, .. }
            | Self::Sweep { amount, .. }
            | Self::AdjustBalance { amount, .. }
            | Self::CashTransfer { amount, .. } => Some(amount),
            _ => None,
        }
    }
}

/// Parameter tokens only: dollar signs inside account/asset strings are literals.
/// Incomplete tokens are also returned so the editor can complete them.
pub fn parameter_tokens(source: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '"' {
            while let Some((_, ch)) = chars.next() {
                if ch == '\\' {
                    chars.next();
                } else if ch == '"' {
                    break;
                }
            }
        } else if ch == '$' {
            let mut name = String::new();
            let mut end = start + 1;
            if chars.peek().is_some_and(|(_, ch)| *ch == '"') {
                let (i, _) = chars.next().unwrap();
                end = i + 1;
                while let Some((i, ch)) = chars.next() {
                    end = i + ch.len_utf8();
                    if ch == '"' {
                        break;
                    }
                    if ch == '\\' {
                        if let Some((i, escaped)) = chars.next() {
                            end = i + escaped.len_utf8();
                            name.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                c => c,
                            });
                        }
                    } else {
                        name.push(ch);
                    }
                }
            } else {
                while let Some(&(i, ch)) = chars.peek() {
                    if !ch.is_alphanumeric() && ch != '_' {
                        break;
                    }
                    chars.next();
                    name.push(ch);
                    end = i + ch.len_utf8();
                }
            }
            tokens.push((start..end, name));
        }
    }
    tokens
}

pub fn visit_parameter_names(source: &mut String, visitor: &mut impl FnMut(&mut String)) {
    for (range, mut name) in parameter_tokens(source).into_iter().rev() {
        let original = name.clone();
        visitor(&mut name);
        if name != original {
            source.replace_range(range, &parameter_source(&name));
        }
    }
}

/// Re-render compiled references using the account's new name. The account form
/// calls this after updating the account; other callers rename references first.
pub fn rename_account_expressions(
    data: &mut super::app_data::SimulationData,
    old: &str,
    new: &str,
) {
    let (mut before, parameters) = super::convert::amount_expression_context(data);
    let Some(id) = before.account_id(old).or_else(|| before.account_id(new)) else {
        return;
    };
    before.register_account(id, Some(old.into()), None);
    let mut after = before.clone();
    after.register_account(id, Some(new.into()), None);
    fn rename(
        amount: &mut AmountData,
        before: &finplan_core::config::SimulationMetadata,
        after: &finplan_core::config::SimulationMetadata,
        parameters: &std::collections::HashMap<
            finplan_core::model::ParameterId,
            finplan_core::model::ParameterValue,
        >,
    ) {
        match amount {
            AmountData::Expression { source } => {
                if let Ok(compiled) =
                    finplan_core::expression::compile_amount(source, before, parameters)
                    && let Ok(rendered) = compiled.amount.to_source(after)
                    && compiled.amount.to_source(before).ok().as_ref() != Some(&rendered)
                {
                    *source = match compiled.amount_mode {
                        Some(finplan_core::model::AmountMode::Gross) => {
                            format!("gross({rendered})")
                        }
                        Some(finplan_core::model::AmountMode::Net) => format!("net({rendered})"),
                        None => rendered,
                    };
                }
            }
            AmountData::RateTimes { inner, .. }
            | AmountData::Scale { inner, .. }
            | AmountData::InflationAdjusted { inner } => rename(inner, before, after, parameters),
            _ => {}
        }
    }
    for event in &mut data.events {
        for effect in &mut event.effects {
            if let Some(amount) = effect.amount_mut() {
                rename(amount, &before, &after, &parameters);
            }
        }
    }
}
