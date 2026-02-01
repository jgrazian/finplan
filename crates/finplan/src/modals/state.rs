/// Modal types for forms, pickers, and confirmations.
use super::action::ModalAction;
use super::amount_builder::format_amount_summary;
use super::context::ModalContext;
use crate::data::events_data::AmountData;

#[derive(Debug)]
pub enum ModalState {
    None,
    TextInput(TextInputModal),
    Message(MessageModal),
    ScenarioPicker(ScenarioPickerModal),
    Picker(PickerModal),
    Form(FormModal),
    Confirm(ConfirmModal),
}

#[derive(Debug)]
pub struct ScenarioPickerModal {
    pub title: String,
    pub scenarios: Vec<String>,
    pub selected_index: usize,
    pub action: ModalAction,
    /// For SaveAs: allow entering a new name
    pub new_name: Option<String>,
    pub editing_new_name: bool,
}

impl ScenarioPickerModal {
    pub fn new(title: &str, scenarios: Vec<String>, action: ModalAction) -> Self {
        Self {
            title: title.to_string(),
            scenarios,
            selected_index: 0,
            action,
            new_name: if action == ModalAction::SAVE_AS {
                Some(String::new())
            } else {
                None
            },
            editing_new_name: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.editing_new_name {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.editing_new_name {
            return;
        }
        // +1 for "New scenario" option when saving
        let max_index = if self.action == ModalAction::SAVE_AS {
            self.scenarios.len()
        } else {
            self.scenarios.len().saturating_sub(1)
        };
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    pub fn selected_name(&self) -> Option<String> {
        if self.action == ModalAction::SAVE_AS && self.selected_index == self.scenarios.len() {
            // "New scenario" selected
            self.new_name.clone()
        } else {
            self.scenarios.get(self.selected_index).cloned()
        }
    }

    pub fn is_new_scenario_selected(&self) -> bool {
        self.action == ModalAction::SAVE_AS && self.selected_index == self.scenarios.len()
    }
}

#[derive(Debug)]
pub struct TextInputModal {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor_pos: usize,
    pub action: ModalAction,
}

impl TextInputModal {
    pub fn new(title: &str, prompt: &str, default_value: &str, action: ModalAction) -> Self {
        let value = default_value.to_string();
        let cursor_pos = value.len();
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            value,
            cursor_pos,
            action,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.cursor_pos += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.value.len();
    }
}

#[derive(Debug)]
pub struct MessageModal {
    pub title: String,
    pub message: String,
    pub is_error: bool,
}

impl MessageModal {
    pub fn info(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            is_error: false,
        }
    }

    pub fn error(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            is_error: true,
        }
    }
}

// ========== PickerModal ==========

#[derive(Debug)]
pub struct PickerModal {
    pub title: String,
    pub options: Vec<String>,
    pub selected_index: usize,
    pub action: ModalAction,
    /// Context data for the picker (e.g., indices for subsequent actions)
    pub context: Option<ModalContext>,
}

impl PickerModal {
    pub fn new(title: &str, options: Vec<String>, action: ModalAction) -> Self {
        Self {
            title: title.to_string(),
            options,
            selected_index: 0,
            action,
            context: None,
        }
    }

    /// Set typed context
    pub fn with_typed_context(mut self, context: ModalContext) -> Self {
        self.context = Some(context);
        self
    }
}

// ========== FormModal ==========

/// Form kind for type-safe dispatch of form-specific behavior.
/// Only forms with special runtime behavior (e.g., dependent fields) need explicit kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormKind {
    /// Asset purchase effect - has dependent fields (To Account → Asset)
    AssetPurchase,
    /// Asset sale effect - may need dependent fields (From Account → Asset)
    AssetSale,
    /// All other forms without special behavior
    #[default]
    Generic,
}

/// Field indices for AssetPurchase form
pub mod asset_purchase_fields {
    pub const FROM_ACCOUNT: usize = 0;
    pub const TO_ACCOUNT: usize = 1;
    pub const ASSET: usize = 2;
    pub const AMOUNT: usize = 3;
}

