//! Clients screen — full client table matching UniFi web UI (spec §2.3).

use std::collections::HashMap;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use tokio::sync::mpsc::UnboundedSender;

use unifly_core::{Client, ClientType, Device};

use crate::action::{Action, ClientTypeFilter};
use crate::component::Component;
use crate::theme;
use crate::widgets::{bytes_fmt, sub_tabs};

/// Which column is used for sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientSortColumn {
    #[default]
    Activity,
    Download,
    Upload,
}

pub struct ClientsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    clients: Arc<Vec<Arc<Client>>>,
    devices: Arc<Vec<Arc<Device>>>,
    client_daily_usage: Arc<HashMap<String, (u64, u64)>>,
    table_state: TableState,
    filter: ClientTypeFilter,
    search_query: String,
    detail_open: bool,
    detail_client_idx: usize,
    sort_column: ClientSortColumn,
    cached_filtered: Vec<Arc<Client>>,
    device_name_map: HashMap<String, String>,
    update_count: u64,
}

impl ClientsScreen {
    pub fn new() -> Self {
        let mut screen = Self {
            focused: false,
            action_tx: None,
            clients: Arc::new(Vec::new()),
            devices: Arc::new(Vec::new()),
            client_daily_usage: Arc::new(HashMap::new()),
            table_state: TableState::default(),
            filter: ClientTypeFilter::All,
            search_query: String::new(),
            detail_open: false,
            detail_client_idx: 0,
            sort_column: ClientSortColumn::default(),
            cached_filtered: Vec::new(),
            device_name_map: HashMap::new(),
            update_count: 0,
        };
        screen.recompute_filtered();
        screen
    }

