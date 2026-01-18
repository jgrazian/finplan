/// Tab identifiers for the TUI application.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabId {
    PortfolioProfiles,
    Scenario,
    Events,
    Results,
}

impl TabId {
    pub const ALL: [TabId; 4] = [
        TabId::PortfolioProfiles,
        TabId::Scenario,
        TabId::Events,
        TabId::Results,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            TabId::PortfolioProfiles => "Portfolio & Profiles",
            TabId::Scenario => "Scenario",
            TabId::Events => "Events",
            TabId::Results => "Results",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            TabId::PortfolioProfiles => 0,
            TabId::Scenario => 1,
            TabId::Events => 2,
            TabId::Results => 3,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(TabId::PortfolioProfiles),
            1 => Some(TabId::Scenario),
            2 => Some(TabId::Events),
            3 => Some(TabId::Results),
            _ => None,
        }
    }
}
