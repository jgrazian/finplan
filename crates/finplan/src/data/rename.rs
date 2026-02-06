//! Cascading rename logic for simulation data.
//!
//! When a user renames an account, event, or return profile in the TUI,
//! these methods propagate the name change to all cross-references.

use super::app_data::SimulationData;
use super::events_data::{AmountData, EffectData, TriggerData};
use super::profiles_data::ReturnProfileTag;

impl SimulationData {
    /// Rename an account and update all references throughout the simulation data.
    pub fn rename_account(&mut self, old_name: &str, new_name: &str) {
        if old_name == new_name {
            return;
        }
        for event in &mut self.events {
            rename_account_in_trigger(&mut event.trigger, old_name, new_name);
            for effect in &mut event.effects {
                rename_account_in_effect(effect, old_name, new_name);
            }
        }
    }

    /// Rename an event and update all references throughout the simulation data.
    pub fn rename_event(&mut self, old_name: &str, new_name: &str) {
        if old_name == new_name {
            return;
        }
        for event in &mut self.events {
            rename_event_in_trigger(&mut event.trigger, old_name, new_name);
            for effect in &mut event.effects {
                rename_event_in_effect(effect, old_name, new_name);
            }
        }
        // Update sweep parameters
        for sweep in &mut self.analysis.sweep_parameters {
            if sweep.event_name == old_name {
                sweep.event_name = new_name.to_string();
            }
        }
    }

    /// Rename a return profile and update all references throughout the simulation data.
    pub fn rename_profile(&mut self, old_name: &str, new_name: &str) {
        if old_name == new_name {
            return;
        }
        let old_tag = ReturnProfileTag(old_name.to_string());
        let new_tag = ReturnProfileTag(new_name.to_string());
        // Update account properties
        for account in &mut self.portfolios.accounts {
            if let Some(prop) = account.account_type.as_property_mut()
                && prop.return_profile.as_ref() == Some(&old_tag)
            {
                prop.return_profile = Some(new_tag.clone());
            }
        }
        // Update asset mappings
        for profile in self.assets.values_mut() {
            if *profile == old_tag {
                *profile = new_tag.clone();
            }
        }
        for profile in self.historical_assets.values_mut() {
            if *profile == old_tag {
                *profile = new_tag.clone();
            }
        }
    }
}

fn rename_account_in_trigger(trigger: &mut TriggerData, old_name: &str, new_name: &str) {
    match trigger {
        TriggerData::AccountBalance { account, .. } => {
            if account.0 == old_name {
                account.0 = new_name.to_string();
            }
        }
        TriggerData::AssetBalance { account, .. } => {
            if account.0 == old_name {
                account.0 = new_name.to_string();
            }
        }
        TriggerData::Repeating { start, end, .. } => {
            if let Some(s) = start {
                rename_account_in_trigger(s, old_name, new_name);
            }
            if let Some(e) = end {
                rename_account_in_trigger(e, old_name, new_name);
            }
        }
        TriggerData::And { conditions } | TriggerData::Or { conditions } => {
            for cond in conditions {
                rename_account_in_trigger(cond, old_name, new_name);
            }
        }
        _ => {}
    }
}

fn rename_account_in_effect(effect: &mut EffectData, old_name: &str, new_name: &str) {
    match effect {
        EffectData::Income { to, amount, .. } => {
            if to.0 == old_name {
                to.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        EffectData::Expense { from, amount } => {
            if from.0 == old_name {
                from.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        EffectData::AssetPurchase {
            from,
            to_account,
            amount,
            ..
        } => {
            if from.0 == old_name {
                from.0 = new_name.to_string();
            }
            if to_account.0 == old_name {
                to_account.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        EffectData::AssetSale { from, amount, .. } => {
            if from.0 == old_name {
                from.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        EffectData::Sweep {
            to,
            amount,
            exclude_accounts,
            ..
        } => {
            if to.0 == old_name {
                to.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
            for acct in exclude_accounts {
                if acct.0 == old_name {
                    acct.0 = new_name.to_string();
                }
            }
        }
        EffectData::ApplyRmd { destination, .. } => {
            if destination.0 == old_name {
                destination.0 = new_name.to_string();
            }
        }
        EffectData::AdjustBalance { account, amount } => {
            if account.0 == old_name {
                account.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        EffectData::CashTransfer {
            from, to, amount, ..
        } => {
            if from.0 == old_name {
                from.0 = new_name.to_string();
            }
            if to.0 == old_name {
                to.0 = new_name.to_string();
            }
            rename_account_in_amount(amount, old_name, new_name);
        }
        _ => {}
    }
}

fn rename_account_in_amount(amount: &mut AmountData, old_name: &str, new_name: &str) {
    match amount {
        AmountData::AccountBalance { account } | AmountData::AccountCashBalance { account } => {
            if account.0 == old_name {
                account.0 = new_name.to_string();
            }
        }
        AmountData::InflationAdjusted { inner } => {
            rename_account_in_amount(inner, old_name, new_name);
        }
        AmountData::Scale { inner, .. } => {
            rename_account_in_amount(inner, old_name, new_name);
        }
        _ => {}
    }
}

fn rename_event_in_trigger(trigger: &mut TriggerData, old_name: &str, new_name: &str) {
    match trigger {
        TriggerData::RelativeToEvent { event, .. } => {
            if event.0 == old_name {
                event.0 = new_name.to_string();
            }
        }
        TriggerData::Repeating { start, end, .. } => {
            if let Some(s) = start {
                rename_event_in_trigger(s, old_name, new_name);
            }
            if let Some(e) = end {
                rename_event_in_trigger(e, old_name, new_name);
            }
        }
        TriggerData::And { conditions } | TriggerData::Or { conditions } => {
            for cond in conditions {
                rename_event_in_trigger(cond, old_name, new_name);
            }
        }
        _ => {}
    }
}

fn rename_event_in_effect(effect: &mut EffectData, old_name: &str, new_name: &str) {
    match effect {
        EffectData::TriggerEvent { event }
        | EffectData::PauseEvent { event }
        | EffectData::ResumeEvent { event }
        | EffectData::TerminateEvent { event } => {
            if event.0 == old_name {
                event.0 = new_name.to_string();
            }
        }
        EffectData::Random {
            on_true, on_false, ..
        } => {
            if on_true.0 == old_name {
                on_true.0 = new_name.to_string();
            }
            if let Some(of) = on_false
                && of.0 == old_name
            {
                of.0 = new_name.to_string();
            }
        }
        _ => {}
    }
}
