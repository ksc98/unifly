//! Clients screen — client table with type filters (spec §2.3).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::{Client, ClientType};

use crate::action::{Action, ClientTypeFilter};
use crate::component::Component;
use crate::theme;
use crate::widgets::{bytes_fmt, sub_tabs};

pub struct ClientsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    clients: Arc<Vec<Arc<Client>>>,
    table_state: TableState,
    filter: ClientTypeFilter,
    search_query: String,
    detail_open: bool,
    detail_client_idx: usize,
}

impl ClientsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            clients: Arc::new(Vec::new()),
            table_state: TableState::default(),
            filter: ClientTypeFilter::All,
            search_query: String::new(),
            detail_open: false,
            detail_client_idx: 0,
        }
    }

    fn filtered_clients(&self) -> Vec<&Arc<Client>> {
        let q = self.search_query.to_lowercase();
        self.clients
            .iter()
            .filter(|c| match self.filter {
                ClientTypeFilter::All => true,
                ClientTypeFilter::Wireless => c.client_type == ClientType::Wireless,
                ClientTypeFilter::Wired => c.client_type == ClientType::Wired,
                ClientTypeFilter::Vpn => c.client_type == ClientType::Vpn,
                ClientTypeFilter::Guest => c.is_guest,
            })
            .filter(|c| {
                if q.is_empty() {
                    return true;
                }
                c.name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                    || c.hostname
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || c.ip
                        .map(|ip| ip.to_string())
                        .unwrap_or_default()
                        .contains(&q)
                    || c.mac.to_string().contains(&q)
            })
            .collect()
    }

    fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    fn select(&mut self, idx: usize) {
        let filtered = self.filtered_clients();
        let clamped = if filtered.is_empty() {
            0
        } else {
            idx.min(filtered.len() - 1)
        };
        self.table_state.select(Some(clamped));
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    fn move_selection(&mut self, delta: isize) {
        let filtered = self.filtered_clients();
        if filtered.is_empty() {
            return;
        }
        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, filtered.len() as isize - 1);
        self.select(next as usize);
    }

    fn cycle_filter(&mut self) {
        self.filter = match self.filter {
            ClientTypeFilter::All => ClientTypeFilter::Wireless,
            ClientTypeFilter::Wireless => ClientTypeFilter::Wired,
            ClientTypeFilter::Wired => ClientTypeFilter::Vpn,
            ClientTypeFilter::Vpn => ClientTypeFilter::Guest,
            ClientTypeFilter::Guest => ClientTypeFilter::All,
        };
        self.table_state.select(Some(0));
    }

    #[allow(clippy::unused_self, clippy::too_many_lines, clippy::as_conversions)]
    fn render_detail(&self, frame: &mut Frame, area: Rect, client: &Client) {
        let name = client
            .name
            .as_deref()
            .or(client.hostname.as_deref())
            .unwrap_or("Unknown");
        let ip = client.ip.map_or_else(|| "─".into(), |ip| ip.to_string());
        let mac = client.mac.to_string();
        let type_str = format!("{:?}", client.client_type);

        let title = format!(" {name}  ·  {type_str}  ·  {ip}  ·  {mac} ");
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let network = client
            .network_id
            .as_ref()
            .map_or_else(|| "─".into(), std::string::ToString::to_string);
        let signal = client
            .wireless
            .as_ref()
            .and_then(|w| w.signal_dbm)
            .map_or_else(|| "─".into(), |dbm| format!("{dbm} dBm"));
        let channel = client
            .wireless
            .as_ref()
            .and_then(|w| w.channel)
            .map_or_else(|| "─".into(), |ch| ch.to_string());
        let ssid = client
            .wireless
            .as_ref()
            .and_then(|w| w.ssid.as_deref())
            .unwrap_or("─");
        let tx = client
            .tx_bytes
            .map_or_else(|| "─".into(), bytes_fmt::fmt_bytes_short);
        let rx = client
            .rx_bytes
            .map_or_else(|| "─".into(), bytes_fmt::fmt_bytes_short);
        let duration = client.connected_at.map_or_else(
            || "─".into(),
            |ts| {
                let dur = chrono::Utc::now().signed_duration_since(ts);
                #[allow(clippy::cast_sign_loss)]
                let secs = dur.num_seconds().max(0) as u64;
                bytes_fmt::fmt_uptime(secs)
            },
        );
        let guest = if client.is_guest { "yes" } else { "no" };
        let blocked = if client.blocked { "yes" } else { "no" };

        let detail_layout =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Network        ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(network, Style::default().fg(theme::NEON_CYAN)),
                Span::styled(
                    "       SSID         ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(ssid, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  Signal         ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&signal, Style::default().fg(theme::NEON_CYAN)),
                Span::styled(
                    "       Channel      ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(&channel, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  TX             ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&tx, Style::default().fg(theme::CORAL)),
                Span::styled(
                    "       RX           ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(&rx, Style::default().fg(theme::CORAL)),
            ]),
            Line::from(vec![
                Span::styled("  Duration       ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&duration, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  Guest          ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(guest, Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    "       Blocked      ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(
                    blocked,
                    Style::default().fg(if client.blocked {
                        theme::ERROR_RED
                    } else {
                        theme::DIM_WHITE
                    }),
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), detail_layout[0]);

        let hints = Line::from(vec![
            Span::styled("  b ", theme::key_hint_key()),
            Span::styled("block  ", theme::key_hint()),
            Span::styled("B ", theme::key_hint_key()),
            Span::styled("unblock  ", theme::key_hint()),
            Span::styled("x ", theme::key_hint_key()),
            Span::styled("kick  ", theme::key_hint()),
            Span::styled("Esc ", theme::key_hint_key()),
            Span::styled("back", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), detail_layout[1]);
    }

    fn filter_index(&self) -> usize {
        match self.filter {
            ClientTypeFilter::All => 0,
            ClientTypeFilter::Wireless => 1,
            ClientTypeFilter::Wired => 2,
            ClientTypeFilter::Vpn => 3,
            ClientTypeFilter::Guest => 4,
        }
    }
}

/// Client type icon with fallback letter.
#[allow(dead_code)]
fn client_type_span(client: &Client) -> Span<'static> {
    if client.is_guest {
        return Span::styled("G", Style::default().fg(theme::ELECTRIC_YELLOW));
    }
    match client.client_type {
        ClientType::Wireless => Span::styled("W", Style::default().fg(theme::NEON_CYAN)),
        ClientType::Wired => Span::styled("E", Style::default().fg(theme::DIM_WHITE)),
        ClientType::Vpn => Span::styled("V", Style::default().fg(theme::ELECTRIC_PURPLE)),
        ClientType::Teleport => Span::styled("T", Style::default().fg(theme::CORAL)),
        _ => Span::styled("?", Style::default().fg(theme::DIM_WHITE)),
    }
}

impl Component for ClientsScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if self.detail_open {
            return match key.code {
                KeyCode::Esc => {
                    self.detail_open = false;
                    Ok(Some(Action::CloseDetail))
                }
                KeyCode::Char('b') => {
                    let filtered = self.filtered_clients();
                    if let Some(client) = filtered.get(self.detail_client_idx) {
                        Ok(Some(Action::RequestBlockClient(client.id.clone())))
                    } else {
                        Ok(None)
                    }
                }
                KeyCode::Char('B') => {
                    let filtered = self.filtered_clients();
                    if let Some(client) = filtered.get(self.detail_client_idx) {
                        Ok(Some(Action::RequestUnblockClient(client.id.clone())))
                    } else {
                        Ok(None)
                    }
                }
                KeyCode::Char('x') => {
                    let filtered = self.filtered_clients();
                    if let Some(client) = filtered.get(self.detail_client_idx) {
                        Ok(Some(Action::RequestKickClient(client.id.clone())))
                    } else {
                        Ok(None)
                    }
                }
                _ => Ok(None),
            };
        }

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
                let len = self.filtered_clients().len();
                if len > 0 {
                    self.select(len - 1);
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
            KeyCode::Tab => {
                self.cycle_filter();
                Ok(Some(Action::FilterClientType(self.filter)))
            }
            KeyCode::Enter => {
                let idx = self.selected_index();
                let id = self.filtered_clients().get(idx).map(|c| c.id.clone());
                if let Some(id) = id {
                    self.detail_open = true;
                    self.detail_client_idx = idx;
                    Ok(Some(Action::OpenClientDetail(id)))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('b') => {
                let filtered = self.filtered_clients();
                if let Some(client) = filtered.get(self.selected_index()) {
                    Ok(Some(Action::RequestBlockClient(client.id.clone())))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('B') => {
                let filtered = self.filtered_clients();
                if let Some(client) = filtered.get(self.selected_index()) {
                    Ok(Some(Action::RequestUnblockClient(client.id.clone())))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('x') => {
                let filtered = self.filtered_clients();
                if let Some(client) = filtered.get(self.selected_index()) {
                    Ok(Some(Action::RequestKickClient(client.id.clone())))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
                let filtered_len = self.filtered_clients().len();
                if filtered_len > 0 && self.selected_index() >= filtered_len {
                    self.select(filtered_len - 1);
                }
            }
            Action::FilterClientType(filter) => {
                self.filter = *filter;
                self.table_state.select(Some(0));
            }
            Action::SearchInput(query) => {
                self.search_query.clone_from(query);
                self.table_state.select(Some(0));
            }
            Action::CloseSearch => {
                self.search_query.clear();
            }
            Action::CloseDetail => {
                self.detail_open = false;
            }
            _ => {}
        }
        Ok(None)
    }

    #[allow(clippy::too_many_lines, clippy::as_conversions)]
    fn render(&self, frame: &mut Frame, area: Rect) {
        let filtered = self.filtered_clients();
        let total = self.clients.len();
        let shown = filtered.len();

        let title = if self.search_query.is_empty() {
            format!(" Clients ({shown}/{total}) ")
        } else {
            format!(" Clients ({shown}/{total}) [\"{}\" ] ", self.search_query)
        };
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

        // Split for table + optional detail panel
        let (table_area, detail_area) = if self.detail_open {
            let chunks = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(inner);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner, None)
        };

        let layout = Layout::vertical([
            Constraint::Length(1), // filter tabs
            Constraint::Min(1),    // table
            Constraint::Length(1), // hints
        ])
        .split(table_area);

        // Filter tab bar
        let filter_labels = &["All", "Wireless", "Wired", "VPN", "Guest"];
        let filter_line = sub_tabs::render_sub_tabs(filter_labels, self.filter_index());
        frame.render_widget(Paragraph::new(filter_line), layout[0]);

        // Table header
        let header = Row::new(vec![
            Cell::from("Type").style(theme::table_header()),
            Cell::from("Name").style(theme::table_header()),
            Cell::from("IP").style(theme::table_header()),
            Cell::from("MAC").style(theme::table_header()),
            Cell::from("Signal").style(theme::table_header()),
            Cell::from("TX/RX").style(theme::table_header()),
            Cell::from("Duration").style(theme::table_header()),
        ]);

        // Table rows
        let selected_idx = self.selected_index();
        let rows: Vec<Row> =
            filtered
                .iter()
                .enumerate()
                .map(|(i, client)| {
                    let is_selected = i == selected_idx;
                    let prefix = if is_selected { "▸" } else { " " };

                    let type_char = match client.client_type {
                        ClientType::Wireless => "W",
                        ClientType::Wired => "E",
                        ClientType::Vpn => "V",
                        ClientType::Teleport => "T",
                        _ => "?",
                    };
                    let type_str = if client.is_guest {
                        format!("{prefix}G")
                    } else {
                        format!("{prefix}{type_char}")
                    };

                    let name = client
                        .name
                        .as_deref()
                        .or(client.hostname.as_deref())
                        .unwrap_or("unknown");

                    let ip = client.ip.map_or_else(|| "─".into(), |ip| ip.to_string());

                    let mac = client.mac.to_string();

                    let signal = client.wireless.as_ref().and_then(|w| w.signal_dbm).map_or(
                        "····",
                        |dbm| {
                            if dbm >= -50 {
                                "▂▄▆█"
                            } else if dbm >= -60 {
                                "▂▄▆ "
                            } else if dbm >= -70 {
                                "▂▄  "
                            } else if dbm >= -80 {
                                "▂   "
                            } else {
                                "·   "
                            }
                        },
                    );

                    let traffic = bytes_fmt::fmt_tx_rx(
                        client.tx_bytes.unwrap_or(0),
                        client.rx_bytes.unwrap_or(0),
                    );

                    let duration = client.connected_at.map_or_else(
                        || "─".into(),
                        |ts| {
                            let dur = chrono::Utc::now().signed_duration_since(ts);
                            #[allow(clippy::cast_sign_loss)]
                            let secs = dur.num_seconds().max(0) as u64;
                            bytes_fmt::fmt_uptime(secs)
                        },
                    );

                    let type_color = if client.is_guest {
                        theme::ELECTRIC_YELLOW
                    } else {
                        match client.client_type {
                            ClientType::Wireless => theme::NEON_CYAN,
                            ClientType::Vpn => theme::ELECTRIC_PURPLE,
                            ClientType::Teleport => theme::CORAL,
                            _ => theme::DIM_WHITE,
                        }
                    };

                    let signal_color = client.wireless.as_ref().and_then(|w| w.signal_dbm).map_or(
                        theme::BORDER_GRAY,
                        |dbm| {
                            if dbm >= -50 {
                                theme::SUCCESS_GREEN
                            } else if dbm >= -60 {
                                theme::NEON_CYAN
                            } else if dbm >= -70 {
                                theme::ELECTRIC_YELLOW
                            } else if dbm >= -80 {
                                theme::CORAL
                            } else {
                                theme::ERROR_RED
                            }
                        },
                    );

                    let row_style = if is_selected {
                        theme::table_selected()
                    } else {
                        theme::table_row()
                    };

                    Row::new(vec![
                        Cell::from(type_str).style(Style::default().fg(type_color)),
                        Cell::from(name.to_string()).style(
                            Style::default()
                                .fg(theme::NEON_CYAN)
                                .add_modifier(if is_selected {
                                    Modifier::BOLD
                                } else {
                                    Modifier::empty()
                                }),
                        ),
                        Cell::from(ip).style(Style::default().fg(theme::CORAL)),
                        Cell::from(mac),
                        Cell::from(signal.to_string()).style(Style::default().fg(signal_color)),
                        Cell::from(traffic),
                        Cell::from(duration),
                    ])
                    .style(row_style)
                })
                .collect();

        let widths = [
            Constraint::Length(3),
            Constraint::Min(14),
            Constraint::Length(15),
            Constraint::Length(17),
            Constraint::Length(6),
            Constraint::Length(11),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.table_state;
        frame.render_stateful_widget(table, layout[1], &mut state);

        // Key hints
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("Enter ", theme::key_hint_key()),
            Span::styled("detail  ", theme::key_hint()),
            Span::styled("Tab ", theme::key_hint_key()),
            Span::styled("filter  ", theme::key_hint()),
            Span::styled("b ", theme::key_hint_key()),
            Span::styled("block  ", theme::key_hint()),
            Span::styled("x ", theme::key_hint_key()),
            Span::styled("kick", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[2]);

        // Render detail panel if open
        if let Some(detail_area) = detail_area {
            if let Some(client) = filtered.get(self.detail_client_idx) {
                self.render_detail(frame, detail_area, client);
            }
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Clients"
    }
}