/// Field indices for AssetSale form
pub mod asset_sale_fields {
    pub const FROM_ACCOUNT: usize = 0;
    pub const ASSET: usize = 1;
    pub const AMOUNT: usize = 2;
    pub const AMOUNT_TYPE: usize = 3;
    pub const LOT_METHOD: usize = 4;
    /// Special value meaning "sell all assets" (liquidate)
    pub const ALL_ASSETS: &str = "[All]";
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    Text,
    Currency,
    Percentage,
    ReadOnly,
    /// Select from a list of options (options stored in FormField.options)
    Select,
    /// Complex amount with recursive structure (displayed as summary, edited via modal)
    Amount(Box<AmountData>),
    /// Trigger field - displays summary, Enter opens trigger editor
    /// Stores the trigger summary string for display
    Trigger,
}

#[derive(Debug, Clone)]
pub struct FormField {
    pub label: String,
    pub field_type: FieldType,
    pub value: String,
    pub cursor_pos: usize,
    /// Options for Select field type
    pub options: Vec<String>,
}

impl FormField {
    pub fn new(label: &str, field_type: FieldType, value: &str) -> Self {
        Self {
            label: label.to_string(),
            field_type,
            value: value.to_string(),
            cursor_pos: 0,
            options: Vec::new(),
        }
    }

    pub fn text(label: &str, value: &str) -> Self {
        Self::new(label, FieldType::Text, value)
    }

    pub fn currency(label: &str, value: f64) -> Self {
        Self::new(label, FieldType::Currency, &format!("{:.2}", value))
    }

    pub fn percentage(label: &str, rate: f64) -> Self {
        // Store as display value (e.g., 5.0 for 5%)
        Self::new(
            label,
            FieldType::Percentage,
            &format!("{:.2}", rate * 100.0),
        )
    }

    pub fn read_only(label: &str, value: &str) -> Self {
        Self::new(label, FieldType::ReadOnly, value)
    }

    /// Create a select field with options. The first option matching `selected` will be selected,
    /// or the first option if no match. Pass empty string for `selected` to select first option.
    pub fn select(label: &str, options: Vec<String>, selected: &str) -> Self {
        let value = if options.iter().any(|o| o == selected) {
            selected.to_string()
        } else {
            options.first().cloned().unwrap_or_default()
        };
        Self {
            label: label.to_string(),
            field_type: FieldType::Select,
            value,
            cursor_pos: 0,
            options,
        }
    }

    /// Create an amount field with recursive AmountData structure.
    /// The value is displayed as a summary; editing opens a nested modal.
    pub fn amount(label: &str, amount: AmountData) -> Self {
        let value = format_amount_summary(&amount);
        Self {
            label: label.to_string(),
            field_type: FieldType::Amount(Box::new(amount)),
            value,
            cursor_pos: 0,
            options: Vec::new(),
        }
    }

    /// Get the AmountData if this is an Amount field
    pub fn as_amount(&self) -> Option<&AmountData> {
        match &self.field_type {
            FieldType::Amount(amount) => Some(amount),
            _ => None,
        }
    }

    /// Create a trigger field that shows the trigger summary and opens editor on Enter
    pub fn trigger(label: &str, summary: &str) -> Self {
        Self {
            label: label.to_string(),
            field_type: FieldType::Trigger,
            value: summary.to_string(),
            cursor_pos: 0,
            options: Vec::new(),
        }
    }

    /// Update the amount data (for Amount fields)
    pub fn set_amount(&mut self, amount: AmountData) {
        self.value = format_amount_summary(&amount);
        self.field_type = FieldType::Amount(Box::new(amount));
    }

    /// Get the index of the currently selected option (for Select fields)
    pub fn selected_index(&self) -> usize {
        self.options
            .iter()
            .position(|o| o == &self.value)
            .unwrap_or(0)
    }

    /// Select the next option (for Select fields)
    pub fn select_next(&mut self) {
        if self.options.is_empty() {
            return;
        }
        let idx = (self.selected_index() + 1) % self.options.len();
        self.value = self.options[idx].clone();
    }

    /// Select the previous option (for Select fields)
    pub fn select_prev(&mut self) {
        if self.options.is_empty() {
            return;
        }
        let idx = if self.selected_index() == 0 {
            self.options.len() - 1
        } else {
            self.selected_index() - 1
        };
        self.value = self.options[idx].clone();
    }
}

#[derive(Debug, Clone)]
pub struct FormModal {
    pub title: String,
    pub fields: Vec<FormField>,
    pub focused_field: usize,
    pub editing: bool,
    pub action: ModalAction,
    /// Context data for the form (e.g., account index being edited)
    pub context: Option<ModalContext>,
    /// Form kind for type-safe dispatch of special behavior
    pub kind: FormKind,
    /// Original value of field being edited (for Esc to revert)
    pub editing_original_value: Option<String>,
}

