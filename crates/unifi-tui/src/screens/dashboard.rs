//! Dashboard screen — network health overview, the home screen.
//!
//! Layout (spec §2.1):
//! ┌─ Health ──────┐  ┌─ Quick Stats ──────────────────┐
//! │ Device/client  │  │ Uptime, WAN IP, firmware, etc. │
//! │ status counts  │  └────────────────────────────────┘
//! │               │  ┌─ Bandwidth ─────────────────────┐
//! └───────────────┘  │ TX/RX sparklines + rates        │
//! ┌─ Top Clients ─┐  └────────────────────────────────┘
//! │ by traffic     │  ┌─ Recent Events ────────────────┐
//! │ (top 5-7)     │  │ last 5-7 events                │
//! └───────────────┘  └────────────────────────────────┘

use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Paragraph, Sparkline as RatatuiSparkline,
};
use ratatui::Frame;
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::{Client, Device, DeviceState, Event};
use unifi_core::model::EventSeverity;

use crate::action::Action;
use crate::component::Component;
use crate::theme;
use crate::widgets::bytes_fmt;
use crate::widgets::status_indicator;

/// Dashboard screen state.
pub struct DashboardScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    events: Vec<Arc<Event>>,
    /// Ring buffer of bandwidth samples (tx_bps, rx_bps) for sparklines.
    bandwidth_tx: Vec<u64>,
    bandwidth_rx: Vec<u64>,
    /// Tracks when we last received a data update (for refresh indicator).
    last_data_update: Option<Instant>,
}

