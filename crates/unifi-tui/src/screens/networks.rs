//! Networks screen — network table with inline detail expansion (spec §2.4).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::Network;

use crate::action::Action;
use crate::component::Component;
use crate::theme;

pub struct NetworksScreen {
    focused: bool,
    networks: Arc<Vec<Arc<Network>>>,
    table_state: TableState,
    detail_open: bool,
}

impl NetworksScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            networks: Arc::new(Vec::new()),
            table_state: TableState::default(),
            detail_open: false,
        }
    }

    fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    fn select(&mut self, idx: usize) {
        let clamped = if self.networks.is_empty() {
            0
        } else {
            idx.min(self.networks.len() - 1)
        };
        self.table_state.select(Some(clamped));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.networks.is_empty() {
            return;
        }
        let current = self.selected_index() as isize;
        let next = (current + delta).clamp(0, self.networks.len() as isize - 1);
        self.select(next as usize);
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect, network: &Network) {
        let vlan_str = network
            .vlan_id
            .map(|v| format!("VLAN {v}"))
            .unwrap_or_else(|| "─".into());
        let subnet = network.subnet.as_deref().unwrap_or("─");

        let title = format!(" {}  ·  {vlan_str}  ·  {subnet} ", network.name);
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mgmt = network
            .management
            .as_ref()
            .map(|m| format!("{m:?}"))
            .unwrap_or_else(|| "─".into());

        let dhcp_str = network
            .dhcp
            .as_ref()
            .map(|d| {
                if d.enabled {
                    let start = d
                        .range_start
                        .map(|ip| ip.to_string())
                        .unwrap_or_else(|| "?".into());
                    let stop = d
                        .range_stop
                        .map(|ip| ip.to_string())
                        .unwrap_or_else(|| "?".into());
                    format!("Server ({start} - {stop})")
                } else {
                    "Disabled".into()
                }
            })
            .unwrap_or_else(|| "─".into());

        let internet = if network.internet_access_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let isolation = if network.isolation_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let mdns = if network.mdns_forwarding_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let ipv6 = if network.ipv6_enabled {
            network
                .ipv6_mode
                .as_ref()
                .map(|m| format!("{m:?}"))
                .unwrap_or_else(|| "Enabled".into())
        } else {
            "Disabled".into()
        };

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Management     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&mgmt, Style::default().fg(theme::NEON_CYAN)),
                Span::styled(
                    "     Internet Access  ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(internet, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  DHCP           ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&dhcp_str, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  Isolation      ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(isolation, Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    "     mDNS Forwarding  ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(mdns, Style::default().fg(theme::DIM_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("  IPv6           ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&ipv6, Style::default().fg(theme::DIM_WHITE)),
            ]),
        ];

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

impl Component for NetworksScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                Ok(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                Ok(None)
            }
            KeyCode::Char('g') => {
                self.select(0);
                Ok(Some(Action::ScrollToTop))
            }
            KeyCode::Char('G') => {
                if !self.networks.is_empty() {
                    self.select(self.networks.len() - 1);
                }
                Ok(Some(Action::ScrollToBottom))
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
                Ok(Some(Action::PageDown))
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
                Ok(Some(Action::PageUp))
            }
            KeyCode::Enter => {
                self.detail_open = !self.detail_open;
                Ok(None)
            }
            KeyCode::Esc if self.detail_open => {
                self.detail_open = false;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        if let Action::NetworksUpdated(networks) = action {
            self.networks = Arc::clone(networks);
            if !self.networks.is_empty() && self.selected_index() >= self.networks.len() {
                self.select(self.networks.len() - 1);
            }
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let count = self.networks.len();
        let title = format!(" Networks ({count}) ");
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

        let (table_area, detail_area) = if self.detail_open {
            let chunks = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(inner);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner, None)
        };

        let layout = Layout::vertical([
            Constraint::Min(1),    // table
            Constraint::Length(1), // hints
        ])
        .split(table_area);

        // Table
        let header = Row::new(vec![
            Cell::from("Name").style(theme::table_header()),
            Cell::from("VLAN").style(theme::table_header()),
            Cell::from("Subnet").style(theme::table_header()),
            Cell::from("DHCP").style(theme::table_header()),
            Cell::from("Type").style(theme::table_header()),
            Cell::from("Zone").style(theme::table_header()),
        ]);

        let selected_idx = self.selected_index();
        let rows: Vec<Row> = self
            .networks
            .iter()
            .enumerate()
            .map(|(i, net)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▸" } else { " " };

                let vlan = net
                    .vlan_id
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "─".into());
                let subnet = net.subnet.as_deref().unwrap_or("─");
                let dhcp = net
                    .dhcp
                    .as_ref()
                    .map(|d| {
                        if d.enabled {
                            let start = d.range_start.map(|ip| ip.to_string()).unwrap_or_default();
                            let stop = d.range_stop.map(|ip| ip.to_string()).unwrap_or_default();
                            format!("{start}-{stop}")
                        } else {
                            "Disabled".into()
                        }
                    })
                    .unwrap_or_else(|| "─".into());
                let mgmt = net
                    .management
                    .as_ref()
                    .map(|m| format!("{m:?}"))
                    .unwrap_or_else(|| "─".into());

                // Zone ID placeholder — we'd resolve this to a name with zone data
                let zone = net.firewall_zone_id.as_ref().map(|_| "Zone").unwrap_or("─");

                let row_style = if is_selected {
                    theme::table_selected()
                } else {
                    theme::table_row()
                };

                Row::new(vec![
                    Cell::from(format!("{prefix}{}", net.name)).style(
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(vlan),
                    Cell::from(subnet.to_string()).style(Style::default().fg(theme::CORAL)),
                    Cell::from(dhcp),
                    Cell::from(mgmt),
                    Cell::from(zone.to_string()),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Min(14),
            Constraint::Length(6),
            Constraint::Length(16),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.table_state;
        frame.render_stateful_widget(table, layout[0], &mut state);

        // Hints
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("Enter ", theme::key_hint_key()),
            Span::styled("expand  ", theme::key_hint()),
            Span::styled("Esc ", theme::key_hint_key()),
            Span::styled("collapse", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);

        // Detail
        if let Some(detail_area) = detail_area {
            if let Some(network) = self.networks.get(selected_idx) {
                self.render_detail(frame, detail_area, network);
            }
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &str {
        "Networks"
    }
}