impl FormModal {
    pub fn new(title: &str, fields: Vec<FormField>, action: ModalAction) -> Self {
        // Find first editable field
        let first_editable = fields
            .iter()
            .position(|f| f.field_type != FieldType::ReadOnly)
            .unwrap_or(0);

        Self {
            title: title.to_string(),
            fields,
            focused_field: first_editable,
            editing: false,
            action,
            context: None,
            kind: FormKind::default(),
            editing_original_value: None,
        }
    }

    /// Set the form kind for type-safe dispatch
    pub fn with_kind(mut self, kind: FormKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set typed context
    pub fn with_typed_context(mut self, context: ModalContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Start in editing mode (for better UX)
    pub fn start_editing(mut self) -> Self {
        if !self.fields.is_empty()
            && self.fields[self.focused_field].field_type != FieldType::ReadOnly
        {
            self.editing = true;
            self.editing_original_value = Some(self.fields[self.focused_field].value.clone());
            self.fields[self.focused_field].cursor_pos =
                self.fields[self.focused_field].value.len();
        }
        self
    }

    // ========== Typed Field Extraction ==========

    /// Get a string value from a field by index
    pub fn get_str(&self, index: usize) -> Option<&str> {
        self.fields.get(index).map(|f| f.value.as_str())
    }

    /// Get a non-empty string value from a field by index
    pub fn get_str_non_empty(&self, index: usize) -> Option<&str> {
        self.fields
            .get(index)
            .map(|f| f.value.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Get an optional string (returns Some only if non-empty)
    pub fn get_optional_str(&self, index: usize) -> Option<String> {
        self.get_str_non_empty(index).map(|s| s.to_string())
    }

    /// Get a currency value (f64) from a field by index
    /// Handles $ prefix and commas
    pub fn get_currency(&self, index: usize) -> Option<f64> {
        self.fields.get(index).and_then(|f| {
            let s = f.value.trim().trim_start_matches('$').replace(',', "");
            s.parse().ok()
        })
    }

    /// Get a currency value with a default if parsing fails
    pub fn get_currency_or(&self, index: usize, default: f64) -> f64 {
        self.get_currency(index).unwrap_or(default)
    }

    /// Get a percentage value as a decimal (e.g., "5.0" -> 0.05)
    pub fn get_percentage(&self, index: usize) -> Option<f64> {
        self.fields.get(index).and_then(|f| {
            let s = f.value.trim().trim_end_matches('%');
            s.parse::<f64>().ok().map(|v| v / 100.0)
        })
    }

    /// Get a percentage value with a default if parsing fails
    pub fn get_percentage_or(&self, index: usize, default: f64) -> f64 {
        self.get_percentage(index).unwrap_or(default)
    }

    /// Get a boolean value (Y/N, Yes/No, true/false)
    pub fn get_bool(&self, index: usize) -> Option<bool> {
        self.fields.get(index).map(|f| {
            let s = f.value.to_uppercase();
            s.starts_with('Y') || s == "TRUE" || s == "1"
        })
    }

    /// Get a boolean value with a default
    pub fn get_bool_or(&self, index: usize, default: bool) -> bool {
        self.get_bool(index).unwrap_or(default)
    }

    /// Get an integer value
    pub fn get_int<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.fields
            .get(index)
            .and_then(|f| f.value.trim().parse().ok())
    }

    /// Get an integer value with a default
    pub fn get_int_or<T: std::str::FromStr>(&self, index: usize, default: T) -> T {
        self.get_int(index).unwrap_or(default)
    }

    /// Get an AmountData value from a field by index
    pub fn get_amount(&self, index: usize) -> Option<AmountData> {
        self.fields.get(index).and_then(|f| match &f.field_type {
            FieldType::Amount(amount) => Some((**amount).clone()),
            _ => None,
        })
    }

    /// Get an AmountData value with a default if not found or not an Amount field
    pub fn get_amount_or(&self, index: usize, default: AmountData) -> AmountData {
        self.get_amount(index).unwrap_or(default)
    }

    /// Get all field values as a FormValues helper for convenient access
    pub fn values(&self) -> FormValues<'_> {
        FormValues { form: self }
    }

    // ========== Label-Based Field Access ==========
    //
    // These methods access fields by label instead of index, making code
    // more readable and resistant to field reordering.

    /// Find a field by its label
    fn field_by_label(&self, label: &str) -> Option<&FormField> {
        self.fields.iter().find(|f| f.label == label)
    }

    /// Get a string value from a field by label
    pub fn str(&self, label: &str) -> Option<&str> {
        self.field_by_label(label).map(|f| f.value.as_str())
    }

    /// Get a non-empty string value by label (returns None if empty)
    pub fn str_non_empty(&self, label: &str) -> Option<&str> {
        self.field_by_label(label)
            .map(|f| f.value.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Get an optional string by label (returns Some only if non-empty)
    pub fn optional_str(&self, label: &str) -> Option<String> {
        self.str_non_empty(label).map(|s| s.to_string())
    }

    /// Get a currency value by label. Handles $ prefix and commas.
    pub fn currency(&self, label: &str) -> Option<f64> {
        self.field_by_label(label).and_then(|f| {
            let s = f.value.trim().trim_start_matches('$').replace(',', "");
            s.parse().ok()
        })
    }

    /// Get a currency value by label with a default
    pub fn currency_or(&self, label: &str, default: f64) -> f64 {
        self.currency(label).unwrap_or(default)
    }

    /// Get a percentage value by label as a decimal (e.g., "5.0" -> 0.05)
    pub fn percentage(&self, label: &str) -> Option<f64> {
        self.field_by_label(label).and_then(|f| {
            let s = f.value.trim().trim_end_matches('%');
            s.parse::<f64>().ok().map(|v| v / 100.0)
        })
    }

    /// Get a percentage value by label with a default
    pub fn percentage_or(&self, label: &str, default: f64) -> f64 {
        self.percentage(label).unwrap_or(default)
    }

    /// Get a boolean value by label (Y/N, Yes/No, true/false)
    pub fn bool(&self, label: &str) -> Option<bool> {
        self.field_by_label(label).map(|f| {
            let s = f.value.to_uppercase();
            s.starts_with('Y') || s == "TRUE" || s == "1"
        })
    }

    /// Get a boolean value by label with a default
    pub fn bool_or(&self, label: &str, default: bool) -> bool {
        self.bool(label).unwrap_or(default)
    }

    /// Get an integer value by label
    pub fn int<T: std::str::FromStr>(&self, label: &str) -> Option<T> {
        self.field_by_label(label)
            .and_then(|f| f.value.trim().parse().ok())
    }

    /// Get an integer value by label with a default
    pub fn int_or<T: std::str::FromStr>(&self, label: &str, default: T) -> T {
        self.int(label).unwrap_or(default)
    }

    /// Get an AmountData value by label
    pub fn amount(&self, label: &str) -> Option<AmountData> {
        self.field_by_label(label)
            .and_then(|f| match &f.field_type {
                FieldType::Amount(amount) => Some((**amount).clone()),
                _ => None,
            })
    }

    /// Get an AmountData value by label with a default
    pub fn amount_or(&self, label: &str, default: AmountData) -> AmountData {
        self.amount(label).unwrap_or(default)
    }
}

/// Helper struct for convenient typed access to form values
pub struct FormValues<'a> {
    form: &'a FormModal,
}

impl<'a> FormValues<'a> {
    /// Get string at index
    pub fn str(&self, index: usize) -> &str {
        self.form.get_str(index).unwrap_or("")
    }

    /// Get non-empty string at index as Option
    pub fn optional_str(&self, index: usize) -> Option<String> {
        self.form.get_optional_str(index)
    }

    /// Get currency at index with default
    pub fn currency(&self, index: usize, default: f64) -> f64 {
        self.form.get_currency_or(index, default)
    }

    /// Get percentage at index with default (as decimal)
    pub fn percentage(&self, index: usize, default: f64) -> f64 {
        self.form.get_percentage_or(index, default)
    }

    /// Get boolean at index with default
    pub fn bool(&self, index: usize, default: bool) -> bool {
        self.form.get_bool_or(index, default)
    }

    /// Get integer at index with default
    pub fn int<T: std::str::FromStr>(&self, index: usize, default: T) -> T {
        self.form.get_int_or(index, default)
    }
}

// ========== ConfirmModal ==========

#[derive(Debug)]
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    pub action: ModalAction,
    /// Context data for the confirmation (e.g., index of item to delete)
    pub context: Option<ModalContext>,
}

impl ConfirmModal {
    pub fn new(title: &str, message: &str, action: ModalAction) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            action,
            context: None,
        }
    }

