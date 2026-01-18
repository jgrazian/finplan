/// Per-screen state structs.

use super::panels::{EventsPanel, PortfolioProfilesPanel};

#[derive(Debug)]
pub struct PortfolioProfilesState {
    pub selected_account_index: usize,
    pub selected_profile_index: usize,
    pub selected_mapping_index: usize,
    pub selected_config_index: usize,
    pub focused_panel: PortfolioProfilesPanel,
}

impl Default for PortfolioProfilesState {
    fn default() -> Self {
        Self {
            selected_account_index: 0,
            selected_profile_index: 0,
            selected_mapping_index: 0,
            selected_config_index: 0,
            focused_panel: PortfolioProfilesPanel::Accounts,
        }
    }
}

#[derive(Debug)]
pub struct EventsState {
    pub selected_event_index: usize,
    pub focused_panel: EventsPanel,
}

impl Default for EventsState {
    fn default() -> Self {
        Self {
            selected_event_index: 0,
            focused_panel: EventsPanel::EventList,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScenarioState {
    pub focused_field: usize,
}

#[derive(Debug, Default)]
pub struct ResultsState {
    pub scroll_offset: usize,
}
