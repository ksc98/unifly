//! Screen trait and screen identifier enum.

use std::fmt;

/// Identifies each primary TUI screen, navigable by number keys 1-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScreenId {
    #[default]
    Dashboard, // 1
    Devices,  // 2
    Clients,  // 3
    Networks, // 4
    Firewall, // 5
    Topology, // 6
    Events,   // 7
    Stats,    // 8
    /// Onboarding wizard — not in the tab bar, not navigable by number keys.
    Setup,
}

impl ScreenId {
    /// All screens in tab-bar order.
    pub const ALL: [ScreenId; 8] = [
        Self::Dashboard,
        Self::Devices,
        Self::Clients,
        Self::Networks,
        Self::Firewall,
        Self::Topology,
        Self::Events,
        Self::Stats,
    ];

    /// Numeric key (1-8) for this screen. Setup has no number key.
    pub fn number(self) -> u8 {
        match self {
            Self::Dashboard => 1,
            Self::Devices => 2,
            Self::Clients => 3,
            Self::Networks => 4,
            Self::Firewall => 5,
            Self::Topology => 6,
            Self::Events => 7,
            Self::Stats => 8,
            Self::Setup => 0,
        }
    }

    /// Screen from a numeric key (1-8). Returns None for out-of-range.
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Dashboard),
            2 => Some(Self::Devices),
            3 => Some(Self::Clients),
            4 => Some(Self::Networks),
            5 => Some(Self::Firewall),
            6 => Some(Self::Topology),
            7 => Some(Self::Events),
            8 => Some(Self::Stats),
            _ => None,
        }
    }

    /// Next screen in tab order (wraps around).
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Previous screen in tab order (wraps around).
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Short label for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Devices => "Devices",
            Self::Clients => "Clients",
            Self::Networks => "Networks",
            Self::Firewall => "Firewall",
            Self::Topology => "Topo",
            Self::Events => "Events",
            Self::Stats => "Stats",
            Self::Setup => "Setup",
        }
    }
}

impl fmt::Display for ScreenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
