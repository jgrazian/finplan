use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::components::{Component, EventResult, status_bar::StatusBar, tab_bar::TabBar};
use crate::data::events_data::{
    AccountTag, AmountData, EffectData, EventData, EventTag, IntervalData, ThresholdData,
    TriggerData,
};
use crate::data::parameters_data::{DistributionType, FederalBracketsPreset, InflationData};
use crate::data::portfolio_data::{
    AccountData, AccountType, AssetAccount, AssetTag, AssetValue, Debt, Property,
};
use crate::data::profiles_data::{ProfileData, ReturnProfileData, ReturnProfileTag};
use crate::modals::{
    ModalResult, handle_modal_key, parse_currency, parse_percentage, render_modal,
};
use crate::screens::{
    events::EventsScreen, portfolio_profiles::PortfolioProfilesScreen, results::ResultsScreen,
    scenario::ScenarioScreen,
};
use crate::state::{
    AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal, TabId,
};

pub struct App {
    state: AppState,
    tab_bar: TabBar,
    status_bar: StatusBar,
    portfolio_profiles_screen: PortfolioProfilesScreen,
    scenario_screen: ScenarioScreen,
    events_screen: EventsScreen,
    results_screen: ResultsScreen,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let state = AppState::default();

        Self {
            state,
            tab_bar: TabBar::new(),
            status_bar: StatusBar::new(),
            portfolio_profiles_screen: PortfolioProfilesScreen::new(),
            scenario_screen: ScenarioScreen::new(),
            events_screen: EventsScreen::new(),
            results_screen: ResultsScreen::new(),
        }
    }

    /// Create app with a specific config file path
    /// Loads existing data if the file exists, otherwise creates default with sample data
    pub fn with_config_path(config_path: PathBuf) -> Self {
        let state = if config_path.exists() {
            match AppState::load_from_file(config_path.clone()) {
                Ok(mut state) => {
                    state.config_path = Some(config_path);
                    state
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to load config from {:?}: {:?}",
                        config_path, e
                    );
                    eprintln!("Starting with default configuration.");
                    let mut state = AppState::default();
                    state.config_path = Some(config_path);
                    state
                }
            }
        } else {
            // File doesn't exist, create default with sample data
            let mut state = AppState::default();
            state.config_path = Some(config_path);
            state
        };

        Self {
            state,
            tab_bar: TabBar::new(),
            status_bar: StatusBar::new(),
            portfolio_profiles_screen: PortfolioProfilesScreen::new(),
            scenario_screen: ScenarioScreen::new(),
            events_screen: EventsScreen::new(),
            results_screen: ResultsScreen::new(),
        }
    }
}

