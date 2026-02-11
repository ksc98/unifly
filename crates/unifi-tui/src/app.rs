//! Application core — event loop, screen management, action dispatch.

use std::collections::HashMap;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Tabs},
};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::action::Action;
use crate::component::Component;
use crate::event::{Event, EventReader};
use crate::screen::ScreenId;
use crate::screens::create_screens;
use crate::theme;
use crate::tui::Tui;

/// Connection status as seen by the TUI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Top-level application state and event loop.
pub struct App {
    /// Current active screen.
    active_screen: ScreenId,
    /// Previous screen for GoBack.
    previous_screen: Option<ScreenId>,
    /// All screen components, keyed by ScreenId.
    screens: HashMap<ScreenId, Box<dyn Component>>,
    /// Whether the app should keep running.
    running: bool,
    /// Connection status indicator.
    connection_status: ConnectionStatus,
    /// Help overlay visibility.
    help_visible: bool,
    /// Search overlay visibility.
    search_active: bool,
    /// Terminal size for responsive layout.
    terminal_size: (u16, u16),
    /// Action sender — components can dispatch actions through this.
    action_tx: mpsc::UnboundedSender<Action>,
    /// Action receiver — main loop drains this.
    action_rx: mpsc::UnboundedReceiver<Action>,
}

impl App {
    /// Create a new App with all placeholder screens.
    pub fn new() -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let screens: HashMap<ScreenId, Box<dyn Component>> =
            create_screens().into_iter().collect();

