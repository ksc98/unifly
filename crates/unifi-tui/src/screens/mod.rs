//! Screen implementations. Each screen is a top-level Component.
//!
//! For now, all eight screens use a `PlaceholderScreen` that renders the
//! screen name centered. Real implementations come in the next task.

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::component::Component;
use crate::screen::ScreenId;
use crate::theme;

/// A placeholder screen that renders the screen name and a brief message.
pub struct PlaceholderScreen {
    id: ScreenId,
    focused: bool,
}

impl PlaceholderScreen {
    pub fn new(id: ScreenId) -> Self {
        Self { id, focused: false }
    }
}

impl Component for PlaceholderScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        // Placeholder screens don't handle keys — global handler takes care of it
        Ok(None)
    }

    fn update(&mut self, _action: &Action) -> Result<Option<Action>> {
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(format!(" {} ", self.id))
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

        let text = format!(
            "{}\n\nScreen {} ─ Coming Soon",
            self.id,
            self.id.number()
        );
        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(theme::table_row());

        // Center vertically within the inner area
        let y_offset = inner.height.saturating_sub(3) / 2;
        let centered = Rect {
            x: inner.x,
            y: inner.y + y_offset,
            width: inner.width,
            height: 3.min(inner.height),
        };
        frame.render_widget(paragraph, centered);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &str {
        self.id.label()
    }
}

/// Create all eight placeholder screens.
pub fn create_screens() -> Vec<(ScreenId, PlaceholderScreen)> {
    ScreenId::ALL
        .iter()
        .map(|&id| (id, PlaceholderScreen::new(id)))
        .collect()
}
