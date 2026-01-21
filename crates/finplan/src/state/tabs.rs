/// Tab identifiers for the TUI application.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabId {
    PortfolioProfiles,
    Scenario,
    Events,
    Results,
    Optimize,
}

impl TabId {
    pub const ALL: [TabId; 5] = [
        TabId::PortfolioProfiles,
        TabId::Events,
        TabId::Scenario,
        TabId::Results,
        TabId::Optimize,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            TabId::PortfolioProfiles => "Portfolio & Profiles",
            TabId::Events => "Events",
            TabId::Scenario => "Scenario",
            TabId::Results => "Results",
            TabId::Optimize => "Optimize",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            TabId::PortfolioProfiles => 0,
            TabId::Events => 1,
            TabId::Scenario => 2,
            TabId::Results => 3,
            TabId::Optimize => 4,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(TabId::PortfolioProfiles),
            1 => Some(TabId::Events),
            2 => Some(TabId::Scenario),
            3 => Some(TabId::Results),
            4 => Some(TabId::Optimize),
            _ => None,
        }
    }
}