impl DashboardScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            events: Vec::new(),
            bandwidth_tx: Vec::new(),
            bandwidth_rx: Vec::new(),
            last_data_update: None,
        }
    }

    /// Format the data age as a human-readable string for the title bar.
    fn refresh_age_str(&self) -> String {
        match self.last_data_update {
            Some(t) => {
                let secs = t.elapsed().as_secs();
                if secs < 5 {
                    "just now".into()
                } else if secs < 60 {
                    format!("{secs}s ago")
                } else {
                    format!("{}m ago", secs / 60)
                }
            }
            None => "no data".into(),
        }
    }

    /// Count devices by state.
    fn device_counts(&self) -> (usize, usize, usize) {
        let mut online = 0usize;
        let mut offline = 0usize;
        let mut transitional = 0usize;

        for d in self.devices.iter() {
            match d.state {
                DeviceState::Online => online += 1,
                DeviceState::Offline
                | DeviceState::ConnectionInterrupted
                | DeviceState::Isolated => offline += 1,
                _ => transitional += 1,
            }
        }

        (online, offline, transitional)
    }

    /// Render the Health panel (top-left).
    fn render_health(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Health ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (online, offline, transitional) = self.device_counts();
        let total_clients = self.clients.len();
        let wireless = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifi_core::ClientType::Wireless)
            .count();
        let wired = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifi_core::ClientType::Wired)
            .count();
        let vpn = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifi_core::ClientType::Vpn)
            .count();

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  ● ", Style::default().fg(theme::SUCCESS_GREEN)),
                Span::styled(
                    format!("{online} Devices Online"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]),
            Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(theme::ERROR_RED)),
                Span::styled(
                    format!("{offline} Device Offline"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]),
            if transitional > 0 {
                Line::from(vec![
                    Span::styled("  ◐ ", Style::default().fg(theme::ELECTRIC_YELLOW)),
                    Span::styled(
                        format!("{transitional} Updating"),
                        Style::default().fg(theme::DIM_WHITE),
                    ),
                ])
            } else {
                Line::from("")
            },
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    format!("  {total_clients} Clients"),
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                format!("    {wireless} WiFi"),
                Style::default().fg(theme::DIM_WHITE),
            )),
            Line::from(Span::styled(
                format!("    {wired} Wired"),
                Style::default().fg(theme::DIM_WHITE),
            )),
            Line::from(Span::styled(
                format!("    {vpn} VPN"),
                Style::default().fg(theme::DIM_WHITE),
            )),
        ];

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Render Quick Stats panel (top-right).
    fn render_quick_stats(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Quick Stats ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Derive stats from devices
        let gateway = self
            .devices
            .iter()
            .find(|d| d.device_type == unifi_core::DeviceType::Gateway);

        let uptime_str = gateway
            .and_then(|g| g.stats.uptime_secs)
            .map(bytes_fmt::fmt_uptime)
            .unwrap_or_else(|| "─".into());

        let wan_ip = gateway
            .and_then(|g| g.ip)
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "─".into());

        let outdated_fw = self
            .devices
            .iter()
            .filter(|d| d.firmware_updatable)
            .count();

        let guests = self.clients.iter().filter(|c| c.is_guest).count();

        let vpn_clients = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifi_core::ClientType::Vpn)
            .count();

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Uptime     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(uptime_str, Style::default().fg(theme::NEON_CYAN)),
                Span::styled("     Firmware  ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    format!("{outdated_fw} outdated"),
                    if outdated_fw > 0 {
                        Style::default().fg(theme::ELECTRIC_YELLOW)
                    } else {
                        Style::default().fg(theme::SUCCESS_GREEN)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("  WAN IP     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(wan_ip, Style::default().fg(theme::CORAL)),
                Span::styled("     Guests    ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    format!("{guests}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  ISP Latency    ─",
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled("     VPN Clients ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    format!("{vpn_clients}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]),
        ];

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Render Bandwidth sparkline panel (mid-right).
    fn render_bandwidth(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Bandwidth ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 4 {
            return;
        }

        let sparkline_layout = Layout::vertical([
            Constraint::Length(1), // TX label
            Constraint::Length(1), // TX sparkline
            Constraint::Length(1), // RX label
            Constraint::Length(1), // RX sparkline
            Constraint::Min(0),   // stats line
        ])
        .split(inner);

        let current_tx = self.bandwidth_tx.last().copied().unwrap_or(0);
        let current_rx = self.bandwidth_rx.last().copied().unwrap_or(0);

        // TX label
        let tx_label = Line::from(vec![
            Span::styled("  TX ", Style::default().fg(theme::NEON_CYAN)),
            Span::styled(
                format!("{} ↑", bytes_fmt::fmt_rate(current_tx)),
                Style::default().fg(theme::NEON_CYAN),
            ),
        ]);
        frame.render_widget(Paragraph::new(tx_label), sparkline_layout[0]);

        // TX sparkline
        let tx_sparkline = RatatuiSparkline::default()
            .data(&self.bandwidth_tx)
            .style(Style::default().fg(theme::NEON_CYAN));
        frame.render_widget(tx_sparkline, sparkline_layout[1]);

        // RX label
        let rx_label = Line::from(vec![
            Span::styled("  RX ", Style::default().fg(theme::CORAL)),
            Span::styled(
                format!("{} ↓", bytes_fmt::fmt_rate(current_rx)),
                Style::default().fg(theme::CORAL),
            ),
        ]);
        frame.render_widget(Paragraph::new(rx_label), sparkline_layout[2]);

        // RX sparkline
        let rx_sparkline = RatatuiSparkline::default()
            .data(&self.bandwidth_rx)
            .style(Style::default().fg(theme::CORAL));
        frame.render_widget(rx_sparkline, sparkline_layout[3]);
    }

    /// Render Top Clients panel (mid-left).
    fn render_top_clients(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Top Clients ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Sort clients by traffic (tx + rx), take top entries that fit
        let max_rows = inner.height.saturating_sub(1) as usize;
        let mut sorted: Vec<_> = self.clients.iter().collect();
        sorted.sort_by(|a, b| {
            let a_total = a.tx_bytes.unwrap_or(0) + a.rx_bytes.unwrap_or(0);
            let b_total = b.tx_bytes.unwrap_or(0) + b.rx_bytes.unwrap_or(0);
            b_total.cmp(&a_total)
        });

        let mut lines = Vec::new();
        for client in sorted.iter().take(max_rows.min(7)) {
            let name = client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("unknown");
            let total = client.tx_bytes.unwrap_or(0) + client.rx_bytes.unwrap_or(0);
            let traffic = bytes_fmt::fmt_bytes_short(total);

            // Truncate name to fit
            let display_name: String = name.chars().take(14).collect();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {display_name:<14}"),
                    Style::default().fg(theme::NEON_CYAN),
                ),
                Span::styled(
                    format!(" {traffic:>7}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No clients",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Render Recent Events panel (bottom-right).
    fn render_recent_events(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Recent Events ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let max_rows = inner.height.saturating_sub(2) as usize;
        let recent: Vec<_> = self.events.iter().rev().take(max_rows.min(7)).collect();

        let mut lines = Vec::new();
        for evt in &recent {
            let time_str = evt.timestamp.format("%H:%M").to_string();
            let severity_color = match evt.severity {
                EventSeverity::Error | EventSeverity::Critical => theme::ERROR_RED,
                EventSeverity::Warning => theme::ELECTRIC_YELLOW,
                EventSeverity::Info => theme::NEON_CYAN,
                _ => theme::DIM_WHITE,
            };

            let msg: String = evt.message.chars().take(inner.width.saturating_sub(22) as usize).collect();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {time_str}  "),
                    Style::default().fg(theme::ELECTRIC_YELLOW),
                ),
                status_indicator::status_span(&DeviceState::Online), // placeholder severity dot
                Span::styled(format!(" {msg}"), Style::default().fg(severity_color)),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No events",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        }

        // Hint at bottom
        if inner.height > (lines.len() as u16 + 2) {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "       ↓ press 7 for full event log",
                theme::key_hint(),
            )));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

impl Component for DashboardScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        // Dashboard has no screen-specific key handlers beyond globals
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                self.last_data_update = Some(Instant::now());
                // Extract bandwidth from gateway stats for sparkline
                if let Some(gw) = self
                    .devices
                    .iter()
                    .find(|d| d.device_type == unifi_core::DeviceType::Gateway)
                {
                    if let Some(ref bw) = gw.stats.uplink_bandwidth {
                        self.bandwidth_tx.push(bw.tx_bytes_per_sec);
                        self.bandwidth_rx.push(bw.rx_bytes_per_sec);
                        // Keep last 60 samples
                        if self.bandwidth_tx.len() > 60 {
                            self.bandwidth_tx.remove(0);
                            self.bandwidth_rx.remove(0);
                        }
                    }
                }
            }
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
            }
            Action::EventReceived(event) => {
                self.events.push(Arc::clone(event));
                // Keep last 100 events for dashboard display
                if self.events.len() > 100 {
                    self.events.remove(0);
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let refresh_str = self.refresh_age_str();
        let title_line = Line::from(vec![
            Span::styled(" UniFi Dashboard ", theme::title_style()),
            Span::styled(
                format!(" [{refresh_str}] "),
                Style::default().fg(theme::BORDER_GRAY),
            ),
        ]);

        let block = Block::default()
            .title(title_line)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 40 || inner.height < 10 {
            // Minimal mode — just show a summary line
            let summary = format!(
                "Devices: {} │ Clients: {}",
                self.devices.len(),
                self.clients.len()
            );
            frame.render_widget(
                Paragraph::new(summary).style(theme::table_row()),
                inner,
            );
            return;
        }

        // Two-column layout: left (25 cols) | right (remaining)
        let left_width = 28u16.min(inner.width / 3);
        let columns = Layout::horizontal([
            Constraint::Length(left_width),
            Constraint::Min(30),
        ])
        .split(inner);

        // Left column: Health panel + Top Clients
        let left = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(columns[0]);

        self.render_health(frame, left[0]);
        self.render_top_clients(frame, left[1]);

        // Right column: Quick Stats + Bandwidth + Recent Events
        let right = Layout::vertical([
            Constraint::Length(6),  // Quick Stats
            Constraint::Length(8),  // Bandwidth
            Constraint::Min(6),    // Recent Events
        ])
        .split(columns[1]);

        self.render_quick_stats(frame, right[0]);
        self.render_bandwidth(frame, right[1]);
        self.render_recent_events(frame, right[2]);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &str {
        "Dashboard"
    }
}
