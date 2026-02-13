//! Networks screen — network table with inline detail expansion and editing overlay.

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, TableState};
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::{Network, UpdateNetworkRequest};

use crate::action::Action;
use crate::component::Component;
use crate::theme;

// ── Edit form state ──────────────────────────────────────────────────

/// Editable fields for a network. Initialized from the selected `Network`.
#[allow(clippy::struct_excessive_bools)]
struct NetworkEditState {
    name: String,
    vlan_id: String,
    dhcp_enabled: bool,
    isolation_enabled: bool,
    internet_access_enabled: bool,
    mdns_forwarding_enabled: bool,
    ipv6_enabled: bool,
    enabled: bool,
    /// Which field is currently focused (0-indexed).
    field_idx: usize,
}

impl NetworkEditState {
    fn from_network(net: &Network) -> Self {
        Self {
            name: net.name.clone(),
            vlan_id: net.vlan_id.map_or_else(String::new, |v| v.to_string()),
            dhcp_enabled: net.dhcp.as_ref().is_some_and(|d| d.enabled),
            isolation_enabled: net.isolation_enabled,
            internet_access_enabled: net.internet_access_enabled,
            mdns_forwarding_enabled: net.mdns_forwarding_enabled,
            ipv6_enabled: net.ipv6_enabled,
            enabled: net.enabled,
            field_idx: 0,
        }
    }

    const FIELD_COUNT: usize = 8;

    fn field_label(idx: usize) -> &'static str {
        match idx {
            0 => "Name",
            1 => "VLAN ID",
            2 => "Enabled",
            3 => "DHCP",
            4 => "Isolation",
            5 => "Internet",
            6 => "mDNS Fwd",
            7 => "IPv6",
            _ => "",
        }
    }

    fn field_value(&self, idx: usize) -> String {
        match idx {
            0 => self.name.clone(),
            1 => self.vlan_id.clone(),
            2 => bool_label(self.enabled),
            3 => bool_label(self.dhcp_enabled),
            4 => bool_label(self.isolation_enabled),
            5 => bool_label(self.internet_access_enabled),
            6 => bool_label(self.mdns_forwarding_enabled),
            7 => bool_label(self.ipv6_enabled),
            _ => String::new(),
        }
    }

    fn is_bool_field(idx: usize) -> bool {
        idx >= 2
    }

    fn toggle_bool(&mut self) {
        match self.field_idx {
            2 => self.enabled = !self.enabled,
            3 => self.dhcp_enabled = !self.dhcp_enabled,
            4 => self.isolation_enabled = !self.isolation_enabled,
            5 => self.internet_access_enabled = !self.internet_access_enabled,
            6 => self.mdns_forwarding_enabled = !self.mdns_forwarding_enabled,
            7 => self.ipv6_enabled = !self.ipv6_enabled,
            _ => {}
        }
    }

    fn handle_text_input(&mut self, ch: char) {
        match self.field_idx {
            0 => self.name.push(ch),
            1 if ch.is_ascii_digit() => self.vlan_id.push(ch),
            _ => {}
        }
    }

    fn handle_backspace(&mut self) {
        match self.field_idx {
            0 => { self.name.pop(); }
            1 => { self.vlan_id.pop(); }
            _ => {}
        }
    }

    fn build_request(&self) -> UpdateNetworkRequest {
        UpdateNetworkRequest {
            name: Some(self.name.clone()),
            vlan_id: self.vlan_id.parse().ok(),
            enabled: Some(self.enabled),
            dhcp_enabled: Some(self.dhcp_enabled),
            isolation_enabled: Some(self.isolation_enabled),
            internet_access_enabled: Some(self.internet_access_enabled),
            mdns_forwarding_enabled: Some(self.mdns_forwarding_enabled),
            ipv6_enabled: Some(self.ipv6_enabled),
            subnet: None,
        }
    }
}

fn bool_label(v: bool) -> String {
    if v { "Enabled".into() } else { "Disabled".into() }
}

// ── Main screen ──────────────────────────────────────────────────────

pub struct NetworksScreen {
    focused: bool,
    networks: Arc<Vec<Arc<Network>>>,
    table_state: TableState,
    detail_open: bool,
    edit_state: Option<NetworkEditState>,
    action_tx: Option<UnboundedSender<Action>>,
}

