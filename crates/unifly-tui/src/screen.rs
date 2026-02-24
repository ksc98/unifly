//! Screen trait and screen identifier enum.

use std::fmt;

/// Identifies each primary TUI screen, navigable by number keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScreenId {
    #[default]
    Dashboard, // 1
    #[allow(dead_code)]
    Devices, // (removed from tab bar)
    Clients, // 2
    #[allow(dead_code)]
    Networks, // (removed from tab bar)
    #[allow(dead_code)]
    Firewall, // (removed from tab bar)
    Topology, // 3
    Events,   // 4
    Stats,    // 5
    /// Onboarding wizard — not in the tab bar, not navigable by number keys.
    Setup,
    /// Settings editor — not in the tab bar, opened with `,`.
    Settings,
}

impl ScreenId {
    /// All screens in tab-bar order.
    pub const ALL: [ScreenId; 5] = [
        Self::Dashboard,
        Self::Clients,
        Self::Topology,
        Self::Events,
        Self::Stats,
    ];

    /// Numeric key (1-5) for this screen. Setup has no number key.
    pub fn number(self) -> u8 {
        match self {
            Self::Dashboard => 1,
            Self::Clients => 2,
            Self::Topology => 3,
            Self::Events => 4,
            Self::Stats => 5,
            Self::Devices | Self::Networks | Self::Firewall
            | Self::Setup | Self::Settings => 0,
        }
    }

    /// Screen from a numeric key (1-5). Returns None for out-of-range.
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Dashboard),
            2 => Some(Self::Clients),
            3 => Some(Self::Topology),
            4 => Some(Self::Events),
            5 => Some(Self::Stats),
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
            Self::Settings => "Settings",
        }
    }

    /// Compact label for narrow terminals (< 100 cols).
    pub fn label_short(self) -> &'static str {
        match self {
            Self::Dashboard => "Dash",
            Self::Devices => "Dev",
            Self::Clients => "Cli",
            Self::Networks => "Net",
            Self::Firewall => "FW",
            Self::Topology => "Topo",
            Self::Events => "Evt",
            Self::Stats => "Stat",
            Self::Setup => "Setup",
            Self::Settings => "Set",
        }
    }
}

impl fmt::Display for ScreenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