    /// Set typed context
    pub fn with_typed_context(mut self, context: ModalContext) -> Self {
        self.context = Some(context);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_modal_typed_extraction() {
        let form = FormModal::new(
            "Test",
            vec![
                FormField::text("Name", "John Doe"),
                FormField::text("Description", ""),
                FormField::currency("Amount", 1234.56),
                FormField::percentage("Rate", 0.075),
                FormField::text("Active (Y/N)", "Y"),
                FormField::text("Age", "25"),
            ],
            ModalAction::CREATE_ACCOUNT,
        );

        // Test string extraction
        assert_eq!(form.get_str(0), Some("John Doe"));
        assert_eq!(form.get_str_non_empty(0), Some("John Doe"));
        assert_eq!(form.get_str_non_empty(1), None); // Empty string
        assert_eq!(form.get_optional_str(0), Some("John Doe".to_string()));
        assert_eq!(form.get_optional_str(1), None);

        // Test currency extraction
        assert_eq!(form.get_currency(2), Some(1234.56));
        assert_eq!(form.get_currency_or(2, 0.0), 1234.56);
        assert_eq!(form.get_currency_or(99, 100.0), 100.0); // Out of bounds

        // Test percentage extraction (stored as display value, converted to decimal)
        assert!((form.get_percentage(3).unwrap() - 0.075).abs() < 0.0001);
        assert!((form.get_percentage_or(3, 0.0) - 0.075).abs() < 0.0001);

        // Test boolean extraction
        assert_eq!(form.get_bool(4), Some(true));
        assert!(form.get_bool_or(4, false));

        // Test integer extraction
        assert_eq!(form.get_int::<u32>(5), Some(25));
        assert_eq!(form.get_int_or::<u32>(5, 0), 25);
        assert_eq!(form.get_int_or::<u32>(99, 0), 0); // Out of bounds
    }

    #[test]
    fn test_form_values_helper() {
        let form = FormModal::new(
            "Test",
            vec![
                FormField::text("Name", "Test Name"),
                FormField::currency("Amount", 500.0),
                FormField::percentage("Rate", 0.05),
                FormField::text("Enabled", "N"),
            ],
            ModalAction::CREATE_ACCOUNT,
        );

        let values = form.values();

        assert_eq!(values.str(0), "Test Name");
        assert_eq!(values.currency(1, 0.0), 500.0);
        assert!((values.percentage(2, 0.0) - 0.05).abs() < 0.0001);
        assert!(!values.bool(3, true));
    }

    #[test]
    fn test_label_based_access() {
        let form = FormModal::new(
            "Test",
            vec![
                FormField::text("Name", "John Doe"),
                FormField::text("Description", ""),
                FormField::currency("Amount", 1234.56),
                FormField::percentage("Rate", 0.075),
                FormField::select("Active", vec!["Yes".to_string(), "No".to_string()], "Yes"),
                FormField::text("Age", "25"),
            ],
            ModalAction::CREATE_ACCOUNT,
        );

        // String access by label
        assert_eq!(form.str("Name"), Some("John Doe"));
        assert_eq!(form.str("Description"), Some("")); // Empty but exists
        assert_eq!(form.str_non_empty("Name"), Some("John Doe"));
        assert_eq!(form.str_non_empty("Description"), None); // Empty returns None
        assert_eq!(form.optional_str("Name"), Some("John Doe".to_string()));
        assert_eq!(form.optional_str("Description"), None);
        assert_eq!(form.str("NonExistent"), None); // Missing field

        // Currency by label
        assert_eq!(form.currency("Amount"), Some(1234.56));
        assert_eq!(form.currency_or("Amount", 0.0), 1234.56);
        assert_eq!(form.currency_or("NonExistent", 99.0), 99.0);

        // Percentage by label
        assert!((form.percentage("Rate").unwrap() - 0.075).abs() < 0.0001);
        assert!((form.percentage_or("Rate", 0.0) - 0.075).abs() < 0.0001);

        // Boolean by label
        assert_eq!(form.bool("Active"), Some(true));
        assert!(form.bool_or("Active", false));

        // Integer by label
        assert_eq!(form.int::<u32>("Age"), Some(25));
        assert_eq!(form.int_or::<u32>("Age", 0), 25);
        assert_eq!(form.int_or::<u32>("NonExistent", 42), 42);
    }
}