impl App {
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        while !self.state.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        // Auto-save on exit
        if let Err(e) = self.state.save() {
            if let Some(path) = &self.state.config_path {
                eprintln!("Warning: Failed to save config to {:?}: {:?}", path, e);
            }
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Create main layout: tab bar, content, status bar
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Min(0),    // Content
                Constraint::Length(2), // Status bar
            ])
            .split(frame.area());

        // Render tab bar
        self.tab_bar.render(frame, chunks[0], &self.state);

        // Render active screen
        self.render_active_screen(frame, chunks[1]);

        // Render status bar
        self.status_bar.render(frame, chunks[2], &self.state);

        // Render modal overlay (if active)
        render_modal(frame, &self.state);
    }

    fn render_active_screen(&mut self, frame: &mut Frame, area: Rect) {
        match self.state.active_tab {
            TabId::PortfolioProfiles => {
                self.portfolio_profiles_screen
                    .render(frame, area, &self.state)
            }
            TabId::Scenario => self.scenario_screen.render(frame, area, &self.state),
            TabId::Events => self.events_screen.render(frame, area, &self.state),
            TabId::Results => self.results_screen.render(frame, area, &self.state),
        }
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Handle modal first if active
        if !matches!(self.state.modal, ModalState::None) {
            match handle_modal_key(key_event, &mut self.state) {
                ModalResult::Confirmed(action, value) => {
                    self.handle_modal_result(action, value);
                }
                ModalResult::Cancelled => {
                    self.state.modal = ModalState::None;
                }
                ModalResult::Continue => {}
            }
            return;
        }

        // Global key bindings
        match key_event.code {
            KeyCode::Char('q') if key_event.modifiers.is_empty() => {
                self.state.exit = true;
                return;
            }
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.exit = true;
                return;
            }
            KeyCode::Esc => {
                // Clear error message on Esc
                self.state.clear_error();
                return;
            }
            _ => {}
        }

        // Try tab bar first
        let result = self.tab_bar.handle_key(key_event, &mut self.state);
        if result != EventResult::NotHandled {
            return;
        }

        // Then try active screen
        let result = match self.state.active_tab {
            TabId::PortfolioProfiles => self
                .portfolio_profiles_screen
                .handle_key(key_event, &mut self.state),
            TabId::Scenario => self.scenario_screen.handle_key(key_event, &mut self.state),
            TabId::Events => self.events_screen.handle_key(key_event, &mut self.state),
            TabId::Results => self.results_screen.handle_key(key_event, &mut self.state),
        };

        match result {
            EventResult::Exit => self.state.exit = true,
            _ => {}
        }
    }

    fn handle_modal_result(&mut self, action: ModalAction, value: String) {
        // Extract context from the modal before we clear it
        let context = match &self.state.modal {
            ModalState::Form(form) => form.context.clone(),
            ModalState::Confirm(confirm) => confirm.context.clone(),
            _ => None,
        };

        match action {
            ModalAction::SaveAs => {
                self.state.save_scenario_as(&value);
                self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                    "Success",
                    &format!("Scenario saved as '{}'", value),
                ));
            }
            ModalAction::Load => {
                if self.state.app_data.simulations.contains_key(&value) {
                    self.state.switch_scenario(&value);
                    self.state.modal = ModalState::Message(crate::state::MessageModal::info(
                        "Success",
                        &format!("Switched to scenario '{}'", value),
                    ));
                } else {
                    self.state.modal = ModalState::Message(crate::state::MessageModal::error(
                        "Error",
                        &format!("Scenario '{}' not found", value),
                    ));
                }
            }
            ModalAction::SwitchTo => {
                if self.state.app_data.simulations.contains_key(&value) {
                    self.state.switch_scenario(&value);
                }
                self.state.modal = ModalState::None;
            }

            // ========== Account Category/Type Pickers ==========
            ModalAction::PickAccountCategory => {
                self.handle_account_category_pick(&value);
            }
            ModalAction::PickAccountType => {
                self.handle_account_type_pick(&value);
            }

            // ========== Account CRUD ==========
            ModalAction::CreateAccount => {
                self.handle_create_account(&value, &context);
            }
            ModalAction::EditAccount => {
                self.handle_edit_account(&value, &context);
            }
            ModalAction::DeleteAccount => {
                self.handle_delete_account(&context);
            }

            // ========== Profile CRUD ==========
            ModalAction::PickProfileType => {
                self.handle_profile_type_pick(&value);
            }
            ModalAction::CreateProfile => {
                self.handle_create_profile(&value, &context);
            }
            ModalAction::EditProfile => {
                self.handle_edit_profile(&value, &context);
            }
            ModalAction::DeleteProfile => {
                self.handle_delete_profile(&context);
            }

            // ========== Holding CRUD ==========
            ModalAction::AddHolding => {
                self.handle_add_holding(&value, &context);
            }
            ModalAction::EditHolding => {
                self.handle_edit_holding(&value, &context);
            }
            ModalAction::DeleteHolding => {
                self.handle_delete_holding(&context);
            }

            // ========== Config Editing ==========
            ModalAction::PickFederalBrackets => {
                self.handle_federal_brackets_pick(&value);
            }
            ModalAction::EditTaxConfig => {
                self.handle_edit_tax_config(&value, &context);
            }
            ModalAction::PickInflationType => {
                self.handle_inflation_type_pick(&value);
            }
            ModalAction::EditInflation => {
                self.handle_edit_inflation(&value, &context);
            }

            // ========== Return Profile Picker ==========
            ModalAction::PickReturnProfile => {
                // Used for property account return profile selection
                self.state.modal = ModalState::None;
            }

            // ========== Event CRUD ==========
            ModalAction::PickTriggerType => {
                self.handle_trigger_type_pick(&value);
            }
            ModalAction::PickEffectType => {
                self.handle_effect_type_pick(&value, &context);
            }
            ModalAction::PickAccountForEffect => {
                self.handle_account_for_effect_pick(&value, &context);
            }
            ModalAction::PickEventReference => {
                self.handle_event_reference_pick(&value, &context);
            }
            ModalAction::PickInterval => {
                self.handle_interval_pick(&value, &context);
            }
            ModalAction::CreateEvent => {
                self.handle_create_event(&value, &context);
            }
            ModalAction::EditEvent => {
                self.handle_edit_event(&value, &context);
            }
            ModalAction::DeleteEvent => {
                self.handle_delete_event(&context);
            }
            // ========== Effect Management ==========
            ModalAction::ManageEffects => {
                self.handle_manage_effects(&value);
            }
            ModalAction::PickEffectTypeForAdd => {
                self.handle_effect_type_for_add(&value);
            }
            ModalAction::AddEffect => {
                self.handle_add_effect(&value, &context);
            }
            ModalAction::DeleteEffect => {
                self.handle_delete_effect(&context);
            }
        }
    }

    // ========== Account Handlers ==========

    fn handle_account_category_pick(&mut self, category: &str) {
        let options = match category {
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
        };

        if !options.is_empty() {
            self.state.modal = ModalState::Picker(PickerModal::new(
                "Select Account Type",
                options,
                ModalAction::PickAccountType,
            ));
        } else {
            self.state.modal = ModalState::None;
        }
    }

    fn handle_account_type_pick(&mut self, account_type: &str) {
        // Create the appropriate form based on account type
        let (title, fields, context) = match account_type {
            "Brokerage" | "401(k)" | "Roth 401(k)" | "Traditional IRA" | "Roth IRA" => {
                // Investment accounts
                (
                    "New Investment Account",
                    vec![
                        FormField::text("Name", ""),
                        FormField::text("Description", ""),
                    ],
                    account_type.to_string(),
                )
            }
            "Checking" | "Savings" | "HSA" | "Property" | "Collectible" => {
                // Property-type accounts
                (
                    "New Cash/Property Account",
                    vec![
                        FormField::text("Name", ""),
                        FormField::text("Description", ""),
                        FormField::currency("Value", 0.0),
                        FormField::text("Return Profile", ""),
                    ],
                    account_type.to_string(),
                )
            }
            "Mortgage" | "Loan" | "Student Loan" => {
                // Debt accounts
                (
                    "New Debt Account",
                    vec![
                        FormField::text("Name", ""),
                        FormField::text("Description", ""),
                        FormField::currency("Balance", 0.0),
                        FormField::percentage("Interest Rate", 0.0),
                    ],
                    account_type.to_string(),
                )
            }
            _ => {
                self.state.modal = ModalState::None;
                return;
            }
        };

        self.state.modal = ModalState::Form(
            FormModal::new(title, fields, ModalAction::CreateAccount).with_context(&context),
        );
    }

    fn handle_create_account(&mut self, value: &str, context: &Option<String>) {
        let parts: Vec<&str> = value.split('|').collect();
        let account_type_str = context.as_deref().unwrap_or("");

        let account = match account_type_str {
            "Brokerage" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                Some(AccountData {
                    name,
                    description: desc,
                    account_type: AccountType::Brokerage(AssetAccount { assets: vec![] }),
                })
            }
            "401(k)" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                Some(AccountData {
                    name,
                    description: desc,
                    account_type: AccountType::Traditional401k(AssetAccount { assets: vec![] }),
                })
            }
            "Roth 401(k)" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                Some(AccountData {
                    name,
                    description: desc,
                    account_type: AccountType::Roth401k(AssetAccount { assets: vec![] }),
                })
            }
            "Traditional IRA" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                Some(AccountData {
                    name,
                    description: desc,
                    account_type: AccountType::TraditionalIRA(AssetAccount { assets: vec![] }),
                })
            }
            "Roth IRA" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                Some(AccountData {
                    name,
                    description: desc,
                    account_type: AccountType::RothIRA(AssetAccount { assets: vec![] }),
                })
            }
            "Checking" | "Savings" | "HSA" | "Property" | "Collectible" => {
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

                let account_type = match account_type_str {
                    "Checking" => AccountType::Checking(prop),
                    "Savings" => AccountType::Savings(prop),
                    "HSA" => AccountType::HSA(prop),
                    "Property" => AccountType::Property(prop),
                    "Collectible" => AccountType::Collectible(prop),
                    _ => return,
                };

                Some(AccountData {
                    name,
                    description: desc,
                    account_type,
                })
            }
            "Mortgage" | "Loan" | "Student Loan" => {
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

                let account_type = match account_type_str {
                    "Mortgage" => AccountType::Mortgage(debt),
                    "Loan" => AccountType::LoanDebt(debt),
                    "Student Loan" => AccountType::StudentLoanDebt(debt),
                    _ => return,
                };

                Some(AccountData {
                    name,
                    description: desc,
                    account_type,
                })
            }
            _ => None,
        };

        if let Some(acc) = account {
            self.state.data_mut().portfolios.accounts.push(acc);
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }

    fn handle_edit_account(&mut self, value: &str, context: &Option<String>) {
        let idx = context
            .as_ref()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let parts: Vec<&str> = value.split('|').collect();

        if let Some(account) = self.state.data_mut().portfolios.accounts.get_mut(idx) {
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
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }

    fn handle_delete_account(&mut self, context: &Option<String>) {
        if let Some(idx) = context.as_ref().and_then(|s| s.parse::<usize>().ok()) {
            let accounts_len = self.state.data().portfolios.accounts.len();
            if idx < accounts_len {
                self.state.data_mut().portfolios.accounts.remove(idx);
                let new_len = self.state.data().portfolios.accounts.len();
                // Adjust selected index
                if self.state.portfolio_profiles_state.selected_account_index >= new_len
                    && new_len > 0
                {
                    self.state.portfolio_profiles_state.selected_account_index = new_len - 1;
                }
                self.state.mark_modified();
            }
        }
        self.state.modal = ModalState::None;
    }

    // ========== Profile Handlers ==========

    fn handle_profile_type_pick(&mut self, profile_type: &str) {
        let (title, fields, context) = match profile_type {
            "None" => (
                "New Profile (None)",
                vec![
                    FormField::text("Name", ""),
                    FormField::text("Description", ""),
                ],
                "None".to_string(),
            ),
            "Fixed Rate" => (
                "New Profile (Fixed)",
                vec![
                    FormField::text("Name", ""),
                    FormField::text("Description", ""),
                    FormField::percentage("Rate", 0.07),
                ],
                "Fixed".to_string(),
            ),
            "Normal Distribution" => (
                "New Profile (Normal)",
                vec![
                    FormField::text("Name", ""),
                    FormField::text("Description", ""),
                    FormField::percentage("Mean", 0.07),
                    FormField::percentage("Std Dev", 0.15),
                ],
                "Normal".to_string(),
            ),
            "Log-Normal Distribution" => (
                "New Profile (Log-Normal)",
                vec![
                    FormField::text("Name", ""),
                    FormField::text("Description", ""),
                    FormField::percentage("Mean", 0.07),
                    FormField::percentage("Std Dev", 0.15),
                ],
                "LogNormal".to_string(),
            ),
            _ => {
                self.state.modal = ModalState::None;
                return;
            }
        };

        self.state.modal = ModalState::Form(
            FormModal::new(title, fields, ModalAction::CreateProfile).with_context(&context),
        );
    }

    fn handle_create_profile(&mut self, value: &str, context: &Option<String>) {
        let parts: Vec<&str> = value.split('|').collect();
        let profile_type = context.as_deref().unwrap_or("");

        let name = parts.first().unwrap_or(&"").to_string();
        if name.is_empty() {
            self.state
                .set_error("Profile name cannot be empty".to_string());
            self.state.modal = ModalState::None;
            return;
        }

        let desc = parts
            .get(1)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let profile = match profile_type {
            "None" => ReturnProfileData::None,
            "Fixed" => {
                let rate = parts
                    .get(2)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.07);
                ReturnProfileData::Fixed { rate }
            }
            "Normal" => {
                let mean = parts
                    .get(2)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.07);
                let std_dev = parts
                    .get(3)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.15);
                ReturnProfileData::Normal { mean, std_dev }
            }
            "LogNormal" => {
                let mean = parts
                    .get(2)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.07);
                let std_dev = parts
                    .get(3)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.15);
                ReturnProfileData::LogNormal { mean, std_dev }
            }
            _ => ReturnProfileData::None,
        };

        let profile_data = ProfileData {
            name: ReturnProfileTag(name),
            description: desc,
            profile,
        };

        self.state.data_mut().profiles.push(profile_data);
        self.state.mark_modified();
        self.state.modal = ModalState::None;
    }

    fn handle_edit_profile(&mut self, value: &str, context: &Option<String>) {
        let idx = context
            .as_ref()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let parts: Vec<&str> = value.split('|').collect();

        if let Some(profile_data) = self.state.data_mut().profiles.get_mut(idx) {
            // Parts vary by profile type
            // [name, description, type, ...params]
            if let Some(name) = parts.first()
                && !name.is_empty()
            {
                profile_data.name = ReturnProfileTag(name.to_string());
            }
            profile_data.description = parts
                .get(1)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());

            // Update parameters based on profile type
            match &mut profile_data.profile {
                ReturnProfileData::None => {}
                ReturnProfileData::Fixed { rate } => {
                    if let Some(r) = parts.get(3).and_then(|s| parse_percentage(s).ok()) {
                        *rate = r;
                    }
                }
                ReturnProfileData::Normal { mean, std_dev }
                | ReturnProfileData::LogNormal { mean, std_dev } => {
                    if let Some(m) = parts.get(3).and_then(|s| parse_percentage(s).ok()) {
                        *mean = m;
                    }
                    if let Some(s) = parts.get(4).and_then(|s| parse_percentage(s).ok()) {
                        *std_dev = s;
                    }
                }
            }
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }

    fn handle_delete_profile(&mut self, context: &Option<String>) {
        if let Some(idx) = context.as_ref().and_then(|s| s.parse::<usize>().ok()) {
            let profiles_len = self.state.data().profiles.len();
            if idx < profiles_len {
                self.state.data_mut().profiles.remove(idx);
                let new_len = self.state.data().profiles.len();
                // Adjust selected index
                if self.state.portfolio_profiles_state.selected_profile_index >= new_len
                    && new_len > 0
                {
                    self.state.portfolio_profiles_state.selected_profile_index = new_len - 1;
                }
                self.state.mark_modified();
            }
        }
        self.state.modal = ModalState::None;
    }

    // ========== Holding Handlers ==========

    fn handle_add_holding(&mut self, value: &str, context: &Option<String>) {
        let idx = context
            .as_ref()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let parts: Vec<&str> = value.split('|').collect();
        let asset_name = parts.first().unwrap_or(&"").to_string();
        let asset_value = parts
            .get(1)
            .and_then(|s| parse_currency(s).ok())
            .unwrap_or(0.0);

        if asset_name.is_empty() {
            self.state
                .set_error("Asset name cannot be empty".to_string());
            self.state.modal = ModalState::None;
            return;
        }

        if let Some(account) = self.state.data_mut().portfolios.accounts.get_mut(idx) {
            let assets = match &mut account.account_type {
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => Some(&mut inv.assets),
                _ => None,
            };

            if let Some(assets) = assets {
                assets.push(AssetValue {
                    asset: AssetTag(asset_name),
                    value: asset_value,
                });
                self.state.mark_modified();
            }
        }

        self.state.modal = ModalState::None;
    }

    fn handle_edit_holding(&mut self, value: &str, context: &Option<String>) {
        // Context format: "account_idx:holding_idx"
        let indices: Vec<usize> = context
            .as_ref()
            .map(|s| s.split(':').filter_map(|p| p.parse().ok()).collect())
            .unwrap_or_default();

        if indices.len() != 2 {
            self.state.modal = ModalState::None;
            return;
        }

        let (account_idx, holding_idx) = (indices[0], indices[1]);
        let parts: Vec<&str> = value.split('|').collect();

        if let Some(account) = self
            .state
            .data_mut()
            .portfolios
            .accounts
            .get_mut(account_idx)
        {
            let assets = match &mut account.account_type {
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => Some(&mut inv.assets),
                _ => None,
            };

            if let Some(assets) = assets
                && let Some(holding) = assets.get_mut(holding_idx)
            {
                if let Some(name) = parts.first() {
                    holding.asset = AssetTag(name.to_string());
                }
                if let Some(val) = parts.get(1).and_then(|s| parse_currency(s).ok()) {
                    holding.value = val;
                }
                self.state.mark_modified();
            }
        }

        self.state.modal = ModalState::None;
    }

    fn handle_delete_holding(&mut self, context: &Option<String>) {
        // Context format: "account_idx:holding_idx"
        let indices: Vec<usize> = context
            .as_ref()
            .map(|s| s.split(':').filter_map(|p| p.parse().ok()).collect())
            .unwrap_or_default();

        if indices.len() != 2 {
            self.state.modal = ModalState::None;
            return;
        }

        let (account_idx, holding_idx) = (indices[0], indices[1]);

        if let Some(account) = self
            .state
            .data_mut()
            .portfolios
            .accounts
            .get_mut(account_idx)
        {
            let assets = match &mut account.account_type {
                AccountType::Brokerage(inv)
                | AccountType::Traditional401k(inv)
                | AccountType::Roth401k(inv)
                | AccountType::TraditionalIRA(inv)
                | AccountType::RothIRA(inv) => Some(&mut inv.assets),
                _ => None,
            };

            if let Some(assets) = assets
                && holding_idx < assets.len()
            {
                assets.remove(holding_idx);
                self.state.mark_modified();
            }
        }

        self.state.modal = ModalState::None;
    }

    // ========== Config Handlers ==========

    fn handle_federal_brackets_pick(&mut self, value: &str) {
        let preset = match value {
            "2024 Single" => FederalBracketsPreset::Single2024,
            "2024 Married Joint" => FederalBracketsPreset::MarriedJoint2024,
            _ => FederalBracketsPreset::Single2024,
        };

        self.state.data_mut().parameters.tax_config.federal_brackets = preset;
        self.state.mark_modified();
        self.state.modal = ModalState::None;
    }

    fn handle_edit_tax_config(&mut self, value: &str, context: &Option<String>) {
        let parts: Vec<&str> = value.split('|').collect();
        let config_type = context.as_deref().unwrap_or("");

        match config_type {
            "state_rate" => {
                if let Some(rate) = parts.first().and_then(|s| parse_percentage(s).ok()) {
                    self.state.data_mut().parameters.tax_config.state_rate = rate;
                    self.state.mark_modified();
                }
            }
            "cap_gains_rate" => {
                if let Some(rate) = parts.first().and_then(|s| parse_percentage(s).ok()) {
                    self.state
                        .data_mut()
                        .parameters
                        .tax_config
                        .capital_gains_rate = rate;
                    self.state.mark_modified();
                }
            }
            _ => {}
        }

        self.state.modal = ModalState::None;
    }

    fn handle_inflation_type_pick(&mut self, value: &str) {
        match value {
            "None" => {
                self.state.data_mut().parameters.inflation = InflationData::None;
                self.state.mark_modified();
                self.state.modal = ModalState::None;
            }
            "Fixed" => {
                // Show form for fixed rate
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "Fixed Inflation",
                        vec![FormField::percentage("Rate", 0.03)],
                        ModalAction::EditInflation,
                    )
                    .with_context("Fixed"),
                );
            }
            "Normal" => {
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "Normal Inflation",
                        vec![
                            FormField::percentage("Mean", 0.03),
                            FormField::percentage("Std Dev", 0.02),
                        ],
                        ModalAction::EditInflation,
                    )
                    .with_context("Normal"),
                );
            }
            "Log-Normal" => {
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "Log-Normal Inflation",
                        vec![
                            FormField::percentage("Mean", 0.03),
                            FormField::percentage("Std Dev", 0.02),
                        ],
                        ModalAction::EditInflation,
                    )
                    .with_context("LogNormal"),
                );
            }
            "US Historical" => {
                // Show picker for distribution type
                let options = vec![
                    "Fixed (Mean)".to_string(),
                    "Normal".to_string(),
                    "Log-Normal".to_string(),
                ];
                self.state.modal = ModalState::Picker(PickerModal::new(
                    "Historical Distribution",
                    options,
                    ModalAction::EditInflation,
                ));
            }
            // Handle US Historical distribution sub-selection
            "Fixed (Mean)" => {
                self.state.data_mut().parameters.inflation = InflationData::USHistorical {
                    distribution: DistributionType::Fixed,
                };
                self.state.mark_modified();
                self.state.modal = ModalState::None;
            }
            _ => {
                self.state.modal = ModalState::None;
            }
        }
    }

    fn handle_edit_inflation(&mut self, value: &str, context: &Option<String>) {
        let parts: Vec<&str> = value.split('|').collect();
        let inflation_type = context.as_deref().unwrap_or("");

        match inflation_type {
            "Fixed" => {
                let rate = parts
                    .first()
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.03);
                self.state.data_mut().parameters.inflation = InflationData::Fixed { rate };
                self.state.mark_modified();
            }
            "Normal" => {
                let mean = parts
                    .first()
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.03);
                let std_dev = parts
                    .get(1)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.02);
                self.state.data_mut().parameters.inflation =
                    InflationData::Normal { mean, std_dev };
                self.state.mark_modified();
            }
            "LogNormal" => {
                let mean = parts
                    .first()
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.03);
                let std_dev = parts
                    .get(1)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(0.02);
                self.state.data_mut().parameters.inflation =
                    InflationData::LogNormal { mean, std_dev };
                self.state.mark_modified();
            }
            // Handle US Historical sub-picker selection
            _ if value == "Normal" => {
                self.state.data_mut().parameters.inflation = InflationData::USHistorical {
                    distribution: DistributionType::Normal,
                };
                self.state.mark_modified();
            }
            _ if value == "Log-Normal" => {
                self.state.data_mut().parameters.inflation = InflationData::USHistorical {
                    distribution: DistributionType::LogNormal,
                };
                self.state.mark_modified();
            }
            _ if value == "Fixed (Mean)" => {
                self.state.data_mut().parameters.inflation = InflationData::USHistorical {
                    distribution: DistributionType::Fixed,
                };
                self.state.mark_modified();
            }
            _ => {}
        }

        self.state.modal = ModalState::None;
    }

    // ========== Event Handlers ==========

    fn handle_trigger_type_pick(&mut self, trigger_type: &str) {
        // Based on trigger type, show appropriate form
        let (title, fields, context) = match trigger_type {
            "Date" => (
                "New Event - Date Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::text("Date (YYYY-MM-DD)", "2025-01-01"),
                    FormField::text("Once Only (Y/N)", "N"),
                ],
                "Date".to_string(),
            ),
            "Age" => (
                "New Event - Age Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::text("Age (years)", "65"),
                    FormField::text("Once Only (Y/N)", "Y"),
                ],
                "Age".to_string(),
            ),
            "Repeating" => {
                // Show interval picker first
                let intervals = vec![
                    "Weekly".to_string(),
                    "Bi-Weekly".to_string(),
                    "Monthly".to_string(),
                    "Quarterly".to_string(),
                    "Yearly".to_string(),
                ];
                self.state.modal = ModalState::Picker(PickerModal::new(
                    "Select Repeat Interval",
                    intervals,
                    ModalAction::PickInterval,
                ));
                return;
            }
            "Manual" => (
                "New Event - Manual Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::text("Once Only (Y/N)", "N"),
                ],
                "Manual".to_string(),
            ),
            "Account Balance" => {
                // Get account list
                let accounts = EventsScreen::get_account_names(&self.state);
                if accounts.is_empty() {
                    self.state
                        .set_error("No accounts available. Create an account first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Picker(PickerModal::new(
                    "Select Account for Balance Trigger",
                    accounts,
                    ModalAction::PickAccountForEffect,
                ));
                // Store trigger type in a way we can retrieve later
                // We'll use context format: "AccountBalance|{account}"
                return;
            }
            "Net Worth" => (
                "New Event - Net Worth Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::currency("Threshold", 1000000.0),
                    FormField::text("Comparison (>=/<= )", ">="),
                    FormField::text("Once Only (Y/N)", "Y"),
                ],
                "NetWorth".to_string(),
            ),
            "Relative to Event" => {
                // Get event list
                let events = EventsScreen::get_event_names(&self.state);
                if events.is_empty() {
                    self.state
                        .set_error("No events available. Create an event first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Picker(PickerModal::new(
                    "Select Reference Event",
                    events,
                    ModalAction::PickEventReference,
                ));
                return;
            }
            _ => {
                self.state.modal = ModalState::None;
                return;
            }
        };

        self.state.modal = ModalState::Form(
            FormModal::new(title, fields, ModalAction::CreateEvent)
                .with_context(&context)
                .start_editing(),
        );
    }

    fn handle_interval_pick(&mut self, interval: &str, _context: &Option<String>) {
        // Show form for repeating event with selected interval
        let interval_str = interval.to_string();
        self.state.modal = ModalState::Form(
            FormModal::new(
                &format!("New Event - {} Repeating", interval),
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::read_only("Interval", &interval_str),
                    FormField::text("Start Date (YYYY-MM-DD, optional)", ""),
                    FormField::text("End Age (years, optional)", ""),
                ],
                ModalAction::CreateEvent,
            )
            .with_context(&format!("Repeating|{}", interval))
            .start_editing(),
        );
    }

    fn handle_effect_type_pick(&mut self, _effect_type: &str, _context: &Option<String>) {
        // This would be used for adding effects to events
        // For now, just close the modal - effect adding would be a more complex flow
        self.state.modal = ModalState::None;
    }

    fn handle_account_for_effect_pick(&mut self, account: &str, _context: &Option<String>) {
        // Show form for account balance trigger
        self.state.modal = ModalState::Form(
            FormModal::new(
                "New Event - Account Balance Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::read_only("Account", account),
                    FormField::currency("Threshold", 100000.0),
                    FormField::text("Comparison (>=/<= )", ">="),
                    FormField::text("Once Only (Y/N)", "Y"),
                ],
                ModalAction::CreateEvent,
            )
            .with_context(&format!("AccountBalance|{}", account))
            .start_editing(),
        );
    }

    fn handle_event_reference_pick(&mut self, event_ref: &str, _context: &Option<String>) {
        // Show form for relative event trigger
        self.state.modal = ModalState::Form(
            FormModal::new(
                "New Event - Relative to Event",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::read_only("Reference Event", event_ref),
                    FormField::text("Offset Years", "0"),
                    FormField::text("Offset Months", "0"),
                    FormField::text("Once Only (Y/N)", "Y"),
                ],
                ModalAction::CreateEvent,
            )
            .with_context(&format!("RelativeToEvent|{}", event_ref))
            .start_editing(),
        );
    }

    fn handle_create_event(&mut self, value: &str, context: &Option<String>) {
        let parts: Vec<&str> = value.split('|').collect();
        let trigger_type = context.as_deref().unwrap_or("");

        // Parse trigger type and create appropriate event
        let (trigger, name, description, once) = match trigger_type {
            "Date" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                let date = parts.get(2).unwrap_or(&"2025-01-01").to_string();
                let once = parts
                    .get(3)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(false);

                (TriggerData::Date { date }, name, desc, once)
            }
            "Age" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                let years: u8 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(65);
                let once = parts
                    .get(3)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(true);

                (
                    TriggerData::Age {
                        years,
                        months: None,
                    },
                    name,
                    desc,
                    once,
                )
            }
            "Manual" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                let once = parts
                    .get(2)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(false);

                (TriggerData::Manual, name, desc, once)
            }
            "NetWorth" => {
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                let threshold_val = parts
                    .get(2)
                    .and_then(|s| parse_currency(s).ok())
                    .unwrap_or(1000000.0);
                let comparison = parts.get(3).unwrap_or(&">=");
                let once = parts
                    .get(4)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(true);

                let threshold = if comparison.contains("<=") {
                    ThresholdData::LessThanOrEqual {
                        value: threshold_val,
                    }
                } else {
                    ThresholdData::GreaterThanOrEqual {
                        value: threshold_val,
                    }
                };

                (TriggerData::NetWorth { threshold }, name, desc, once)
            }
            s if s.starts_with("Repeating|") => {
                let interval_str = s.strip_prefix("Repeating|").unwrap_or("Monthly");
                let interval = match interval_str {
                    "Weekly" => IntervalData::Weekly,
                    "Bi-Weekly" => IntervalData::BiWeekly,
                    "Monthly" => IntervalData::Monthly,
                    "Quarterly" => IntervalData::Quarterly,
                    "Yearly" => IntervalData::Yearly,
                    _ => IntervalData::Monthly,
                };

                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                // parts[2] is the read-only interval field
                let start_date = parts.get(3).filter(|s| !s.is_empty());
                let end_age: Option<u8> = parts.get(4).and_then(|s| s.parse().ok());

                let start = start_date.map(|d| {
                    Box::new(TriggerData::Date {
                        date: d.to_string(),
                    })
                });
                let end = end_age.map(|years| {
                    Box::new(TriggerData::Age {
                        years,
                        months: None,
                    })
                });

                (
                    TriggerData::Repeating {
                        interval,
                        start,
                        end,
                    },
                    name,
                    desc,
                    false,
                )
            }
            s if s.starts_with("AccountBalance|") => {
                let account_name = s.strip_prefix("AccountBalance|").unwrap_or("");
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                // parts[2] is the read-only account field
                let threshold_val = parts
                    .get(3)
                    .and_then(|s| parse_currency(s).ok())
                    .unwrap_or(100000.0);
                let comparison = parts.get(4).unwrap_or(&">=");
                let once = parts
                    .get(5)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(true);

                let threshold = if comparison.contains("<=") {
                    ThresholdData::LessThanOrEqual {
                        value: threshold_val,
                    }
                } else {
                    ThresholdData::GreaterThanOrEqual {
                        value: threshold_val,
                    }
                };

                (
                    TriggerData::AccountBalance {
                        account: AccountTag(account_name.to_string()),
                        threshold,
                    },
                    name,
                    desc,
                    once,
                )
            }
            s if s.starts_with("RelativeToEvent|") => {
                let event_ref = s.strip_prefix("RelativeToEvent|").unwrap_or("");
                let name = parts.first().unwrap_or(&"").to_string();
                let desc = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
                // parts[2] is the read-only event ref field
                let offset_years: i32 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
                let offset_months: i32 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
                let once = parts
                    .get(5)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(true);

                let offset = if offset_years != 0 {
                    crate::data::events_data::OffsetData::Years {
                        value: offset_years,
                    }
                } else {
                    crate::data::events_data::OffsetData::Months {
                        value: offset_months,
                    }
                };

                (
                    TriggerData::RelativeToEvent {
                        event: EventTag(event_ref.to_string()),
                        offset,
                    },
                    name,
                    desc,
                    once,
                )
            }
            _ => {
                self.state.modal = ModalState::None;
                return;
            }
        };

        if name.is_empty() {
            self.state
                .set_error("Event name cannot be empty".to_string());
            self.state.modal = ModalState::None;
            return;
        }

        let event = EventData {
            name: EventTag(name),
            description,
            trigger,
            effects: vec![],
            once,
            enabled: true,
        };

        self.state.data_mut().events.push(event);
        // Select the newly created event
        self.state.events_state.selected_event_index = self.state.data().events.len() - 1;
        self.state.mark_modified();
        self.state.modal = ModalState::None;
    }

    fn handle_edit_event(&mut self, value: &str, context: &Option<String>) {
        let idx = context
            .as_ref()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let parts: Vec<&str> = value.split('|').collect();

        if let Some(event) = self.state.data_mut().events.get_mut(idx) {
            // Parts: [name, description, once, enabled, trigger (ro), effects (ro)]
            if let Some(name) = parts.first()
                && !name.is_empty()
            {
                event.name = EventTag(name.to_string());
            }
            event.description = parts
                .get(1)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            if let Some(once_str) = parts.get(2) {
                event.once = once_str.to_uppercase().starts_with('Y');
            }
            if let Some(enabled_str) = parts.get(3) {
                event.enabled = enabled_str.to_uppercase().starts_with('Y');
            }
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }

    fn handle_delete_event(&mut self, context: &Option<String>) {
        if let Some(idx) = context.as_ref().and_then(|s| s.parse::<usize>().ok()) {
            let events_len = self.state.data().events.len();
            if idx < events_len {
                self.state.data_mut().events.remove(idx);
                let new_len = self.state.data().events.len();
                // Adjust selected index
                if self.state.events_state.selected_event_index >= new_len && new_len > 0 {
                    self.state.events_state.selected_event_index = new_len - 1;
                }
                self.state.mark_modified();
            }
        }
        self.state.modal = ModalState::None;
    }

    // ========== Effect Handlers ==========

    fn handle_manage_effects(&mut self, selected: &str) {
        let event_idx = self.state.events_state.selected_event_index;

        if selected == "[ + Add New Effect ]" {
            // Show effect type picker
            let effect_types = vec![
                "Income".to_string(),
                "Expense".to_string(),
                "Trigger Event".to_string(),
                "Pause Event".to_string(),
                "Resume Event".to_string(),
                "Terminate Event".to_string(),
            ];
            self.state.modal = ModalState::Picker(PickerModal::new(
                "Select Effect Type",
                effect_types,
                ModalAction::PickEffectTypeForAdd,
            ));
        } else {
            // Parse effect index from "N. description" format
            if let Some(effect_idx) = selected
                .split('.')
                .next()
                .and_then(|s| s.parse::<usize>().ok())
            {
                let effect_idx = effect_idx - 1; // Convert to 0-based index
                if let Some(event) = self.state.data().events.get(event_idx)
                    && let Some(effect) = event.effects.get(effect_idx)
                {
                    let effect_desc = EventsScreen::format_effect(effect);
                    self.state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Effect",
                            &format!("Delete effect: {}?", effect_desc),
                            ModalAction::DeleteEffect,
                        )
                        .with_context(&format!("{}:{}", event_idx, effect_idx)),
                    );
                    return;
                }
            }
            self.state.modal = ModalState::None;
        }
    }

    fn handle_effect_type_for_add(&mut self, effect_type: &str) {
        let event_idx = self.state.events_state.selected_event_index;
        let accounts = EventsScreen::get_account_names(&self.state);
        let events = EventsScreen::get_event_names(&self.state);

        let first_account = accounts.first().map(|s| s.as_str()).unwrap_or("");
        let first_event = events.first().map(|s| s.as_str()).unwrap_or("");

        match effect_type {
            "Income" => {
                if accounts.is_empty() {
                    self.state
                        .set_error("No accounts available. Create an account first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Income Effect",
                        vec![
                            FormField::text("To Account", first_account),
                            FormField::currency("Amount", 0.0),
                            FormField::text("Gross (Y/N)", "N"),
                            FormField::text("Taxable (Y/N)", "Y"),
                        ],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("Income|{}", event_idx))
                    .start_editing(),
                );
            }
            "Expense" => {
                if accounts.is_empty() {
                    self.state
                        .set_error("No accounts available. Create an account first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Expense Effect",
                        vec![
                            FormField::text("From Account", first_account),
                            FormField::currency("Amount", 0.0),
                        ],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("Expense|{}", event_idx))
                    .start_editing(),
                );
            }
            "Trigger Event" => {
                if events.is_empty() {
                    self.state
                        .set_error("No events available. Create an event first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Trigger Effect",
                        vec![FormField::text("Event to Trigger", first_event)],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("TriggerEvent|{}", event_idx))
                    .start_editing(),
                );
            }
            "Pause Event" => {
                if events.is_empty() {
                    self.state
                        .set_error("No events available. Create an event first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Pause Effect",
                        vec![FormField::text("Event to Pause", first_event)],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("PauseEvent|{}", event_idx))
                    .start_editing(),
                );
            }
            "Resume Event" => {
                if events.is_empty() {
                    self.state
                        .set_error("No events available. Create an event first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Resume Effect",
                        vec![FormField::text("Event to Resume", first_event)],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("ResumeEvent|{}", event_idx))
                    .start_editing(),
                );
            }
            "Terminate Event" => {
                if events.is_empty() {
                    self.state
                        .set_error("No events available. Create an event first.".to_string());
                    self.state.modal = ModalState::None;
                    return;
                }
                self.state.modal = ModalState::Form(
                    FormModal::new(
                        "New Terminate Effect",
                        vec![FormField::text("Event to Terminate", first_event)],
                        ModalAction::AddEffect,
                    )
                    .with_context(&format!("TerminateEvent|{}", event_idx))
                    .start_editing(),
                );
            }
            _ => {
                self.state.modal = ModalState::None;
            }
        }
    }

    fn handle_add_effect(&mut self, value: &str, context: &Option<String>) {
        let ctx = context.as_deref().unwrap_or("");
        let parts: Vec<&str> = ctx.split('|').collect();
        let effect_type = parts.first().copied().unwrap_or("");
        let event_idx: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

        let form_parts: Vec<&str> = value.split('|').collect();

        let effect = match effect_type {
            "Income" => {
                let to_account = form_parts.first().unwrap_or(&"").to_string();
                let amount = form_parts
                    .get(1)
                    .and_then(|s| parse_currency(s).ok())
                    .unwrap_or(0.0);
                let gross = form_parts
                    .get(2)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(false);
                let taxable = form_parts
                    .get(3)
                    .map(|s| s.to_uppercase().starts_with('Y'))
                    .unwrap_or(true);

                Some(EffectData::Income {
                    to: AccountTag(to_account),
                    amount: AmountData::Fixed(amount),
                    gross,
                    taxable,
                })
            }
            "Expense" => {
                let from_account = form_parts.first().unwrap_or(&"").to_string();
                let amount = form_parts
                    .get(1)
                    .and_then(|s| parse_currency(s).ok())
                    .unwrap_or(0.0);

                Some(EffectData::Expense {
                    from: AccountTag(from_account),
                    amount: AmountData::Fixed(amount),
                })
            }
            "TriggerEvent" => {
                let event_name = form_parts.first().unwrap_or(&"").to_string();
                Some(EffectData::TriggerEvent {
                    event: EventTag(event_name),
                })
            }
            "PauseEvent" => {
                let event_name = form_parts.first().unwrap_or(&"").to_string();
                Some(EffectData::PauseEvent {
                    event: EventTag(event_name),
                })
            }
            "ResumeEvent" => {
                let event_name = form_parts.first().unwrap_or(&"").to_string();
                Some(EffectData::ResumeEvent {
                    event: EventTag(event_name),
                })
            }
            "TerminateEvent" => {
                let event_name = form_parts.first().unwrap_or(&"").to_string();
                Some(EffectData::TerminateEvent {
                    event: EventTag(event_name),
                })
            }
            _ => None,
        };

        if let Some(effect) = effect
            && let Some(event) = self.state.data_mut().events.get_mut(event_idx)
        {
            event.effects.push(effect);
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }

    fn handle_delete_effect(&mut self, context: &Option<String>) {
        // Context format: "event_idx:effect_idx"
        let indices: Vec<usize> = context
            .as_ref()
            .map(|s| s.split(':').filter_map(|p| p.parse().ok()).collect())
            .unwrap_or_default();

        if indices.len() != 2 {
            self.state.modal = ModalState::None;
            return;
        }

        let (event_idx, effect_idx) = (indices[0], indices[1]);

        if let Some(event) = self.state.data_mut().events.get_mut(event_idx)
            && effect_idx < event.effects.len()
        {
            event.effects.remove(effect_idx);
            self.state.mark_modified();
        }

        self.state.modal = ModalState::None;
    }
}
