mod app_state;
mod cache;
mod errors;
mod panels;
mod screen_state;
mod tabs;

// Re-export all types from submodules
pub use app_state::*;
pub use cache::*;
pub use errors::*;
pub use panels::*;
pub use screen_state::*;
pub use tabs::*;

// Re-export modal types from the modals module for backwards compatibility
pub use crate::modals::{
    ConfirmModal, FieldType, FormField, FormKind, FormModal, MessageModal, ModalAction,
    ModalContext, ModalState, PickerModal, ScenarioPickerModal, TextInputModal,
    asset_purchase_fields, asset_sale_fields,
};

// Re-export sub-enums from modal_action for convenience
pub use crate::modals::{
    AccountAction, ConfigAction, EffectAction, EventAction, HoldingAction, OptimizeAction,
    ProfileAction, ScenarioAction,
};