        Self {
            active_screen: ScreenId::Dashboard,
            previous_screen: None,
            screens,
            running: true,
            connection_status: ConnectionStatus::default(),
            help_visible: false,
            search_active: false,
            terminal_size: (0, 0),
            action_tx,
            action_rx,
        }
    }

    /// Initialize all screen components with the action sender.
    fn init_screens(&mut self) -> Result<()> {
        for screen in self.screens.values_mut() {
            screen.init(self.action_tx.clone())?;
        }
        // Focus the initial screen
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            screen.set_focused(true);
        }
        Ok(())
    }

    /// Run the main event loop. This is the heart of the TUI.
    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;
        tui.enter()?;
        self.terminal_size = tui.size().unwrap_or((80, 24));
        self.init_screens()?;

        let mut events = EventReader::new(
            Duration::from_millis(250), // 4 Hz tick
            Duration::from_millis(33),  // ~30 FPS render
        );

        info!("TUI event loop started");

        while self.running {
            // 1. Wait for the next event
            let Some(event) = events.next().await else {
                break;
            };

            // 2. Map event → action(s)
            match event {
                Event::Key(key) => {
                    if let Some(action) = self.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(action) = self.handle_mouse_event(mouse)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Resize(w, h) => {
                    self.action_tx.send(Action::Resize(w, h))?;
                }
                Event::Tick => {
                    self.action_tx.send(Action::Tick)?;
                }
                Event::Render => {
                    self.action_tx.send(Action::Render)?;
                }
            }

            // 3. Drain and process all queued actions
            while let Ok(action) = self.action_rx.try_recv() {
                self.process_action(&action)?;

                if let Action::Render = action {
                    tui.draw(|frame| self.render(frame))?;
                }
            }
        }

        events.stop();
        info!("TUI event loop ended");
        Ok(())
    }

    /// Map a key event to an action. Global keys are handled here;
    /// screen-specific keys are delegated to the active screen component.
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // Global keys always take priority (except when search is active)
        if self.search_active {
            // In search mode, only Esc exits. Everything else goes to search.
            return match key.code {
                KeyCode::Esc => Ok(Some(Action::CloseSearch)),
                _ => Ok(None), // search input handling would go here
            };
        }

        if self.help_visible {
            // In help mode, Esc or ? closes help
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') => Ok(Some(Action::ToggleHelp)),
                _ => Ok(None),
            };
        }

        // Global keybindings
        match (key.modifiers, key.code) {
            // Quit
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(Some(Action::Quit)),
            (KeyModifiers::NONE, KeyCode::Char('q')) => return Ok(Some(Action::Quit)),

            // Help
            (KeyModifiers::NONE, KeyCode::Char('?')) => return Ok(Some(Action::ToggleHelp)),

            // Search
            (KeyModifiers::NONE, KeyCode::Char('/')) => return Ok(Some(Action::OpenSearch)),

            // Screen navigation via number keys
            (KeyModifiers::NONE, KeyCode::Char(c @ '1'..='8')) => {
                let n = c as u8 - b'0';
                if let Some(screen) = ScreenId::from_number(n) {
                    return Ok(Some(Action::SwitchScreen(screen)));
                }
            }

            // Tab / Shift+Tab for screen cycling
            (KeyModifiers::NONE, KeyCode::Tab) => {
                return Ok(Some(Action::SwitchScreen(self.active_screen.next())));
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                return Ok(Some(Action::SwitchScreen(self.active_screen.prev())));
            }

            // Esc — context-dependent back
            (KeyModifiers::NONE, KeyCode::Esc) => return Ok(Some(Action::GoBack)),

            _ => {}
        }

        // Delegate to active screen component
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_key_event(key);
        }

        Ok(None)
    }

    /// Handle mouse events (delegate to active screen).
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_mouse_event(mouse);
        }
        Ok(None)
    }

    /// Process a single action — update app state and propagate to components.
    fn process_action(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::Quit => {
                self.running = false;
            }

            Action::Resize(w, h) => {
                self.terminal_size = (*w, *h);
            }

            Action::SwitchScreen(target) => {
                if *target != self.active_screen {
                    debug!("switching screen: {} → {}", self.active_screen, target);
                    // Unfocus current screen
                    if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                        screen.set_focused(false);
                    }
                    self.previous_screen = Some(self.active_screen);
                    self.active_screen = *target;
                    // Focus new screen
                    if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                        screen.set_focused(true);
                    }
                }
            }

            Action::GoBack => {
                if let Some(prev) = self.previous_screen.take() {
                    self.action_tx.send(Action::SwitchScreen(prev))?;
                }
            }

            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
            }

            Action::OpenSearch => {
                self.search_active = true;
            }

            Action::CloseSearch => {
                self.search_active = false;
            }

            Action::Connected => {
                self.connection_status = ConnectionStatus::Connected;
            }

            Action::Disconnected(_) => {
                self.connection_status = ConnectionStatus::Disconnected;
            }

            Action::Reconnecting => {
                self.connection_status = ConnectionStatus::Reconnecting;
            }

            // Render is handled in the main loop, not here
            Action::Render | Action::Tick => {}

            // Propagate everything else to the active screen
            other => {
                if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                    if let Some(follow_up) = screen.update(other)? {
                        self.action_tx.send(follow_up)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Render the full application frame.
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Layout: [screen content] [tab bar] [status bar]
        let layout = Layout::vertical([
            Constraint::Min(1),    // Screen content
            Constraint::Length(1), // Tab bar
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        let content_area = layout[0];
        let tab_area = layout[1];
        let status_area = layout[2];

        // Render active screen
        if let Some(screen) = self.screens.get(&self.active_screen) {
            screen.render(frame, content_area);
        }

        // Render tab bar
        self.render_tab_bar(frame, tab_area);

        // Render status bar
        self.render_status_bar(frame, status_area);

        // Render help overlay on top (if visible)
        if self.help_visible {
            self.render_help_overlay(frame, area);
        }
    }

    /// Render the bottom tab bar showing all 8 screens.
    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let titles: Vec<Line> = ScreenId::ALL
            .iter()
            .map(|&id| {
                let style = if id == self.active_screen {
                    theme::tab_active()
                } else {
                    theme::tab_inactive()
                };
                Line::from(Span::styled(
                    format!(" {} {} ", id.number(), id.label()),
                    style,
                ))
            })
            .collect();

        let tabs = Tabs::new(titles)
            .divider(Span::styled(" ", theme::key_hint()))
            .select(
                ScreenId::ALL
                    .iter()
                    .position(|&s| s == self.active_screen)
                    .unwrap_or(0),
            );

        frame.render_widget(tabs, area);
    }

    /// Render the bottom status bar with connection status and key hints.
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let connection_indicator = match &self.connection_status {
            ConnectionStatus::Connected => {
                Span::styled("● connected", Style::default().fg(theme::SUCCESS_GREEN))
            }
            ConnectionStatus::Disconnected => {
                Span::styled("○ disconnected", Style::default().fg(theme::ERROR_RED))
            }
            ConnectionStatus::Reconnecting => {
                Span::styled("◐ reconnecting", Style::default().fg(theme::ELECTRIC_YELLOW))
            }
            ConnectionStatus::Connecting => {
                Span::styled("◐ connecting", Style::default().fg(theme::ELECTRIC_YELLOW))
            }
        };

        let hints = Span::styled(
            " │ ? help  / search  q quit",
            theme::key_hint(),
        );

        let line = Line::from(vec![
            Span::raw(" "),
            connection_indicator,
            hints,
        ]);

        frame.render_widget(Paragraph::new(line), area);
    }

    /// Render the help overlay centered on screen.
    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_width = 60u16.min(area.width.saturating_sub(4));
        let help_height = 22u16.min(area.height.saturating_sub(4));

        let x = (area.width.saturating_sub(help_width)) / 2;
        let y = (area.height.saturating_sub(help_height)) / 2;

        let help_area = Rect::new(
            area.x + x,
            area.y + y,
            help_width,
            help_height,
        );

        // Clear the background
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BG_DARK)),
            help_area,
        );

        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(help_area);
        frame.render_widget(block, help_area);

        let help_text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Navigation", Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(Span::styled(
                "  ─────────",
                theme::key_hint(),
            )),
            Line::from(vec![
                Span::styled("  1-8       ", theme::key_hint_key()),
                Span::styled("Jump to screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Tab       ", theme::key_hint_key()),
                Span::styled("Next screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  j/k ↑/↓   ", theme::key_hint_key()),
                Span::styled("Move up/down", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Enter     ", theme::key_hint_key()),
                Span::styled("Select / expand", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Esc       ", theme::key_hint_key()),
                Span::styled("Back / close", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  g/G       ", theme::key_hint_key()),
                Span::styled("Top / bottom", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+d/u  ", theme::key_hint_key()),
                Span::styled("Page down / up", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Global", Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(Span::styled(
                "  ──────",
                theme::key_hint(),
            )),
            Line::from(vec![
                Span::styled("  /         ", theme::key_hint_key()),
                Span::styled("Search              ", theme::key_hint()),
                Span::styled("?  ", theme::key_hint_key()),
                Span::styled("This help", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  s         ", theme::key_hint_key()),
                Span::styled("Sort column          ", theme::key_hint()),
                Span::styled("f  ", theme::key_hint_key()),
                Span::styled("Filter", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  q         ", theme::key_hint_key()),
                Span::styled("Quit", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "                         Esc or ? to close",
                theme::key_hint(),
            )),
        ];

        let paragraph = Paragraph::new(help_text);
        frame.render_widget(paragraph, inner);
    }
}
