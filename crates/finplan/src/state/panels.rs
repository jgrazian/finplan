use crate::components::PanelNavigable;

/// Panel focus enums for different screens.
/// Generic left/right focus for two-panel layouts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Left,
    Right,
}

impl FocusedPanel {
    pub fn next(self) -> Self {
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for FocusedPanel {
    fn next(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
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
    pub fn is_secondary(self) -> bool {
        matches!(self, Self::AssetMappings | Self::Config)
    }

    /// Cycle through all panels: Accounts -> Profiles -> AssetMappings -> Config -> Accounts
    pub fn next(self) -> Self {
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for PortfolioProfilesPanel {
    /// Cycle through all panels: Accounts -> Profiles -> AssetMappings -> Config -> Accounts
    fn next(self) -> Self {
        match self {
            Self::Accounts => Self::Profiles,
            Self::Profiles => Self::AssetMappings,
            Self::AssetMappings => Self::Config,
            Self::Config => Self::Accounts,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Accounts => Self::Config,
            Self::Profiles => Self::Accounts,
            Self::AssetMappings => Self::Profiles,
            Self::Config => Self::AssetMappings,
        }
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
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for EventsPanel {
    fn next(self) -> Self {
        match self {
            Self::EventList => Self::Details,
            Self::Details => Self::Timeline,
            Self::Timeline => Self::EventList,
        }
    }

    fn prev(self) -> Self {
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
    ScenarioDetails,  // Left panel: selected scenario details
    SimulationErrors, // Left panel: simulation warnings/errors
    ComparisonTable,  // Right panel: comparison table
    OverlayChart,     // Right panel: net worth overlay chart
}

impl ScenarioPanel {
    pub fn is_left_panel(self) -> bool {
        matches!(
            self,
            Self::ScenarioList | Self::ScenarioDetails | Self::SimulationErrors
        )
    }

    pub fn next(self) -> Self {
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for ScenarioPanel {
    fn next(self) -> Self {
        match self {
            Self::ScenarioList => Self::ScenarioDetails,
            Self::ScenarioDetails => Self::SimulationErrors,
            Self::SimulationErrors => Self::ComparisonTable,
            Self::ComparisonTable => Self::OverlayChart,
            Self::OverlayChart => Self::ScenarioList,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::ScenarioList => Self::OverlayChart,
            Self::ScenarioDetails => Self::ScenarioList,
            Self::SimulationErrors => Self::ScenarioDetails,
            Self::ComparisonTable => Self::SimulationErrors,
            Self::OverlayChart => Self::ComparisonTable,
        }
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
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for ResultsPanel {
    fn next(self) -> Self {
        match self {
            Self::NetWorthChart => Self::AccountChart,
            Self::AccountChart => Self::YearlyBreakdown,
            Self::YearlyBreakdown => Self::Ledger,
            Self::Ledger => Self::NetWorthChart,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::NetWorthChart => Self::Ledger,
            Self::AccountChart => Self::NetWorthChart,
            Self::YearlyBreakdown => Self::AccountChart,
            Self::Ledger => Self::YearlyBreakdown,
        }
    }
}

// ========== Analysis Screen Panels ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalysisPanel {
    #[default]
    Parameters,
    Metrics,
    Config,
    Results,
}

impl AnalysisPanel {
    pub fn next(self) -> Self {
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for AnalysisPanel {
    fn next(self) -> Self {
        match self {
            Self::Parameters => Self::Metrics,
            Self::Metrics => Self::Config,
            Self::Config => Self::Results,
            Self::Results => Self::Parameters,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Parameters => Self::Results,
            Self::Metrics => Self::Parameters,
            Self::Config => Self::Metrics,
            Self::Results => Self::Config,
        }
    }
}

// ========== Legacy Optimize Panel (kept for backwards compatibility) ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizePanel {
    #[default]
    Parameters,
    Objective,
    Progress,
    Results,
}

impl OptimizePanel {
    pub fn next(self) -> Self {
        <Self as PanelNavigable>::next(self)
    }

    pub fn prev(self) -> Self {
        <Self as PanelNavigable>::prev(self)
    }
}

impl PanelNavigable for OptimizePanel {
    fn next(self) -> Self {
        match self {
            Self::Parameters => Self::Objective,
            Self::Objective => Self::Progress,
            Self::Progress => Self::Results,
            Self::Results => Self::Parameters,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Parameters => Self::Results,
            Self::Objective => Self::Parameters,
            Self::Progress => Self::Objective,
            Self::Results => Self::Progress,
        }
    }
}
