/// Modal types for forms, pickers, and confirmations.

use super::ModalAction;

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
            new_name: if action == ModalAction::SaveAs {
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
        let max_index = if self.action == ModalAction::SaveAs {
            self.scenarios.len()
        } else {
            self.scenarios.len().saturating_sub(1)
        };
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    pub fn selected_name(&self) -> Option<String> {
        if self.action == ModalAction::SaveAs && self.selected_index == self.scenarios.len() {
            // "New scenario" selected
            self.new_name.clone()
        } else {
            self.scenarios.get(self.selected_index).cloned()
        }
    }

    pub fn is_new_scenario_selected(&self) -> bool {
        self.action == ModalAction::SaveAs && self.selected_index == self.scenarios.len()
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
}

impl PickerModal {
    pub fn new(title: &str, options: Vec<String>, action: ModalAction) -> Self {
        Self {
            title: title.to_string(),
            options,
            selected_index: 0,
            action,
        }
    }
}

// ========== FormModal ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Currency,
    Percentage,
    ReadOnly,
}

#[derive(Debug, Clone)]
pub struct FormField {
    pub label: String,
    pub field_type: FieldType,
    pub value: String,
    pub cursor_pos: usize,
}

impl FormField {
    pub fn new(label: &str, field_type: FieldType, value: &str) -> Self {
        Self {
            label: label.to_string(),
            field_type,
            value: value.to_string(),
            cursor_pos: 0,
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
        Self::new(label, FieldType::Percentage, &format!("{:.2}", rate * 100.0))
    }

    pub fn read_only(label: &str, value: &str) -> Self {
        Self::new(label, FieldType::ReadOnly, value)
    }
}

#[derive(Debug)]
pub struct FormModal {
    pub title: String,
    pub fields: Vec<FormField>,
    pub focused_field: usize,
    pub editing: bool,
    pub action: ModalAction,
    /// Context data for the form (e.g., account index being edited)
    pub context: Option<String>,
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
        }
    }

    pub fn with_context(mut self, context: &str) -> Self {
        self.context = Some(context.to_string());
        self
    }

    /// Start in editing mode (for better UX)
    pub fn start_editing(mut self) -> Self {
        if !self.fields.is_empty()
            && self.fields[self.focused_field].field_type != FieldType::ReadOnly
        {
            self.editing = true;
            self.fields[self.focused_field].cursor_pos =
                self.fields[self.focused_field].value.len();
        }
        self
    }
}

// ========== ConfirmModal ==========

#[derive(Debug)]
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    pub action: ModalAction,
    /// Context data for the confirmation (e.g., index of item to delete)
    pub context: Option<String>,
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

    pub fn with_context(mut self, context: &str) -> Self {
        self.context = Some(context.to_string());
        self
    }
}