    fn recompute_filtered(&mut self) {
        let q = self.search_query.to_lowercase();
        let mut clients: Vec<_> = self
            .clients
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
                c.name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
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
                    || c.oui
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
            })
            .cloned()
            .collect();
        // Sort descending by the active sort column
        let sort_col = self.sort_column;
        clients.sort_by(|a, b| {
            let key = |c: &Client| -> u64 {
                match sort_col {
                    ClientSortColumn::Activity => c
                        .bandwidth
                        .as_ref()
                        .map_or(0, |bw| bw.tx_bytes_per_sec.saturating_add(bw.rx_bytes_per_sec)),
                    ClientSortColumn::Download => {
                        c.bandwidth.as_ref().map_or(0, |bw| bw.rx_bytes_per_sec)
                    }
                    ClientSortColumn::Upload => {
                        c.bandwidth.as_ref().map_or(0, |bw| bw.tx_bytes_per_sec)
                    }
                }
            };
            key(b).cmp(&key(a))
        });
        self.cached_filtered = clients;
    }

    fn filtered_clients(&self) -> &[Arc<Client>] {
        &self.cached_filtered
    }

    fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    fn select(&mut self, idx: usize) {
        let filtered_len = self.filtered_clients().len();
        let clamped = if filtered_len == 0 {
            0
        } else {
            idx.min(filtered_len - 1)
        };
        self.table_state.select(Some(clamped));
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    fn move_selection(&mut self, delta: isize) {
        let filtered_len = self.filtered_clients().len();
        if filtered_len == 0 {
            return;
        }
        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, filtered_len as isize - 1);
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

    /// Look up the device name for a client's uplink (AP or Switch).
    fn connection_name(&self, client: &Client) -> String {
        let uplink_mac = client.uplink_device_mac.as_ref();
        if let Some(mac) = uplink_mac {
            if let Some(name) = self.device_name_map.get(&mac.to_string()) {
                return name.clone();
            }
            // MAC found but no device match — show MAC suffix
            let s = mac.as_str();
            s.get(s.len().saturating_sub(8)..).unwrap_or(s).to_string()
        } else {
            "─".into()
        }
    }

    /// Derive WiFi technology string from frequency/channel.
    fn technology_str(client: &Client) -> &'static str {
        match client.client_type {
            ClientType::Wired => "Wired",
            ClientType::Vpn | ClientType::Teleport => "VPN",
            ClientType::Wireless => {
                client.wireless.as_ref().map_or("─", |w| {
                    w.frequency_ghz.map_or("─", |f| {
                        if f >= 5.9 {
                            "WiFi 6E"
                        } else if f >= 4.9 {
                            "WiFi 5"
                        } else {
                            "WiFi 4"
                        }
                    })
                })
            }
            _ => "─",
        }
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
            .network_name
            .as_deref()
            .unwrap_or("─");
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
        let vendor = client.oui.as_deref().unwrap_or("─");
        let connection = self.connection_name(client);
        let technology = Self::technology_str(client);

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
                Span::styled("  Vendor         ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(vendor, Style::default().fg(theme::NEON_CYAN)),
                Span::styled(
                    "       Connection   ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(&connection, Style::default().fg(theme::CORAL)),
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
                Span::styled("  Technology     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(technology, Style::default().fg(theme::NEON_CYAN)),
                Span::styled(
                    "       TX           ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(&tx, Style::default().fg(theme::CORAL)),
            ]),
            Line::from(vec![
                Span::styled("  RX             ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(&rx, Style::default().fg(theme::CORAL)),
                Span::styled(
                    "       Uptime       ",
                    Style::default().fg(theme::DIM_WHITE),
                ),
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

/// Experience percentage color.
fn experience_color(pct: u8) -> ratatui::style::Color {
    if pct >= 80 {
        theme::SUCCESS_GREEN
    } else if pct >= 50 {
        theme::ELECTRIC_YELLOW
    } else {
        theme::ERROR_RED
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
                let filtered = self.filtered_clients();
                let id = filtered.get(idx).map(|c| c.id.clone());
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
            KeyCode::Char('A') => {
                self.sort_column = ClientSortColumn::Activity;
                self.recompute_filtered();
                Ok(None)
            }
            KeyCode::Char('D') => {
                self.sort_column = ClientSortColumn::Download;
                self.recompute_filtered();
                Ok(None)
            }
            KeyCode::Char('U') => {
                self.sort_column = ClientSortColumn::Upload;
                self.recompute_filtered();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::ClientsUpdated(clients) => {
                // Ignore empty updates — prevents blanking during reconnect
                if clients.is_empty() && !self.clients.is_empty() {
                    return Ok(None);
                }
                self.clients = Arc::clone(clients);
                self.update_count += 1;
                // Debug: log bandwidth stats across all clients
                let active: Vec<_> = clients.iter()
                    .filter(|c| c.bandwidth.as_ref().map_or(false, |bw| bw.tx_bytes_per_sec + bw.rx_bytes_per_sec > 100))
                    .map(|c| {
                        let bw = c.bandwidth.as_ref().unwrap();
                        format!("{}:{}↑{}↓",
                            c.name.as_deref().or(c.hostname.as_deref()).unwrap_or("?"),
                            bw.tx_bytes_per_sec, bw.rx_bytes_per_sec)
                    })
                    .collect();
                tracing::info!(total=clients.len(), active_count=active.len(), ?active, "clients update");
                self.recompute_filtered();
                let filtered_len = self.filtered_clients().len();
                if filtered_len > 0 && self.selected_index() >= filtered_len {
                    self.select(filtered_len - 1);
                }
            }
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                // Build device name lookup map
                self.device_name_map.clear();
                for dev in devices.iter() {
                    let name = dev
                        .name
                        .as_deref()
                        .unwrap_or(&dev.mac.to_string())
                        .to_string();
                    self.device_name_map.insert(dev.mac.to_string(), name);
                }
            }
            Action::ClientDailyUsageUpdated(usage) => {
                self.client_daily_usage = Arc::clone(usage);
            }
            Action::FilterClientType(filter) => {
                self.filter = *filter;
                self.recompute_filtered();
                self.table_state.select(Some(0));
            }
            Action::SearchInput(query) => {
                self.search_query.clone_from(query);
                self.recompute_filtered();
                self.table_state.select(Some(0));
            }
            Action::CloseSearch => {
                self.search_query.clear();
                self.recompute_filtered();
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
        let filtered = self.filtered_clients().to_vec();
        let total = self.clients.len();
        let shown = filtered.len();

        let title = if self.search_query.is_empty() {
            format!(" Clients ({shown}/{total}) [updates: {}] ", self.update_count)
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
            let chunks =
                Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
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

        // Determine available width to decide which columns to show
        let table_width = layout[1].width;
        let wide = table_width >= 180;
        let medium = table_width >= 120;

        // Table header — columns adapt to width
        let mut header_cells = vec![
            Cell::from("").style(theme::table_header()),      // type icon
            Cell::from("Name").style(theme::table_header()),
        ];
        if wide {
            header_cells.push(Cell::from("Vendor").style(theme::table_header()));
        }
        header_cells.push(Cell::from("Connection").style(theme::table_header()));
        if medium {
            header_cells.push(Cell::from("Network").style(theme::table_header()));
            header_cells.push(Cell::from("WiFi").style(theme::table_header()));
        }
        header_cells.push(Cell::from("Exp").style(theme::table_header()));
        if wide {
            header_cells.push(Cell::from("Tech").style(theme::table_header()));
            header_cells.push(Cell::from("Channel").style(theme::table_header()));
        }
        // Subtle background tint for the active sort column
        let sort_bg = Color::Rgb(50, 52, 68);
        let sort_header = |active: bool, label: &'static str| -> Cell<'static> {
            if active {
                Cell::from(label).style(
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .bg(sort_bg)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                )
            } else {
                Cell::from(label).style(theme::table_header())
            }
        };

        header_cells.push(Cell::from("IP Address").style(theme::table_header()));
        header_cells.push(sort_header(self.sort_column == ClientSortColumn::Activity, "Activity"));
        header_cells.push(sort_header(self.sort_column == ClientSortColumn::Download, "↓ Down"));
        header_cells.push(sort_header(self.sort_column == ClientSortColumn::Upload, "↑ Up"));
        header_cells.push(Cell::from("24h").style(theme::table_header()));
        header_cells.push(Cell::from("Uptime").style(theme::table_header()));

        let header = Row::new(header_cells);

        // Table rows
        let selected_idx = self.selected_index();
        let active_sort = self.sort_column;
        let rows: Vec<Row> = filtered
            .iter()
            .enumerate()
            .map(|(i, client)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▸" } else { " " };

                // Type icon
                let type_icon = match client.client_type {
                    ClientType::Wireless => "󰤥",
                    ClientType::Wired => "󰈀",
                    ClientType::Vpn => "󰌘",
                    ClientType::Teleport => "󰌘",
                    _ => "?",
                };
                let type_str = if client.is_guest {
                    format!("{prefix}G")
                } else {
                    format!("{prefix}{type_icon}")
                };
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

                // Name (with MAC suffix fallback)
                let name = client
                    .name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(client.hostname.as_deref().filter(|s| !s.is_empty()))
                    .or(client
                        .mac
                        .as_str()
                        .get(client.mac.as_str().len().saturating_sub(8)..))
                    .unwrap_or("unknown");

                // Vendor (from OUI)
                let vendor = client.oui.as_deref().unwrap_or("─");

                // Connection (AP/Switch name)
                let connection = self.connection_name(client);

                // Network name
                let network = client.network_name.as_deref().unwrap_or("─");

                // WiFi SSID
                let wifi = client
                    .wireless
                    .as_ref()
                    .and_then(|w| w.ssid.as_deref())
                    .unwrap_or("─");

                // Experience (satisfaction %)
                let (exp_str, exp_color) = client
                    .wireless
                    .as_ref()
                    .and_then(|w| w.satisfaction)
                    .map_or(("─".to_string(), theme::BORDER_GRAY), |pct| {
                        (format!("{pct}%"), experience_color(pct))
                    });

                // Technology
                let tech = Self::technology_str(client);

                // Channel
                let channel = client
                    .wireless
                    .as_ref()
                    .and_then(|w| w.channel)
                    .map_or_else(|| "─".into(), |ch| ch.to_string());

                // IP Address
                let ip = client.ip.map_or_else(|| "─".into(), |ip| ip.to_string());

                // Activity (combined live rate)
                let activity_bps = client.bandwidth.as_ref().map_or(0, |bw| {
                    bw.tx_bytes_per_sec.saturating_add(bw.rx_bytes_per_sec)
                });
                let activity = if activity_bps > 0 {
                    bytes_fmt::fmt_rate(activity_bps)
                } else {
                    "─".into()
                };

                // Download (live RX rate)
                let rx_bps = client
                    .bandwidth
                    .as_ref()
                    .map_or(0, |bw| bw.rx_bytes_per_sec);
                let download = if rx_bps > 0 {
                    bytes_fmt::fmt_rate(rx_bps)
                } else {
                    "─".into()
                };

                // Upload (live TX rate)
                let tx_bps = client
                    .bandwidth
                    .as_ref()
                    .map_or(0, |bw| bw.tx_bytes_per_sec);
                let upload = if tx_bps > 0 {
                    bytes_fmt::fmt_rate(tx_bps)
                } else {
                    "─".into()
                };

                // 24h Usage (from daily stats)
                let daily_total = self
                    .client_daily_usage
                    .get(&client.mac.to_string().to_lowercase())
                    .map(|(tx, rx)| tx.saturating_add(*rx))
                    .unwrap_or(0);
                let daily_usage = if daily_total > 0 {
                    bytes_fmt::fmt_bytes_short(daily_total)
                } else {
                    "─".into()
                };

                // Uptime
                let uptime = client.connected_at.map_or_else(
                    || "─".into(),
                    |ts| {
                        let dur = chrono::Utc::now().signed_duration_since(ts);
                        #[allow(clippy::cast_sign_loss)]
                        let secs = dur.num_seconds().max(0) as u64;
                        bytes_fmt::fmt_uptime_full(secs)
                    },
                );

                let row_style = if is_selected {
                    Style::default().bg(sort_bg)
                } else {
                    theme::table_row()
                };

                let name_style = Style::default()
                    .fg(theme::NEON_CYAN)
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    });

                // Build cells matching header
                let mut cells = vec![
                    Cell::from(type_str).style(Style::default().fg(type_color)),
                    Cell::from(name.to_string()).style(name_style),
                ];
                if wide {
                    cells.push(Cell::from(vendor.to_string()).style(Style::default().fg(theme::DIM_WHITE)));
                }
                cells.push(Cell::from(connection).style(Style::default().fg(theme::CORAL)));
                if medium {
                    cells.push(Cell::from(network.to_string()).style(Style::default().fg(theme::NEON_CYAN)));
                    cells.push(Cell::from(wifi.to_string()).style(Style::default().fg(theme::DIM_WHITE)));
                }
                cells.push(Cell::from(exp_str).style(Style::default().fg(exp_color)));
                if wide {
                    cells.push(Cell::from(tech).style(Style::default().fg(theme::DIM_WHITE)));
                    cells.push(Cell::from(channel).style(Style::default().fg(theme::DIM_WHITE)));
                }
                let sort_cell = |active: bool, text: String, fg: Color| -> Cell<'_> {
                    let mut s = Style::default().fg(fg);
                    if active {
                        s = s.bg(sort_bg);
                    }
                    Cell::from(text).style(s)
                };

                cells.push(Cell::from(ip).style(Style::default().fg(theme::CORAL)));
                cells.push(sort_cell(active_sort == ClientSortColumn::Activity, activity, theme::NEON_CYAN));
                cells.push(sort_cell(active_sort == ClientSortColumn::Download, download, theme::SUCCESS_GREEN));
                cells.push(sort_cell(active_sort == ClientSortColumn::Upload, upload, theme::CORAL));
                cells.push(Cell::from(daily_usage).style(Style::default().fg(theme::ELECTRIC_YELLOW)));
                cells.push(Cell::from(uptime));

                Row::new(cells).style(row_style)
            })
            .collect();

        // Column widths — flexible columns use Fill to absorb extra space
        let mut widths: Vec<Constraint> = vec![
            Constraint::Length(2),  // type icon
            Constraint::Fill(2),    // name (flex)
        ];
        if wide {
            widths.push(Constraint::Fill(2)); // vendor (flex)
        }
        widths.push(Constraint::Fill(2)); // connection (flex)
        if medium {
            widths.push(Constraint::Length(8));   // network
            widths.push(Constraint::Fill(2));     // wifi SSID (flex)
        }
        widths.push(Constraint::Length(5));  // experience
        if wide {
            widths.push(Constraint::Length(6));  // technology
            widths.push(Constraint::Length(5));  // channel
        }
        widths.push(Constraint::Length(14)); // IP
        widths.push(Constraint::Length(10)); // activity
        widths.push(Constraint::Length(10)); // download
        widths.push(Constraint::Length(10)); // upload
        widths.push(Constraint::Length(7));  // 24h usage
        widths.push(Constraint::Length(17)); // uptime

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(Style::default().bg(sort_bg));

        let mut state = self.table_state;
        frame.render_stateful_widget(table, layout[1], &mut state);

        // Key hints
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("A ", theme::key_hint_key()),
            Span::styled("activity  ", theme::key_hint()),
            Span::styled("D ", theme::key_hint_key()),
            Span::styled("download  ", theme::key_hint()),
            Span::styled("U ", theme::key_hint_key()),
            Span::styled("upload", theme::key_hint()),
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
