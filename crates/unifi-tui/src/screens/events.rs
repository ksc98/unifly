//! Events screen — live event stream with pause/filter (spec §2.7).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::Event;
use unifi_core::model::EventSeverity;

use crate::action::Action;
use crate::component::Component;
use crate::theme;

pub struct EventsScreen {
    focused: bool,
    events: Vec<Arc<Event>>,
    paused: bool,
    scroll_offset: usize,
    /// Max events to keep in memory.
    capacity: usize,
}

impl EventsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            events: Vec::new(),
            paused: false,
            scroll_offset: 0,
            capacity: 10_000,
        }
    }

    #[allow(dead_code)]
    fn visible_count(&self, area_height: u16) -> usize {
        area_height.saturating_sub(1) as usize
    }
}

impl Component for EventsScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                if !self.paused {
                    // Resume: snap to bottom
                    self.scroll_offset = 0;
                }
                Ok(Some(Action::ToggleEventPause))
            }
            KeyCode::Char('j') | KeyCode::Down if self.paused => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
                Ok(None)
            }
            KeyCode::Char('k') | KeyCode::Up if self.paused => {
                self.scroll_offset =
                    (self.scroll_offset + 1).min(self.events.len().saturating_sub(1));
                Ok(None)
            }
            KeyCode::Char('g') if self.paused => {
                self.scroll_offset = self.events.len().saturating_sub(1);
                Ok(Some(Action::ScrollToTop))
            }
            KeyCode::Char('G') if self.paused => {
                self.scroll_offset = 0;
                Ok(Some(Action::ScrollToBottom))
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) && self.paused => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                Ok(Some(Action::PageDown))
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) && self.paused => {
                self.scroll_offset =
                    (self.scroll_offset + 10).min(self.events.len().saturating_sub(1));
                Ok(Some(Action::PageUp))
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::EventReceived(event) => {
                self.events.push(Arc::clone(event));
                if self.events.len() > self.capacity {
                    self.events.remove(0);
                }
            }
            Action::ToggleEventPause => {
                // Handled in key handler, but also handle external toggles
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let count = self.events.len();
        let live_indicator = if self.paused {
            Span::styled("PAUSED", Style::default().fg(theme::ELECTRIC_YELLOW))
        } else {
            Span::styled("● LIVE", Style::default().fg(theme::SUCCESS_GREEN))
        };

        let title = format!(" Events ({count}) ");
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Length(1), // status line
            Constraint::Min(1),    // events
            Constraint::Length(1), // hints
        ])
        .split(inner);

        // Status line
        let status = Line::from(vec![
            Span::styled("  Filter: ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled("[all]", Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  Type: ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled("[all]", Style::default().fg(theme::NEON_CYAN)),
            Span::raw("  "),
            live_indicator,
        ]);
        frame.render_widget(Paragraph::new(status), layout[0]);

        // Events list
        let visible_height = layout[1].height as usize;
        let total = self.events.len();

        // Calculate which events to show
        let end = total.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(visible_height);

        let mut lines: Vec<Line> = Vec::new();

        // Table header
        lines.push(Line::from(vec![
            Span::styled("  Timestamp       ", theme::table_header()),
            Span::styled("Type            ", theme::table_header()),
            Span::styled("Category   ", theme::table_header()),
            Span::styled("Message", theme::table_header()),
        ]));

        for event in self.events.get(start..end).unwrap_or_default() {
            let time_str = event.timestamp.format("%H:%M:%S%.3f").to_string();
            let severity_color = match event.severity {
                EventSeverity::Error | EventSeverity::Critical => theme::ERROR_RED,
                EventSeverity::Warning => theme::ELECTRIC_YELLOW,
                EventSeverity::Info => theme::NEON_CYAN,
                _ => theme::DIM_WHITE,
            };
            let category = format!("{:?}", event.category);
            let msg_width = layout[1].width.saturating_sub(50).max(10) as usize;
            let msg: String = event.message.chars().take(msg_width).collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", time_str),
                    Style::default().fg(theme::ELECTRIC_YELLOW),
                ),
                Span::styled(
                    format!("{:<16}", &event.event_type),
                    Style::default().fg(severity_color),
                ),
                Span::styled(
                    format!("{:<11}", category),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(msg, Style::default().fg(severity_color)),
            ]));
        }

        if self.events.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Waiting for events...",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        }

        // Auto-scroll indicator
        if !self.paused && !self.events.is_empty() {
            lines.push(Line::from(Span::styled(
                "  ↓ auto-scrolling",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        }

        frame.render_widget(Paragraph::new(lines), layout[1]);

        // Hints
        let hints = Line::from(vec![
            Span::styled("  Space ", theme::key_hint_key()),
            Span::styled("pause/resume  ", theme::key_hint()),
            Span::styled("j/k ", theme::key_hint_key()),
            Span::styled("scroll (paused)  ", theme::key_hint()),
            Span::styled("/ ", theme::key_hint_key()),
            Span::styled("search", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[2]);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &str {
        "Events"
    }
}