impl NetworksScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            networks: Arc::new(Vec::new()),
            table_state: TableState::default(),
            detail_open: false,
            edit_state: None,
            action_tx: None,
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
        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, self.networks.len() as isize - 1);
        self.select(next as usize);
    }

    fn selected_network(&self) -> Option<&Arc<Network>> {
        self.networks.get(self.selected_index())
    }

    // ── Detail rendering ────────────────────────────────────────

    #[allow(clippy::unused_self)]
    fn render_detail(&self, frame: &mut Frame, area: Rect, network: &Network) {
        let block = Block::default()
            .title(format!(" {} — Detail ", network.name))
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 3 || inner.width < 20 {
            return;
        }

        let label = Style::default().fg(theme::DIM_WHITE);
        let value = Style::default().fg(theme::NEON_CYAN);
        let enabled_style = Style::default().fg(theme::SUCCESS_GREEN);
        let disabled_style = Style::default().fg(theme::BORDER_GRAY);

        let gateway_str = network
            .gateway_ip
            .map_or_else(|| "—".into(), |ip| ip.to_string());
        let subnet_str = network.subnet.as_deref().unwrap_or("—");
        let vlan_str = network
            .vlan_id
            .map_or_else(|| "—".into(), |v| v.to_string());
        let mgmt_str = network
            .management
            .as_ref()
            .map_or_else(|| "—".into(), |m| format!("{m:?}"));

        let (dhcp_status, dhcp_style) = network.dhcp.as_ref().map_or(
            ("—", label),
            |d| {
                if d.enabled {
                    ("Enabled", enabled_style)
                } else {
                    ("Disabled", disabled_style)
                }
            },
        );

        let dhcp_range = network
            .dhcp
            .as_ref()
            .filter(|d| d.enabled)
            .map(|d| {
                let start = d.range_start.map_or_else(|| "?".into(), |ip| ip.to_string());
                let stop = d.range_stop.map_or_else(|| "?".into(), |ip| ip.to_string());
                format!("{start} — {stop}")
            });

        let lease_str = network
            .dhcp
            .as_ref()
            .and_then(|d| d.lease_time_secs)
            .map(format_lease_time);

        let dns_str = network
            .dhcp
            .as_ref()
            .filter(|d| !d.dns_servers.is_empty())
            .map(|d| {
                d.dns_servers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            });

        let bool_span = |v: bool, on: &str, off: &str| -> Span<'static> {
            if v {
                Span::styled(on.to_string(), enabled_style)
            } else {
                Span::styled(off.to_string(), disabled_style)
            }
        };

        let ipv6_str = if network.ipv6_enabled {
            network
                .ipv6_mode
                .as_ref()
                .map_or_else(|| "Enabled".into(), |m| format!("{m:?}"))
        } else {
            "Disabled".into()
        };
        let ipv6_style = if network.ipv6_enabled { enabled_style } else { disabled_style };

        // ── Section: Network Config ──
        let mut lines = vec![
            Line::from(Span::styled(
                " Network Config",
                Style::default().fg(theme::ELECTRIC_PURPLE).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                " ─────────────────────────────────────────",
                Style::default().fg(theme::BORDER_GRAY),
            )),
            Line::from(vec![
                Span::styled("  Gateway IP    ", label),
                Span::styled(gateway_str, value),
                Span::styled("       VLAN          ", label),
                Span::styled(vlan_str, value),
            ]),
            Line::from(vec![
                Span::styled("  Subnet        ", label),
                Span::styled(subnet_str.to_string(), value),
                Span::styled("       Management    ", label),
                Span::styled(mgmt_str, value),
            ]),
        ];

        // ── Section: DHCP Server ──
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " DHCP Server",
            Style::default().fg(theme::ELECTRIC_PURPLE).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            " ─────────────────────────────────────────",
            Style::default().fg(theme::BORDER_GRAY),
        )));
        lines.push(Line::from(vec![
            Span::styled("  DHCP          ", label),
            Span::styled(dhcp_status.to_string(), dhcp_style),
            if let Some(ref lease) = lease_str {
                Span::styled(format!("       Lease Time    {lease}"), label)
            } else {
                Span::raw("")
            },
        ]));

        if let Some(ref range) = dhcp_range {
            lines.push(Line::from(vec![
                Span::styled("  Range         ", label),
                Span::styled(range.clone(), value),
            ]));
        }

        if let Some(ref dns) = dns_str {
            lines.push(Line::from(vec![
                Span::styled("  DNS           ", label),
                Span::styled(dns.clone(), value),
            ]));
        }

        // ── Section: Features ──
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Features",
            Style::default().fg(theme::ELECTRIC_PURPLE).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            " ─────────────────────────────────────────",
            Style::default().fg(theme::BORDER_GRAY),
        )));
        lines.push(Line::from(vec![
            Span::styled("  Internet      ", label),
            bool_span(network.internet_access_enabled, "Enabled", "Disabled"),
            Span::styled("       Isolation     ", label),
            bool_span(network.isolation_enabled, "Enabled", "Disabled"),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  mDNS Fwd      ", label),
            bool_span(network.mdns_forwarding_enabled, "Enabled", "Disabled"),
            Span::styled("       IPv6          ", label),
            Span::styled(ipv6_str, ipv6_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Cellular BU   ", label),
            bool_span(network.cellular_backup_enabled, "Enabled", "Disabled"),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }

    // ── Edit overlay rendering ─────────────────────────────────

    #[allow(clippy::unused_self)]
    fn render_edit_overlay(&self, frame: &mut Frame, area: Rect, edit: &NetworkEditState) {
        // Center the overlay (40 wide, field_count + 6 tall)
        let overlay_w = 44u16.min(area.width.saturating_sub(4));
        let overlay_h = (NetworkEditState::FIELD_COUNT as u16 + 6).min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
        let y = area.y + (area.height.saturating_sub(overlay_h)) / 2;
        let overlay_area = Rect::new(x, y, overlay_w, overlay_h);

        frame.render_widget(Clear, overlay_area);

        let block = Block::default()
            .title(" Edit Network ")
            .title_style(Style::default().fg(theme::ELECTRIC_YELLOW).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(theme::ELECTRIC_PURPLE));

        let inner = block.inner(overlay_area);
        frame.render_widget(block, overlay_area);

        let label = Style::default().fg(theme::DIM_WHITE);
        let value_style = Style::default().fg(theme::NEON_CYAN);
        let focused_label = Style::default().fg(theme::ELECTRIC_YELLOW).add_modifier(Modifier::BOLD);
        let enabled_style = Style::default().fg(theme::SUCCESS_GREEN);
        let disabled_style = Style::default().fg(theme::BORDER_GRAY);

        let mut lines = Vec::new();

        for idx in 0..NetworkEditState::FIELD_COUNT {
            let is_focused = idx == edit.field_idx;
            let lbl_style = if is_focused { focused_label } else { label };
            let marker = if is_focused { "▸ " } else { "  " };
            let field_label = NetworkEditState::field_label(idx);
            let field_value = edit.field_value(idx);

            let val_style = if NetworkEditState::is_bool_field(idx) {
                let is_enabled = matches!(field_value.as_str(), "Enabled");
                if is_enabled { enabled_style } else { disabled_style }
            } else {
                value_style
            };

            let cursor = if is_focused && !NetworkEditState::is_bool_field(idx) { "▎" } else { "" };

            lines.push(Line::from(vec![
                Span::styled(marker, lbl_style),
                Span::styled(format!("{field_label:<14}"), lbl_style),
                Span::styled(field_value, val_style),
                Span::styled(cursor, Style::default().fg(theme::ELECTRIC_YELLOW)),
            ]));
        }

        // Hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" Tab", theme::key_hint_key()),
            Span::styled(" next  ", theme::key_hint()),
            Span::styled("Space", theme::key_hint_key()),
            Span::styled(" toggle  ", theme::key_hint()),
            Span::styled("Enter", theme::key_hint_key()),
            Span::styled(" save  ", theme::key_hint()),
            Span::styled("Esc", theme::key_hint_key()),
            Span::styled(" cancel", theme::key_hint()),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn format_lease_time(secs: u64) -> String {
    if secs >= 86400 && secs % 86400 == 0 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 && secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

impl Component for NetworksScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // ── Edit overlay input ──────────────────────────────────
        if self.edit_state.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.edit_state = None;
                    return Ok(None);
                }
                KeyCode::Enter => {
                    // Take edit state to avoid borrow conflicts
                    if let Some(edit) = self.edit_state.take() {
                        let req = edit.build_request();
                        if let Some(net) = self.networks.get(self.selected_index()) {
                            let id = net.id.clone();
                            return Ok(Some(Action::NetworkSave(id, Box::new(req))));
                        }
                    }
                    return Ok(None);
                }
                _ => {}
            }
            // Remaining edit keys need mutable access
            if let Some(ref mut edit) = self.edit_state {
                match key.code {
                    KeyCode::Tab | KeyCode::Down => {
                        edit.field_idx = (edit.field_idx + 1) % NetworkEditState::FIELD_COUNT;
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        edit.field_idx = if edit.field_idx == 0 {
                            NetworkEditState::FIELD_COUNT - 1
                        } else {
                            edit.field_idx - 1
                        };
                    }
                    KeyCode::Char(' ') if NetworkEditState::is_bool_field(edit.field_idx) => {
                        edit.toggle_bool();
                    }
                    KeyCode::Char(ch) if !NetworkEditState::is_bool_field(edit.field_idx) => {
                        edit.handle_text_input(ch);
                    }
                    KeyCode::Backspace if !NetworkEditState::is_bool_field(edit.field_idx) => {
                        edit.handle_backspace();
                    }
                    _ => {}
                }
            }
            return Ok(None);
        }

        // ── Normal navigation ───────────────────────────────────
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
            KeyCode::Char('e') => {
                if let Some(net) = self.selected_network().cloned() {
                    self.edit_state = Some(NetworkEditState::from_network(&net));
                    self.detail_open = true;
                }
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
            let chunks = Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)])
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

        // ── Table ───────────────────────────────────────────────
        let header = Row::new(vec![
            Cell::from("Name").style(theme::table_header()),
            Cell::from("VLAN").style(theme::table_header()),
            Cell::from("Gateway").style(theme::table_header()),
            Cell::from("Subnet").style(theme::table_header()),
            Cell::from("DHCP").style(theme::table_header()),
            Cell::from("Type").style(theme::table_header()),
            Cell::from("IPv6").style(theme::table_header()),
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
                    .map_or_else(|| "—".into(), |v| v.to_string());
                let gateway = net
                    .gateway_ip
                    .map_or_else(|| "—".into(), |ip| ip.to_string());
                let subnet = net.subnet.as_deref().unwrap_or("—");
                let dhcp: &str = net
                    .dhcp
                    .as_ref()
                    .map_or("—", |d| if d.enabled { "On" } else { "Off" });
                let mgmt = net
                    .management
                    .as_ref()
                    .map_or_else(|| "—".into(), |m| format!("{m:?}"));
                let ipv6 = if net.ipv6_enabled {
                    net.ipv6_mode
                        .as_ref()
                        .map_or_else(|| "On".into(), |m| format!("{m:?}"))
                } else {
                    "Off".into()
                };

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
                    Cell::from(gateway).style(Style::default().fg(theme::CORAL)),
                    Cell::from(subnet.to_string()).style(Style::default().fg(theme::CORAL)),
                    Cell::from(dhcp),
                    Cell::from(mgmt),
                    Cell::from(ipv6),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Min(14),
            Constraint::Length(6),
            Constraint::Length(16),
            Constraint::Length(18),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(10),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.table_state;
        frame.render_stateful_widget(table, layout[0], &mut state);

        // ── Hints ───────────────────────────────────────────────
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("Enter ", theme::key_hint_key()),
            Span::styled("expand  ", theme::key_hint()),
            Span::styled("e ", theme::key_hint_key()),
            Span::styled("edit  ", theme::key_hint()),
            Span::styled("Esc ", theme::key_hint_key()),
            Span::styled("collapse", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);

        // ── Detail panel ────────────────────────────────────────
        if let Some(detail_area) = detail_area {
            if let Some(network) = self.networks.get(selected_idx) {
                self.render_detail(frame, detail_area, network);
            }
        }

        // ── Edit overlay (rendered on top) ──────────────────────
        if let Some(ref edit) = self.edit_state {
            self.render_edit_overlay(frame, area, edit);
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Networks"
    }
}
