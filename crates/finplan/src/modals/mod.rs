// Modal state types
mod action;
pub mod amount_builder;
pub mod context;
mod handler;
mod state;

// Modal UI components
mod confirm;
mod form;
pub mod helpers;
mod message;
mod picker;
mod scenario_picker;
mod text_input;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::actions::{get_assets_for_account, get_assets_for_sale};
use crate::state::AppState;

// Re-export modal types
pub use action::*;
pub use context::ModalContext;
pub use handler::ModalHandler;
pub use state::*;

// Re-export modal UI rendering functions
pub use confirm::render_confirm_modal;
pub use form::{
    format_currency_for_edit, format_percentage_for_edit, parse_currency, parse_percentage,
    render_form_modal,
};
pub use message::render_message_modal;
pub use picker::render_picker_modal;
pub use scenario_picker::render_scenario_picker_modal;
pub use text_input::render_text_input_modal;

/// Typed value returned when a modal is confirmed
#[derive(Debug, Clone)]
pub enum ConfirmedValue {
    /// Form modal with typed field access (boxed to reduce enum size)
    Form(Box<FormModal>),
    /// Picker modal - selected option string
    Picker(String),
    /// Text input modal - entered text
    Text(String),
    /// Confirm modal - just confirmed, no value
    Confirm,
}

impl ConfirmedValue {
    /// Get the form if this is a Form variant
    pub fn as_form(&self) -> Option<&FormModal> {
        match self {
            ConfirmedValue::Form(form) => Some(form),
            _ => None,
        }
    }

    /// Get the selected string for Picker or Text variants
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ConfirmedValue::Picker(s) | ConfirmedValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Result of handling a modal key event
#[derive(Debug)]
pub enum ModalResult {
    /// Modal confirmed with action and typed value
    Confirmed(ModalAction, Box<ConfirmedValue>),
    /// Modal was cancelled
    Cancelled,
    /// Key was handled, modal still active
    Continue,
    /// A field value changed, may need to update dependent fields
    FieldChanged(usize),
    /// An Amount field was activated and needs the amount editor launched
    /// Contains the field index
    AmountFieldActivated(usize),
    /// A Trigger field was activated and needs the trigger editor launched
    /// Contains the field index
    TriggerFieldActivated(usize),
}

/// Render the active modal as an overlay
pub fn render_modal(frame: &mut Frame, state: &mut AppState) {
    match &mut state.modal {
        ModalState::None => {}
        ModalState::TextInput(modal) => {
            render_text_input_modal(frame, modal);
        }
        ModalState::Message(modal) => {
            render_message_modal(frame, modal);
        }
        ModalState::ScenarioPicker(modal) => {
            render_scenario_picker_modal(frame, modal);
        }
        ModalState::Picker(modal) => {
            render_picker_modal(frame, modal);
        }
        ModalState::Form(modal) => {
            render_form_modal(frame, modal);
        }
        ModalState::Confirm(modal) => {
            render_confirm_modal(frame, modal);
        }
    }
}

/// Handle key events for the active modal
pub fn handle_modal_key(key: KeyEvent, state: &mut AppState) -> ModalResult {
    let keybindings = &state.keybindings;
    let result = match &mut state.modal {
        ModalState::None => ModalResult::Continue,
        ModalState::TextInput(modal) => text_input::handle_text_input_key(key, modal),
        ModalState::Message(_) => message::handle_message_key(key),
        ModalState::ScenarioPicker(modal) => {
            scenario_picker::handle_scenario_picker_key(key, modal, keybindings)
        }
        ModalState::Picker(modal) => picker::handle_picker_key(key, modal, keybindings),
        ModalState::Form(modal) => form::handle_form_key(key, modal, keybindings),
        ModalState::Confirm(modal) => confirm::handle_confirm_key(key, modal),
    };

    // Handle dependent field updates for form modals
    if let ModalResult::FieldChanged(field_idx) = result {
        update_dependent_fields(state, field_idx);
        return ModalResult::Continue;
    }

    result
}

/// Update dependent fields when a form field value changes
fn update_dependent_fields(state: &mut AppState, field_idx: usize) {
    // First, check if we need to update and get necessary data (immutable borrow)
    let update_info = {
        let ModalState::Form(modal) = &state.modal else {
            return;
        };

        match modal.kind {
            // Asset Purchase: "To Account" field affects "Asset" field options
            FormKind::AssetPurchase if field_idx == asset_purchase_fields::TO_ACCOUNT => {
                let to_account = modal.fields[asset_purchase_fields::TO_ACCOUNT]
                    .value
                    .clone();
                let current_asset = modal.fields[asset_purchase_fields::ASSET].value.clone();
                let assets = get_assets_for_account(state, &to_account);
                Some((asset_purchase_fields::ASSET, assets, current_asset))
            }
            // Asset Sale: "From Account" field affects "Asset" field options
            FormKind::AssetSale if field_idx == asset_sale_fields::FROM_ACCOUNT => {
                let from_account = modal.fields[asset_sale_fields::FROM_ACCOUNT].value.clone();
                let current_asset = modal.fields[asset_sale_fields::ASSET].value.clone();
                let assets = get_assets_for_sale(state, &from_account);
                Some((asset_sale_fields::ASSET, assets, current_asset))
            }
            // Chart Config: "Chart Type" affects "Y Parameter" options
            FormKind::ChartConfig
                if field_idx == chart_config_fields::CHART_TYPE
                    && modal.fields.len() > chart_config_fields::Y_PARAMETER =>
            {
                let chart_type = modal.fields[chart_config_fields::CHART_TYPE].value.clone();
                let is_2d = chart_type.contains("2D") || chart_type.contains("Heatmap");

                // Get current Y param options (the full parameter list is stored there)
                let y_field = &modal.fields[chart_config_fields::Y_PARAMETER];
                let current_value = y_field.value.clone();

                // For 1D, show only "N/A"; for 2D, show full parameter list
                let new_options = if is_2d {
                    // Restore parameter options from X parameter field (they have the same options)
                    modal.fields[chart_config_fields::X_PARAMETER]
                        .options
                        .clone()
                } else {
                    vec!["N/A".to_string()]
                };

                Some((chart_config_fields::Y_PARAMETER, new_options, current_value))
            }
            _ => None,
        }
    };

    // Now apply the update (mutable borrow)
    if let Some((field_idx, new_options, current_value)) = update_info {
        let ModalState::Form(modal) = &mut state.modal else {
            return;
        };

        // Update the dependent field options
        modal.fields[field_idx].options = new_options;

        // Keep current selection if still valid, otherwise select first
        if !modal.fields[field_idx].options.contains(&current_value) {
            modal.fields[field_idx].value = modal.fields[field_idx]
                .options
                .first()
                .cloned()
                .unwrap_or_default();
        }
    }
}

/// Create a centered rectangle within the given area
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
