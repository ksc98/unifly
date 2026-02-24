//! SilkCircuit Neon palette and semantic styling for the TUI.

use ratatui::style::{Color, Modifier, Style};

// ── Core Palette ──────────────────────────────────────────────────────

pub const ELECTRIC_PURPLE: Color = Color::Rgb(225, 53, 255); // #e135ff
pub const NEON_CYAN: Color = Color::Rgb(128, 255, 234); // #80ffea
pub const CORAL: Color = Color::Rgb(255, 106, 193); // #ff6ac1
pub const ELECTRIC_YELLOW: Color = Color::Rgb(241, 250, 140); // #f1fa8c
pub const SUCCESS_GREEN: Color = Color::Rgb(80, 250, 123); // #50fa7b
pub const ERROR_RED: Color = Color::Rgb(255, 99, 99); // #ff6363

// ── Extended Palette ──────────────────────────────────────────────────

pub const DIM_WHITE: Color = Color::Rgb(189, 193, 207); // #bdc1cf
pub const BORDER_GRAY: Color = Color::Rgb(98, 114, 164); // #6272a4
pub const BG_HIGHLIGHT: Color = Color::Rgb(40, 42, 54); // #282a36
pub const BG_DARK: Color = Color::Rgb(30, 31, 41); // #1e1f29
pub const LIGHT_BLUE: Color = Color::Rgb(139, 233, 253); // #8be9fd

// ── Chart Fill Colors (dimmed versions for area fills) ───────────────

pub const TX_FILL: Color = Color::Rgb(45, 20, 55); // dark purple — upload area fill
pub const RX_FILL: Color = Color::Rgb(20, 40, 65); // dark blue — download area fill

/// Chart series colors for multi-line graphs.
pub const CHART_SERIES: &[Color] = &[
    NEON_CYAN,
    CORAL,
    ELECTRIC_PURPLE,
    SUCCESS_GREEN,
    ELECTRIC_YELLOW,
    LIGHT_BLUE,
];

// ── Semantic Styles ───────────────────────────────────────────────────

/// Title text for blocks/panels.
pub fn title_style() -> Style {
    Style::default().fg(NEON_CYAN).add_modifier(Modifier::BOLD)
}

/// Border for a focused panel.
pub fn border_focused() -> Style {
    Style::default().fg(ELECTRIC_PURPLE)
}

/// Border for an unfocused panel.
pub fn border_default() -> Style {
    Style::default().fg(BORDER_GRAY)
}

/// Table header row.
pub fn table_header() -> Style {
    Style::default()
        .fg(NEON_CYAN)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

/// Normal table row text.
pub fn table_row() -> Style {
    Style::default().fg(DIM_WHITE)
}

/// Selected / highlighted table row.
pub fn table_selected() -> Style {
    Style::default()
        .fg(ELECTRIC_PURPLE)
        .bg(BG_HIGHLIGHT)
        .add_modifier(Modifier::BOLD)
}

/// Active tab in the tab bar.
pub fn tab_active() -> Style {
    Style::default()
        .fg(ELECTRIC_PURPLE)
        .add_modifier(Modifier::BOLD)
}

/// Inactive tab in the tab bar.
pub fn tab_inactive() -> Style {
    Style::default().fg(DIM_WHITE)
}

/// Status bar text.
#[allow(dead_code)]
pub fn status_bar() -> Style {
    Style::default().fg(DIM_WHITE)
}

/// Key hint text (e.g., "q quit  ? help").
pub fn key_hint() -> Style {
    Style::default().fg(BORDER_GRAY)
}

/// Key hint key character.
pub fn key_hint_key() -> Style {
    Style::default().fg(NEON_CYAN).add_modifier(Modifier::BOLD)
}
