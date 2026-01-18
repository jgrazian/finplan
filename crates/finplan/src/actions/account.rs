// Account actions - category picking, type picking, CRUD operations

use crate::data::portfolio_data::{AccountData, AccountType, AssetAccount, Debt, Property};
use crate::data::profiles_data::ReturnProfileTag;
use crate::modals::{parse_currency, parse_percentage};
use crate::state::{AppState, FormField, FormModal, ModalAction, ModalState, PickerModal};

use super::{ActionContext, ActionResult};

/// Get account type options for a category
pub fn get_account_types_for_category(category: &str) -> Vec<String> {
    match category {
        "Investment" => vec![
            "Brokerage".to_string(),
            "401(k)".to_string(),
            "Roth 401(k)".to_string(),
            "Traditional IRA".to_string(),
            "Roth IRA".to_string(),
        ],
        "Cash" => vec![
            "Checking".to_string(),
            "Savings".to_string(),
            "HSA".to_string(),
        ],
        "Property" => vec!["Property".to_string(), "Collectible".to_string()],
        "Debt" => vec![
            "Mortgage".to_string(),
            "Loan".to_string(),
            "Student Loan".to_string(),
        ],
        _ => vec![],
    }
}

/// Handle account category selection - shows type picker
pub fn handle_category_pick(category: &str) -> ActionResult {
    let options = get_account_types_for_category(category);

    if options.is_empty() {
        ActionResult::close()
    } else {
        ActionResult::modal(ModalState::Picker(PickerModal::new(
            "Select Account Type",
            options,
            ModalAction::PICK_ACCOUNT_TYPE,
        )))
    }
}

/// Handle account type selection - shows creation form
pub fn handle_type_pick(account_type: &str) -> ActionResult {
    let (title, fields, context) = match account_type {
        "Brokerage" | "401(k)" | "Roth 401(k)" | "Traditional IRA" | "Roth IRA" => (
            "New Investment Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
            ],
            account_type.to_string(),
        ),
        "Checking" | "Savings" | "HSA" | "Property" | "Collectible" => (
            "New Cash/Property Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::currency("Value", 0.0),
                FormField::text("Return Profile", ""),
            ],
            account_type.to_string(),
        ),
        "Mortgage" | "Loan" | "Student Loan" => (
            "New Debt Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::currency("Balance", 0.0),
                FormField::percentage("Interest Rate", 0.0),
            ],
            account_type.to_string(),
        ),
        _ => return ActionResult::close(),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::CREATE_ACCOUNT).with_context(&context),
    ))
}

/// Handle account creation
pub fn handle_create_account(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let parts = ctx.value_parts();
    let account_type_str = ctx.context_str();

    let account = match account_type_str {
        "Brokerage" => create_investment_account(&parts, AccountType::Brokerage),
        "401(k)" => create_investment_account(&parts, AccountType::Traditional401k),
        "Roth 401(k)" => create_investment_account(&parts, AccountType::Roth401k),
        "Traditional IRA" => create_investment_account(&parts, AccountType::TraditionalIRA),
        "Roth IRA" => create_investment_account(&parts, AccountType::RothIRA),
        "Checking" => create_property_account(&parts, AccountType::Checking),
        "Savings" => create_property_account(&parts, AccountType::Savings),
        "HSA" => create_property_account(&parts, AccountType::HSA),
        "Property" => create_property_account(&parts, AccountType::Property),
        "Collectible" => create_property_account(&parts, AccountType::Collectible),
        "Mortgage" => create_debt_account(&parts, AccountType::Mortgage),
        "Loan" => create_debt_account(&parts, AccountType::LoanDebt),
        "Student Loan" => create_debt_account(&parts, AccountType::StudentLoanDebt),
        _ => None,
    };

    if let Some(acc) = account {
        state.data_mut().portfolios.accounts.push(acc);
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle account editing
pub fn handle_edit_account(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let idx = match ctx.index() {
        Some(i) => i,
        None => return ActionResult::close(),
    };

    let parts = ctx.value_parts();

    if let Some(account) = state.data_mut().portfolios.accounts.get_mut(idx) {
        match &mut account.account_type {
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => {
                // Parts: [type, name, description, value, profile]
                if let Some(name) = parts.get(1) {
                    account.name = name.to_string();
                }
                account.description = parts
                    .get(2)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                if let Some(val) = parts.get(3).and_then(|s| parse_currency(s).ok()) {
                    prop.value = val;
                }
                prop.return_profile = parts
                    .get(4)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .map(ReturnProfileTag);
            }
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => {
                // Parts: [type, name, description, balance, rate]
                if let Some(name) = parts.get(1) {
                    account.name = name.to_string();
                }
                account.description = parts
                    .get(2)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                if let Some(bal) = parts.get(3).and_then(|s| parse_currency(s).ok()) {
                    debt.balance = bal;
                }
                if let Some(rate) = parts.get(4).and_then(|s| parse_percentage(s).ok()) {
                    debt.interest_rate = rate;
                }
            }
            AccountType::Brokerage(_)
            | AccountType::Traditional401k(_)
            | AccountType::Roth401k(_)
            | AccountType::TraditionalIRA(_)
            | AccountType::RothIRA(_) => {
                // Parts: [type, name, description]
                if let Some(name) = parts.get(1) {
                    account.name = name.to_string();
                }
                account.description = parts
                    .get(2)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
            }
        }
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle account deletion
pub fn handle_delete_account(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    if let Some(idx) = ctx.index() {
        let accounts_len = state.data().portfolios.accounts.len();
        if idx < accounts_len {
            state.data_mut().portfolios.accounts.remove(idx);
            let new_len = state.data().portfolios.accounts.len();
            // Adjust selected index
            if state.portfolio_profiles_state.selected_account_index >= new_len && new_len > 0 {
                state.portfolio_profiles_state.selected_account_index = new_len - 1;
            }
            return ActionResult::modified();
        }
    }
    ActionResult::close()
}

// Helper functions for account creation

fn create_investment_account<F>(parts: &[&str], make_type: F) -> Option<AccountData>
where
    F: FnOnce(AssetAccount) -> AccountType,
{
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    Some(AccountData {
        name,
        description: desc,
        account_type: make_type(AssetAccount { assets: vec![] }),
    })
}

fn create_property_account<F>(parts: &[&str], make_type: F) -> Option<AccountData>
where
    F: FnOnce(Property) -> AccountType,
{
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let value = parts
        .get(2)
        .and_then(|s| parse_currency(s).ok())
        .unwrap_or(0.0);
    let profile = parts
        .get(3)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let prop = Property {
        value,
        return_profile: profile.map(ReturnProfileTag),
    };

    Some(AccountData {
        name,
        description: desc,
        account_type: make_type(prop),
    })
}

fn create_debt_account<F>(parts: &[&str], make_type: F) -> Option<AccountData>
where
    F: FnOnce(Debt) -> AccountType,
{
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let balance = parts
        .get(2)
        .and_then(|s| parse_currency(s).ok())
        .unwrap_or(0.0);
    let rate = parts
        .get(3)
        .and_then(|s| parse_percentage(s).ok())
        .unwrap_or(0.0);

    let debt = Debt {
        balance,
        interest_rate: rate,
    };

    Some(AccountData {
        name,
        description: desc,
        account_type: make_type(debt),
    })
}
