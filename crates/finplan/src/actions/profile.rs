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
        ProfileTypeContext::StudentT => (
            "New Profile (Student's t)",
            vec![
                FormField::text("Name", ""),
                FormField::text("Description", ""),
                FormField::percentage("Mean", 0.0957),
                FormField::percentage("Std Dev", 0.1652),
                FormField::select(
                    "Tail Behavior",
                    vec![
                        "Moderate tails (df=5)".to_string(),
                        "Fat tails (df=3)".to_string(),
                        "Very fat tails (df=2)".to_string(),
                    ],
                    "Moderate tails (df=5)",
                ),
            ],
        ),
        ProfileTypeContext::RegimeSwitchingNormal => (
            "New Profile (Regime Switching)",
            vec![
                FormField::text("Name", "S&P 500 Regime Switching"),
                FormField::text("Description", "Bull/bear market regime switching model"),
                FormField::read_only("Bull Market", "12.0% mean, 12.0% std dev"),
                FormField::read_only("Bear Market", "-5.0% mean, 22.0% std dev"),
                FormField::read_only("Bull->Bear Prob", "15% (avg ~7 year bull cycles)"),
                FormField::read_only("Bear->Bull Prob", "40% (avg ~2.5 year bear cycles)"),
            ],
        ),
        ProfileTypeContext::RegimeSwitchingStudentT => (
            "New Profile (Regime Switching Student-t)",
            vec![
                FormField::text("Name", "S&P 500 Regime Switching (Fat Tails)"),
                FormField::text(
                    "Description",
                    "Bull/bear regime switching with fat-tailed distributions",
                ),
                FormField::read_only("Bull Market", "12.0% mean, 9.3% scale (df=5)"),
                FormField::read_only("Bear Market", "-5.0% mean, 17.0% scale (df=5)"),
                FormField::read_only("Bull->Bear Prob", "15% (avg ~7 year bull cycles)"),
                FormField::read_only("Bear->Bull Prob", "40% (avg ~2.5 year bear cycles)"),
            ],
        ),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::CREATE_PROFILE)
            .with_typed_context(ModalContext::ProfileType(profile_type_ctx))
            .start_editing(),
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
        Some(ProfileTypeContext::StudentT) => {
            let mean = parts
                .get(2)
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.0957);
            let std_dev = parts
                .get(3)
                .and_then(|s| parse_percentage(s).ok())
                .unwrap_or(0.1652);
            // Parse tail behavior to get df
            let df: f64 = parts
                .get(4)
                .map(|s| {
                    if s.contains("df=2") {
                        2.0_f64
                    } else if s.contains("df=3") {
                        3.0_f64
                    } else {
                        5.0_f64 // Default moderate tails
                    }
                })
                .unwrap_or(5.0_f64);
            // Convert std_dev to scale: scale = std_dev * sqrt((df-2)/df)
            let scale: f64 = if df > 2.0 {
                std_dev * ((df - 2.0_f64) / df).sqrt()
            } else {
                std_dev // For df <= 2, variance is undefined, use std_dev as scale
            };
            ReturnProfileData::StudentT { mean, scale, df }
        }
        Some(ProfileTypeContext::RegimeSwitchingNormal) => {
            // S&P 500 regime switching preset with Normal distributions
            // Conservative parameters: ~7.4% expected return
            // 73% bull (12% return), 27% bear (-5% return)
            ReturnProfileData::RegimeSwitching {
                bull_mean: 0.12,
                bull_std_dev: 0.12,
                bear_mean: -0.05,
                bear_std_dev: 0.22,
                bull_to_bear_prob: 0.15,
                bear_to_bull_prob: 0.40,
            }
        }
        Some(ProfileTypeContext::RegimeSwitchingStudentT) => {
            // S&P 500 regime switching preset with fat tails
            // Scale adjusted for df=5: scale = std_dev * sqrt((5-2)/5) = std_dev * sqrt(0.6)
            // Conservative parameters: ~7.4% expected return
            let scale_factor = (3.0_f64 / 5.0).sqrt();
            ReturnProfileData::RegimeSwitching {
                bull_mean: 0.12,
                bull_std_dev: 0.12 * scale_factor, // ~0.093
                bear_mean: -0.05,
                bear_std_dev: 0.22 * scale_factor, // ~0.170
                bull_to_bear_prob: 0.15,
                bear_to_bull_prob: 0.40,
            }
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
            ReturnProfileData::StudentT {
                mean,
                scale,
                df: current_df,
            } => {
                if let Some(m) = parts.get(3).and_then(|s| parse_percentage(s).ok()) {
                    *mean = m;
                }
                // Parse std_dev from form and convert to scale
                let std_dev = parts
                    .get(4)
                    .and_then(|s| parse_percentage(s).ok())
                    .unwrap_or(*scale);
                // Parse tail behavior to get new df
                let new_df = parts
                    .get(5)
                    .map(|s| {
                        if s.contains("df=2") {
                            2.0
                        } else if s.contains("df=3") {
                            3.0
                        } else {
                            5.0
                        }
                    })
                    .unwrap_or(*current_df);
                *current_df = new_df;
                // Convert std_dev to scale
                *scale = if new_df > 2.0 {
                    std_dev * ((new_df - 2.0) / new_df).sqrt()
                } else {
                    std_dev
                };
            }
            ReturnProfileData::RegimeSwitching { .. } => {
                // Regime switching is preset-only, no editable parameters
            }
            ReturnProfileData::Bootstrap { .. } => {
                // Bootstrap profiles use historical presets, no editable parameters
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
