//! Horizontal sub-tab bar for use within screens (e.g., firewall sub-tabs,
//! client type filters, device detail tabs).

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::theme;

/// Renders a horizontal tab bar line with the active tab highlighted.
///
/// Each label is rendered inline. The active tab gets Electric Purple + underline;
/// inactive tabs get Dim White.
pub fn render_sub_tabs<'a>(labels: &[&'a str], active_index: usize) -> Line<'a> {
    let mut spans = Vec::with_capacity(labels.len() * 2);

    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme::key_hint()));
        }

        if i == active_index {
            spans.push(Span::styled(
                format!("[{label}]"),
                theme::tab_active().add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(*label, theme::tab_inactive()));
        }
    }

    Line::from(spans)
}
