// Account actions - category picking, type picking, CRUD operations

use crate::data::portfolio_data::{AccountData, AccountType, AssetAccount, Debt, Property};
use crate::data::profiles_data::ReturnProfileTag;
use crate::modals::context::{AccountTypeContext, ModalContext};
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
pub fn handle_type_pick(account_type: &str, state: &AppState) -> ActionResult {
    // Build list of available return profiles for Select fields
    let mut profile_options: Vec<String> = vec!["".to_string()]; // Empty option for "none"
    profile_options.extend(state.data().profiles.iter().map(|p| p.name.0.clone()));

    // Parse the account type string to typed context
    let account_type_ctx = match account_type.parse::<AccountTypeContext>() {
        Ok(ctx) => ctx,
        Err(_) => return ActionResult::close(),
    };

    let (title, fields) = match &account_type_ctx {
        AccountTypeContext::Brokerage
        | AccountTypeContext::Traditional401k
        | AccountTypeContext::Roth401k
        | AccountTypeContext::TraditionalIRA
        | AccountTypeContext::RothIRA => (
            "New Investment Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
            ],
        ),
        AccountTypeContext::Checking
        | AccountTypeContext::Savings
        | AccountTypeContext::HSA
        | AccountTypeContext::Property
        | AccountTypeContext::Collectible => (
            "New Cash/Property Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::currency("Value", 0.0),
                FormField::select("Return Profile", profile_options, ""),
            ],
        ),
        AccountTypeContext::Mortgage
        | AccountTypeContext::Loan
        | AccountTypeContext::StudentLoan => (
            "New Debt Account",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::currency("Balance", 0.0),
                FormField::percentage("Interest Rate", 0.0),
            ],
        ),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::CREATE_ACCOUNT)
            .with_typed_context(ModalContext::AccountType(account_type_ctx))
            .start_editing(),
    ))
}

/// Handle account creation
pub fn handle_create_account(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    // Get typed account type context
    let account_type_ctx = ctx
        .typed_context()
        .and_then(|c| c.as_account_type())
        .cloned();

    let account = match account_type_ctx {
        Some(AccountTypeContext::Brokerage) => {
            create_investment_account_typed(form, AccountType::Brokerage)
        }
        Some(AccountTypeContext::Traditional401k) => {
            create_investment_account_typed(form, AccountType::Traditional401k)
        }
        Some(AccountTypeContext::Roth401k) => {
            create_investment_account_typed(form, AccountType::Roth401k)
        }
        Some(AccountTypeContext::TraditionalIRA) => {
            create_investment_account_typed(form, AccountType::TraditionalIRA)
        }
        Some(AccountTypeContext::RothIRA) => {
            create_investment_account_typed(form, AccountType::RothIRA)
        }
        Some(AccountTypeContext::Checking) => {
            create_property_account_typed(form, AccountType::Checking)
        }
        Some(AccountTypeContext::Savings) => {
            create_property_account_typed(form, AccountType::Savings)
        }
        Some(AccountTypeContext::HSA) => create_property_account_typed(form, AccountType::HSA),
        Some(AccountTypeContext::Property) => {
            create_property_account_typed(form, AccountType::Property)
        }
        Some(AccountTypeContext::Collectible) => {
            create_property_account_typed(form, AccountType::Collectible)
        }
        Some(AccountTypeContext::Mortgage) => {
            create_debt_account_typed(form, AccountType::Mortgage)
        }
        Some(AccountTypeContext::Loan) => create_debt_account_typed(form, AccountType::LoanDebt),
        Some(AccountTypeContext::StudentLoan) => {
            create_debt_account_typed(form, AccountType::StudentLoanDebt)
        }
        None => None,
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

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    if let Some(account) = state.data_mut().portfolios.accounts.get_mut(idx) {
        match &mut account.account_type {
            AccountType::Checking(prop)
            | AccountType::Savings(prop)
            | AccountType::HSA(prop)
            | AccountType::Property(prop)
            | AccountType::Collectible(prop) => {
                // Fields: [Name, Description, Value, Return Profile]
                if let Some(name) = form.get_str(0) {
                    account.name = name.to_string();
                }
                account.description = form.get_optional_str(1);
                if let Some(val) = form.get_currency(2) {
                    prop.value = val;
                }
                prop.return_profile = form.get_optional_str(3).map(ReturnProfileTag);
            }
            AccountType::Mortgage(debt)
            | AccountType::LoanDebt(debt)
            | AccountType::StudentLoanDebt(debt) => {
                // Fields: [Name, Description, Balance, Interest Rate]
                if let Some(name) = form.get_str(0) {
                    account.name = name.to_string();
                }
                account.description = form.get_optional_str(1);
                if let Some(bal) = form.get_currency(2) {
                    debt.balance = bal;
                }
                if let Some(rate) = form.get_percentage(3) {
                    debt.interest_rate = rate;
                }
            }
            AccountType::Brokerage(_)
            | AccountType::Traditional401k(_)
            | AccountType::Roth401k(_)
            | AccountType::TraditionalIRA(_)
            | AccountType::RothIRA(_) => {
                // Fields: [Name, Description]
                if let Some(name) = form.get_str(0) {
                    account.name = name.to_string();
                }
                account.description = form.get_optional_str(1);
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

/// Create an investment account from typed form fields
fn create_investment_account_typed<F>(form: &FormModal, make_type: F) -> Option<AccountData>
where
    F: FnOnce(AssetAccount) -> AccountType,
{
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    Some(AccountData {
        name,
        description: desc,
        account_type: make_type(AssetAccount { assets: vec![] }),
    })
}

/// Create a property account from typed form fields
fn create_property_account_typed<F>(form: &FormModal, make_type: F) -> Option<AccountData>
where
    F: FnOnce(Property) -> AccountType,
{
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let value = form.get_currency_or(2, 0.0);
    let profile = form.get_optional_str(3);

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

/// Create a debt account from typed form fields
fn create_debt_account_typed<F>(form: &FormModal, make_type: F) -> Option<AccountData>
where
    F: FnOnce(Debt) -> AccountType,
{
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let balance = form.get_currency_or(2, 0.0);
    let rate = form.get_percentage_or(3, 0.0);

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
