/// Panel focus enums for different screens.
/// Generic left/right focus for two-panel layouts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Left,
    Right,
}

/// Focused panel for the consolidated Portfolio & Profiles tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortfolioProfilesPanel {
    Accounts,      // Unified left panel (list + details)
    Profiles,      // Unified right panel (list + distribution)
    AssetMappings, // Bottom panel (expanded by default)
    Config,        // Bottom panel (expanded by default)
}

impl PortfolioProfilesPanel {
    /// Cycle through all panels: Accounts -> Profiles -> AssetMappings -> Config -> Accounts
    pub fn next(self) -> Self {
        match self {
            Self::Accounts => Self::Profiles,
            Self::Profiles => Self::AssetMappings,
            Self::AssetMappings => Self::Config,
            Self::Config => Self::Accounts,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Accounts => Self::Config,
            Self::Profiles => Self::Accounts,
            Self::AssetMappings => Self::Profiles,
            Self::Config => Self::AssetMappings,
        }
    }

    pub fn is_secondary(self) -> bool {
        matches!(self, Self::AssetMappings | Self::Config)
    }
}

/// Focused panel for the Events tab (3-panel layout)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventsPanel {
    EventList, // Left: list of events
    Details,   // Middle: event details
    Timeline,  // Right: timeline visualization
}

impl EventsPanel {
    pub fn next(self) -> Self {
        match self {
            Self::EventList => Self::Details,
            Self::Details => Self::Timeline,
            Self::Timeline => Self::EventList,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::EventList => Self::Timeline,
            Self::Details => Self::EventList,
            Self::Timeline => Self::Details,
        }
    }
}

/// Focused panel for the Scenario Comparison tab
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScenarioPanel {
    #[default]
    ScenarioList, // Left panel: list of scenarios
    ScenarioDetails, // Left panel: selected scenario details
    ComparisonTable, // Right panel: comparison table
    OverlayChart,    // Right panel: net worth overlay chart
}

impl ScenarioPanel {
    pub fn next(self) -> Self {
        match self {
            Self::ScenarioList => Self::ScenarioDetails,
            Self::ScenarioDetails => Self::ComparisonTable,
            Self::ComparisonTable => Self::OverlayChart,
            Self::OverlayChart => Self::ScenarioList,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::ScenarioList => Self::OverlayChart,
            Self::ScenarioDetails => Self::ScenarioList,
            Self::ComparisonTable => Self::ScenarioDetails,
            Self::OverlayChart => Self::ComparisonTable,
        }
    }

    pub fn is_left_panel(self) -> bool {
        matches!(self, Self::ScenarioList | Self::ScenarioDetails)
    }
}

/// Focused panel for the Results tab (2x2 grid layout)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsPanel {
    #[default]
    NetWorthChart,
    YearlyBreakdown,
    AccountChart,
    Ledger,
}

impl ResultsPanel {
    pub fn next(self) -> Self {
        match self {
            Self::NetWorthChart => Self::AccountChart,
            Self::AccountChart => Self::YearlyBreakdown,
            Self::YearlyBreakdown => Self::Ledger,
            Self::Ledger => Self::NetWorthChart,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::NetWorthChart => Self::Ledger,
            Self::AccountChart => Self::NetWorthChart,
            Self::YearlyBreakdown => Self::AccountChart,
            Self::Ledger => Self::YearlyBreakdown,
        }
    }
}
