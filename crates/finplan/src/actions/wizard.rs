// Wizard flow helpers for multi-step modal sequences
//
// This module provides lightweight abstractions for common wizard patterns
// in the TUI. Current wizard flows include:
//
// Account Creation:
//   PickAccountCategory → PickAccountType → CreateAccount
//
// Event Creation:
//   PickTriggerType → [PickInterval|PickAccount|PickEvent] → CreateEvent
//
// Effect Addition:
//   ManageEffects → PickEffectTypeForAdd → AddEffect

use crate::state::{FormField, FormModal, ModalAction, ModalState, PickerModal};

/// A wizard step builder for creating form modals
pub struct FormWizard {
    title: String,
    fields: Vec<FormField>,
    action: ModalAction,
    context: Option<String>,
    start_editing: bool,
}

impl FormWizard {
    /// Create a new form wizard step
    pub fn new(title: impl Into<String>, action: ModalAction) -> Self {
        Self {
            title: title.into(),
            fields: vec![],
            action,
            context: None,
            start_editing: false,
        }
    }

    /// Add a text field
    pub fn text(mut self, label: &str, default: &str) -> Self {
        self.fields.push(FormField::text(label, default));
        self
    }

    /// Add a currency field
    pub fn currency(mut self, label: &str, default: f64) -> Self {
        self.fields.push(FormField::currency(label, default));
        self
    }

    /// Add a percentage field
    pub fn percentage(mut self, label: &str, default: f64) -> Self {
        self.fields.push(FormField::percentage(label, default));
        self
    }

    /// Add a read-only field
    pub fn read_only(mut self, label: &str, value: &str) -> Self {
        self.fields.push(FormField::read_only(label, value));
        self
    }

    /// Set the context for this form
    pub fn context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    /// Start in editing mode
    pub fn editing(mut self) -> Self {
        self.start_editing = true;
        self
    }

    /// Build the form modal
    pub fn build(self) -> ModalState {
        let mut form = FormModal::new(&self.title, self.fields, self.action);
        if let Some(ctx) = self.context {
            form = form.with_context(&ctx);
        }
        if self.start_editing {
            form = form.start_editing();
        }
        ModalState::Form(form)
    }
}

/// A wizard step builder for creating picker modals
pub struct PickerWizard {
    title: String,
    options: Vec<String>,
    action: ModalAction,
}

impl PickerWizard {
    /// Create a new picker wizard step
    pub fn new(title: impl Into<String>, action: ModalAction) -> Self {
        Self {
            title: title.into(),
            options: vec![],
            action,
        }
    }

    /// Add options from an iterator
    pub fn options(mut self, opts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options.extend(opts.into_iter().map(Into::into));
        self
    }

    /// Add a single option
    pub fn option(mut self, opt: impl Into<String>) -> Self {
        self.options.push(opt.into());
        self
    }

    /// Build the picker modal
    pub fn build(self) -> ModalState {
        ModalState::Picker(PickerModal::new(&self.title, self.options, self.action))
    }
}

/// Shortcuts for common wizard patterns
pub mod shortcuts {
    use super::*;

    /// Create a simple text input form
    pub fn text_form(
        title: &str,
        label: &str,
        default: &str,
        action: ModalAction,
    ) -> ModalState {
        FormWizard::new(title, action)
            .text(label, default)
            .editing()
            .build()
    }

    /// Create a name + description form
    pub fn name_desc_form(
        title: &str,
        action: ModalAction,
        context: Option<&str>,
    ) -> ModalState {
        let mut wizard = FormWizard::new(title, action)
            .text("Name", "")
            .text("Description", "");
        if let Some(ctx) = context {
            wizard = wizard.context(ctx);
        }
        wizard.build()
    }

    /// Create a simple picker
    pub fn simple_picker<I, S>(title: &str, options: I, action: ModalAction) -> ModalState
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        PickerWizard::new(title, action).options(options).build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_wizard_builder() {
        let modal = FormWizard::new("Test Form", ModalAction::CREATE_ACCOUNT)
            .text("Name", "Default")
            .currency("Amount", 100.0)
            .context("test_context")
            .editing()
            .build();

        match modal {
            ModalState::Form(form) => {
                assert_eq!(form.title, "Test Form");
                assert_eq!(form.fields.len(), 2);
                assert_eq!(form.context_str(), Some("test_context".to_string()));
                assert!(form.editing);
            }
            _ => panic!("Expected Form modal"),
        }
    }

    #[test]
    fn test_picker_wizard_builder() {
        let modal = PickerWizard::new("Test Picker", ModalAction::PICK_ACCOUNT_TYPE)
            .options(["Option 1", "Option 2"])
            .option("Option 3")
            .build();

        match modal {
            ModalState::Picker(picker) => {
                assert_eq!(picker.title, "Test Picker");
                assert_eq!(picker.options.len(), 3);
            }
            _ => panic!("Expected Picker modal"),
        }
    }
}
