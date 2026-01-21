// Profile actions - type picking, CRUD operations

use crate::data::profiles_data::{ProfileData, ReturnProfileData, ReturnProfileTag};
use crate::modals::parse_percentage;
use crate::state::context::{ModalContext, ProfileTypeContext};
use crate::state::{AppState, FormField, FormModal, ModalAction, ModalState};

use super::{ActionContext, ActionResult};

/// Handle profile type selection - shows creation form
pub fn handle_profile_type_pick(profile_type: &str) -> ActionResult {
    let profile_type_ctx = match profile_type.parse::<ProfileTypeContext>() {
        Ok(ctx) => ctx,
        Err(_) => return ActionResult::close(),
    };

    let (title, fields) = match &profile_type_ctx {
        ProfileTypeContext::None => (
            "New Profile (None)",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
            ],
        ),
        ProfileTypeContext::Fixed => (
            "New Profile (Fixed)",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::percentage("Rate", 0.07),
            ],
        ),
        ProfileTypeContext::Normal => (
            "New Profile (Normal)",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::percentage("Mean", 0.07),
                FormField::percentage("Std Dev", 0.15),
            ],
        ),
        ProfileTypeContext::LogNormal => (
            "New Profile (Log-Normal)",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::percentage("Mean", 0.07),
                FormField::percentage("Std Dev", 0.15),
            ],
        ),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::CREATE_PROFILE)
            .with_typed_context(ModalContext::ProfileType(profile_type_ctx)),
    ))
}

/// Handle profile creation
pub fn handle_create_profile(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let parts = ctx.value_parts();

    // Get typed profile type context
    let profile_type_ctx = ctx
        .typed_context()
        .and_then(|c| c.as_profile_type())
        .cloned();

    let name = parts.first().unwrap_or(&"").to_string();
    if name.is_empty() {
        return ActionResult::error("Profile name cannot be empty");
    }

    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let profile = match profile_type_ctx {
        Some(ProfileTypeContext::None) | None => ReturnProfileData::None,
        Some(ProfileTypeContext::Fixed) => {
            let rate = parts
                .get(2)
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.07);
            ReturnProfileData::Fixed { rate }
        }
        Some(ProfileTypeContext::Normal) => {
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
        Some(ProfileTypeContext::LogNormal) => {
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
    };

    let profile_data = ProfileData {
        name: ReturnProfileTag(name),
        description: desc,
        profile,
    };

    state.data_mut().profiles.push(profile_data);
    ActionResult::modified()
}

/// Handle profile editing
pub fn handle_edit_profile(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let idx = match ctx.index() {
        Some(i) => i,
        None => return ActionResult::close(),
    };

    let parts = ctx.value_parts();

    if let Some(profile_data) = state.data_mut().profiles.get_mut(idx) {
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
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle profile deletion
pub fn handle_delete_profile(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    if let Some(idx) = ctx.index() {
        let profiles_len = state.data().profiles.len();
        if idx < profiles_len {
            state.data_mut().profiles.remove(idx);
            let new_len = state.data().profiles.len();
            // Adjust selected index
            if state.portfolio_profiles_state.selected_profile_index >= new_len && new_len > 0 {
                state.portfolio_profiles_state.selected_profile_index = new_len - 1;
            }
            return ActionResult::modified();
        }
    }
    ActionResult::close()
}
