//! Dashboard screen — btop-level info density with Braille traffic graph.
//!
//! Layout:
//! ┌─ WAN Traffic Chart (Braille line graph) ─────────────────────────────┐
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ ┌─ Gateway ────────┐ ┌─ Connectivity ────┐ ┌─ Capacity ─────────┐ │
//! │ │ WAN/IP/IPv6/ISP   │ │ WAN/WLAN/LAN/VPN  │ │ CPU/MEM/Load/Count │ │
//! │ └───────────────────┘ └───────────────────┘ └───────────────────┘ │
//! │ ┌─ Networks + IPv6 ──┐ ┌─ Top Clients (traffic bars) ───────────┐ │
//! │ └────────────────────┘ └─────────────────────────────────────────┘ │
//! ├─ Recent Events (compact) ────────────────────────────────────────────┤
//! └──────────────────────────────────────────────────────────────────────┘

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, BorderType, Borders, Chart, Dataset, GraphType, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use unifly_core::model::{EventSeverity, Ipv6Mode};
use unifly_core::{Client, Device, DeviceType, Event, HealthSummary, Network};

use crate::action::Action;
use crate::component::Component;
use crate::theme;
use crate::widgets::bytes_fmt;

fn parse_ipv6_from_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        if ip.is_ipv6() {
            return Some(ip.to_string());
        }
    }

    for token in trimmed.split([',', ';', ' ', '\t', '\n']) {
        let cleaned = token.trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | '"' | '\''));
        if cleaned.is_empty() {
            continue;
        }
        if let Ok(ip) = cleaned.parse::<IpAddr>() {
            if ip.is_ipv6() {
                return Some(ip.to_string());
            }
        }
    }

    None
}

fn parse_ipv6_from_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => parse_ipv6_from_text(s),
        serde_json::Value::Array(items) => items.iter().find_map(parse_ipv6_from_value),
        serde_json::Value::Object(obj) => {
            const PRIORITY_KEYS: &[&str] = &[
                "wan_ip6",
                "wan_ip6s",
                "wan_ipv6",
                "ip6",
                "ip6Address",
                "ip6_address",
                "ipv6",
                "ipv6Address",
                "ipv6_address",
                "address",
                "ipAddress",
                "ip_address",
                "ip",
                "value",
            ];

            for key in PRIORITY_KEYS {
                if let Some(ipv6) = obj.get(*key).and_then(parse_ipv6_from_value) {
                    return Some(ipv6);
                }
            }

            obj.values().find_map(parse_ipv6_from_value)
        }
        _ => None,
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    if max_chars == 1 {
        return "…".into();
    }
    let mut out = String::new();
    for ch in value.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

/// Dashboard screen state.
pub struct DashboardScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    networks: Arc<Vec<Arc<Network>>>,
    events: Vec<Arc<Event>>,
    health: Arc<Vec<HealthSummary>>,
    /// Chart data: `(sample_counter, bytes_per_sec)` for the Chart widget.
    bandwidth_tx: Vec<(f64, f64)>,
    bandwidth_rx: Vec<(f64, f64)>,
    /// Track peak rates for chart title.
    peak_tx: u64,
    peak_rx: u64,
    /// Monotonic sample counter — x-axis value.
    sample_counter: f64,
    /// Tracks when we last received a data update (for refresh indicator).
    last_data_update: Option<Instant>,
}

impl DashboardScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            networks: Arc::new(Vec::new()),
            events: Vec::new(),
            health: Arc::new(Vec::new()),
            bandwidth_tx: Vec::new(),
            bandwidth_rx: Vec::new(),
            peak_tx: 0,
            peak_rx: 0,
            sample_counter: 0.0,
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

    /// Record a bandwidth sample into the chart data ring buffer.
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    fn push_bandwidth_sample(&mut self, tx_bps: u64, rx_bps: u64) {
        self.sample_counter += 1.0;
        self.bandwidth_tx.push((self.sample_counter, tx_bps as f64));
        self.bandwidth_rx.push((self.sample_counter, rx_bps as f64));
        self.peak_tx = self.peak_tx.max(tx_bps);
        self.peak_rx = self.peak_rx.max(rx_bps);
        // Keep last 60 samples (~30 min at 30s refresh)
        if self.bandwidth_tx.len() > 60 {
            self.bandwidth_tx.remove(0);
            self.bandwidth_rx.remove(0);
        }
    }

    // ── Render Methods ──────────────────────────────────────────────────

    /// Hero panel: WAN traffic chart with Braille markers.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render_traffic_chart(&self, frame: &mut Frame, area: Rect) {
        let current_tx = self.bandwidth_tx.last().map_or(0, |&(_, v)| v as u64);
        let current_rx = self.bandwidth_rx.last().map_or(0, |&(_, v)| v as u64);

        let title = Line::from(vec![
            Span::styled(" WAN Traffic ", theme::title_style()),
            Span::styled("── ", Style::default().fg(theme::BORDER_GRAY)),
            Span::styled(
                format!("TX {} ↑", bytes_fmt::fmt_rate(current_tx)),
                Style::default().fg(theme::NEON_CYAN),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("RX {} ↓", bytes_fmt::fmt_rate(current_rx)),
                Style::default().fg(theme::CORAL),
            ),
            Span::styled(
                format!(
                    "  Peak {} ",
                    bytes_fmt::fmt_rate(self.peak_rx.max(self.peak_tx))
                ),
                Style::default().fg(theme::BORDER_GRAY),
            ),
        ]);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        if self.bandwidth_tx.is_empty() {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new("  Waiting for data…")
                    .style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        // Compute axis bounds
        let x_min = self.bandwidth_tx.first().map_or(0.0, |&(x, _)| x);
        let x_max = self.sample_counter;

        let y_max_raw = self
            .bandwidth_tx
            .iter()
            .chain(self.bandwidth_rx.iter())
            .map(|&(_, v)| v)
            .fold(0.0_f64, f64::max);
        // Round up to a nice ceiling so the chart doesn't clip
        let y_max = if y_max_raw < 1.0 {
            1000.0
        } else {
            y_max_raw * 1.2
        };

        let tx_dataset = Dataset::default()
            .name("TX")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme::NEON_CYAN))
            .data(&self.bandwidth_tx);

        let rx_dataset = Dataset::default()
            .name("RX")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme::CORAL))
            .data(&self.bandwidth_rx);

        let y_labels = vec![
            Span::styled("0", Style::default().fg(theme::BORDER_GRAY)),
            Span::styled(
                bytes_fmt::fmt_rate_axis(y_max / 2.0),
                Style::default().fg(theme::BORDER_GRAY),
            ),
            Span::styled(
                bytes_fmt::fmt_rate_axis(y_max),
                Style::default().fg(theme::BORDER_GRAY),
            ),
        ];

        let chart = Chart::new(vec![tx_dataset, rx_dataset])
            .block(block)
            .x_axis(
                Axis::default()
                    .bounds([x_min, x_max])
                    .style(Style::default().fg(theme::BORDER_GRAY)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max])
                    .labels(y_labels)
                    .style(Style::default().fg(theme::BORDER_GRAY)),
            );

        frame.render_widget(chart, area);
    }

    /// Gateway panel — WAN connection details with IPv6.
    #[allow(clippy::too_many_lines)]
    fn render_gateway(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Gateway ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);
        let w = usize::from(inner.width);

        let gateway = self
            .devices
            .iter()
            .find(|d| d.device_type == DeviceType::Gateway);
        let wan_health = self.health.iter().find(|h| h.subsystem == "wan");
        let www_health = self.health.iter().find(|h| h.subsystem == "www");

        // Extract from health extra JSON (best-effort)
        let isp_name = wan_health
            .and_then(|h| h.extra.get("isp_name").and_then(|v| v.as_str()))
            .or_else(|| {
                wan_health.and_then(|h| h.extra.get("isp_organization").and_then(|v| v.as_str()))
            });
        let dns = wan_health
            .and_then(|h| h.extra.get("nameservers").and_then(|v| v.as_array()))
            .map(|ns| {
                let servers: Vec<_> = ns.iter().filter_map(|v| v.as_str()).collect();
                let shown = servers.iter().take(2).copied().collect::<Vec<_>>();
                let hidden = servers.len().saturating_sub(shown.len());
                if hidden > 0 {
                    format!("{}, +{hidden}", shown.join(", "))
                } else {
                    shown.join(", ")
                }
            });
        let gw_ip = wan_health
            .and_then(|h| h.extra.get("gateways").and_then(|v| v.as_array()))
            .and_then(|a| a.first().and_then(|v| v.as_str()));
        let wan_ipv6 = wan_health
            .and_then(|h| {
                const IPV6_KEYS: &[&str] = &[
                    "wan_ip6",
                    "wan_ip6s",
                    "wan_ipv6",
                    "wan_ipv6s",
                    "ipv6",
                    "ipv6Address",
                    "ipv6_address",
                ];

                for key in IPV6_KEYS {
                    if let Some(ipv6) = h.extra.get(*key).and_then(parse_ipv6_from_value) {
                        return Some(ipv6);
                    }
                }

                if let Some(ipv6) = h
                    .extra
                    .get("wan_ip")
                    .and_then(parse_ipv6_from_value)
                    .or_else(|| h.wan_ip.as_deref().and_then(parse_ipv6_from_text))
                {
                    return Some(ipv6);
                }

                h.gateways
                    .as_ref()
                    .and_then(|gateways| gateways.iter().find_map(|gw| parse_ipv6_from_text(gw)))
            })
            .or_else(|| gateway.and_then(|g| g.wan_ipv6.clone()));
        let gw_version =
            wan_health.and_then(|h| h.extra.get("gw_version").and_then(|v| v.as_str()));
        let latency = www_health.and_then(|h| h.latency);
        let uptime = gateway.and_then(|g| g.stats.uptime_secs);
        let wan_ip = gateway
            .and_then(|g| g.ip)
            .map(|ip| ip.to_string())
            .or_else(|| wan_health.and_then(|h| h.wan_ip.clone()));

        let mut lines = Vec::new();

        // Header: ◈ Model (firmware)
        if let Some(gw) = gateway {
            let model = gw.model.as_deref().unwrap_or("Gateway");
            let fw = gw_version.or(gw.firmware_version.as_deref()).unwrap_or("─");
            let header = truncate_text(&format!("{model} ({fw})"), w.saturating_sub(4));
            lines.push(Line::from(vec![
                Span::styled(" ◈ ", Style::default().fg(theme::ELECTRIC_PURPLE)),
                Span::styled(
                    header,
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                " No gateway",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        }
        lines.push(Line::from(""));

        // Key-value rows
        let kv = |label: &str, value: &str, color: ratatui::style::Color| -> Line<'static> {
            let shown = truncate_text(value, w.saturating_sub(7));
            Line::from(vec![
                Span::styled(
                    format!(" {label:<5}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(shown, Style::default().fg(color)),
            ])
        };

        lines.push(kv(
            "WAN",
            &wan_ip.unwrap_or_else(|| "─".into()),
            theme::CORAL,
        ));
        lines.push(kv(
            "IPv6",
            wan_ipv6.as_deref().unwrap_or("─"),
            theme::LIGHT_BLUE,
        ));

        if let Some(gw) = gw_ip {
            lines.push(kv("GW", gw, theme::DIM_WHITE));
        }
        if let Some(ref d) = dns {
            lines.push(kv("DNS", d, theme::DIM_WHITE));
        }
        if let Some(isp) = isp_name {
            lines.push(kv("ISP", isp, theme::DIM_WHITE));
        }

        // Latency + Uptime on one line
        let lat_str = latency.map_or_else(|| "─".into(), |l| format!("{l:.0}ms"));
        let up_str = uptime.map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
        lines.push(Line::from(vec![
            Span::styled(" Lat  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(lat_str, Style::default().fg(theme::NEON_CYAN)),
            Span::styled("   Up ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(up_str, Style::default().fg(theme::NEON_CYAN)),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Connectivity card — subsystem state and per-subsystem activity.
    #[allow(clippy::too_many_lines)]
    fn render_system_health(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Connectivity ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let w = usize::from(inner.width);
        let content_w = w.saturating_sub(8);
        let mut lines = Vec::new();

        let subsystems = ["wan", "www", "wlan", "lan", "vpn"];
        let mut primary = vec![Span::raw(" ")];
        for (i, &sub) in subsystems.iter().take(3).enumerate() {
            if i > 0 {
                primary.push(Span::raw("  "));
            }
            let h = self.health.iter().find(|h| h.subsystem == sub);
            let (dot_color, status_text) = match h.map(|h| h.status.as_str()) {
                Some("ok") => (theme::SUCCESS_GREEN, "ok"),
                Some("warn" | "warning") => (theme::ELECTRIC_YELLOW, "warn"),
                Some("error") => (theme::ERROR_RED, "err"),
                _ => (theme::BORDER_GRAY, "─"),
            };
            primary.push(Span::styled(
                sub.to_uppercase(),
                Style::default().fg(theme::DIM_WHITE),
            ));
            primary.push(Span::styled(" ●", Style::default().fg(dot_color)));
            primary.push(Span::styled(
                format!(" {status_text}"),
                Style::default().fg(dot_color),
            ));
        }
        lines.push(Line::from(primary));

        let mut secondary = vec![Span::raw(" ")];
        for (i, &sub) in subsystems.iter().skip(3).enumerate() {
            if i > 0 {
                secondary.push(Span::raw("  "));
            }
            let h = self.health.iter().find(|h| h.subsystem == sub);
            let (dot_color, status_text) = match h.map(|h| h.status.as_str()) {
                Some("ok") => (theme::SUCCESS_GREEN, "ok"),
                Some("warn" | "warning") => (theme::ELECTRIC_YELLOW, "warn"),
                Some("error") => (theme::ERROR_RED, "err"),
                _ => (theme::BORDER_GRAY, "─"),
            };
            secondary.push(Span::styled(
                sub.to_uppercase(),
                Style::default().fg(theme::DIM_WHITE),
            ));
            secondary.push(Span::styled(" ●", Style::default().fg(dot_color)));
            secondary.push(Span::styled(
                format!(" {status_text}"),
                Style::default().fg(dot_color),
            ));
        }
        lines.push(Line::from(secondary));
        lines.push(Line::from(" "));

        let mut push_kv = |label: &str, value: String, value_color: ratatui::style::Color| {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {label:<5}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(
                    truncate_text(&value, content_w),
                    Style::default().fg(value_color),
                ),
            ]));
        };

        if let Some(wan) = self.health.iter().find(|h| h.subsystem == "wan") {
            if let (Some(tx), Some(rx)) = (wan.tx_bytes_r, wan.rx_bytes_r) {
                push_kv(
                    "WAN",
                    format!(
                        "↑ {}  ↓ {}",
                        bytes_fmt::fmt_rate(tx),
                        bytes_fmt::fmt_rate(rx)
                    ),
                    theme::NEON_CYAN,
                );
            }
        }

        for &(sub_name, dev_label) in &[("wlan", "AP"), ("lan", "SW")] {
            if let Some(h) = self.health.iter().find(|h| h.subsystem == sub_name) {
                let label = sub_name.to_uppercase();
                let dev_count = h.num_adopted.unwrap_or(0);
                let cli_count: usize = if sub_name == "wlan" {
                    self.clients
                        .iter()
                        .filter(|c| c.client_type == unifly_core::ClientType::Wireless)
                        .count()
                } else {
                    self.clients
                        .iter()
                        .filter(|c| c.client_type == unifly_core::ClientType::Wired)
                        .count()
                };

                let detail = if let (Some(tx), Some(rx)) = (h.tx_bytes_r, h.rx_bytes_r) {
                    if tx > 0 || rx > 0 {
                        format!(
                            "{dev_count} {dev_label} · {cli_count} clients · ↑ {} ↓ {}",
                            bytes_fmt::fmt_rate(tx),
                            bytes_fmt::fmt_rate(rx)
                        )
                    } else {
                        format!("{dev_count} {dev_label} · {cli_count} clients")
                    }
                } else {
                    format!("{dev_count} {dev_label} · {cli_count} clients")
                };
                push_kv(&label, detail, theme::NEON_CYAN);
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Capacity card — CPU/MEM bars, load averages, and fleet counts.
    #[allow(clippy::cast_possible_truncation)]
    fn render_capacity(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Capacity ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let gateway = self
            .devices
            .iter()
            .find(|d| d.device_type == DeviceType::Gateway);
        let w = usize::from(inner.width);
        let content_w = w.saturating_sub(7);
        let bar_width = u16::try_from(content_w.saturating_sub(9).clamp(6, 18)).unwrap_or(6);

        let pct_bar_color = |pct: f64| -> ratatui::style::Color {
            if pct > 80.0 {
                theme::ERROR_RED
            } else if pct > 50.0 {
                theme::ELECTRIC_YELLOW
            } else {
                theme::NEON_CYAN
            }
        };

        let mut lines = Vec::new();
        let mut push_bar = |label: &str, pct: Option<f64>| {
            let line = if let Some(pct) = pct {
                let (filled, empty) = bytes_fmt::fmt_pct_bar(pct, bar_width);
                Line::from(vec![
                    Span::styled(
                        format!(" {label:<4}"),
                        Style::default().fg(theme::DIM_WHITE),
                    ),
                    Span::styled(filled, Style::default().fg(pct_bar_color(pct))),
                    Span::styled(empty, Style::default().fg(theme::BORDER_GRAY)),
                    Span::styled(
                        format!(" {pct:>5.1}%"),
                        Style::default().fg(theme::DIM_WHITE),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {label:<4}"),
                        Style::default().fg(theme::DIM_WHITE),
                    ),
                    Span::styled("─", Style::default().fg(theme::BORDER_GRAY)),
                ])
            };
            lines.push(line);
        };

        push_bar("CPU", gateway.and_then(|g| g.stats.cpu_utilization_pct));
        push_bar("MEM", gateway.and_then(|g| g.stats.memory_utilization_pct));
        lines.push(Line::from(" "));

        if let Some(gw) = gateway {
            if let (Some(l1), Some(l5), Some(l15)) = (
                gw.stats.load_average_1m,
                gw.stats.load_average_5m,
                gw.stats.load_average_15m,
            ) {
                let load = truncate_text(&format!("{l1:.2} / {l5:.2} / {l15:.2}"), content_w);
                lines.push(Line::from(vec![
                    Span::styled(" Load ", Style::default().fg(theme::DIM_WHITE)),
                    Span::styled(load, Style::default().fg(theme::NEON_CYAN)),
                ]));
            }
        }

        let total_devices = self.devices.len();
        let online = self
            .devices
            .iter()
            .filter(|d| d.state == unifly_core::DeviceState::Online)
            .count();
        let total_clients = self.clients.len();
        let wireless = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifly_core::ClientType::Wireless)
            .count();
        let wired = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifly_core::ClientType::Wired)
            .count();

        let dev_summary = truncate_text(
            &format!("{total_devices} total · {online} online"),
            content_w,
        );
        let cli_summary = truncate_text(
            &format!("{total_clients} total · {wireless} wifi · {wired} wired"),
            content_w,
        );

        lines.push(Line::from(vec![
            Span::styled(" Dev  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(dev_summary, Style::default().fg(theme::NEON_CYAN)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Cli  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(cli_summary, Style::default().fg(theme::NEON_CYAN)),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Networks panel with IPv6 config.
    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn render_networks(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Networks ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.networks.is_empty() {
            frame.render_widget(
                Paragraph::new("  No networks").style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        let mut sorted: Vec<_> = self.networks.iter().collect();
        sorted.sort_by_key(|n| n.vlan_id.unwrap_or(0));

        let max_lines = usize::from(inner.height);
        let mut lines = Vec::new();

        for net in &sorted {
            if lines.len() >= max_lines {
                break;
            }

            let name: String = net.name.chars().take(10).collect();
            let vlan = net.vlan_id.map_or_else(|| "─".into(), |v| format!("{v}"));
            let subnet = net.subnet.as_deref().unwrap_or("─");

            // Count clients on this network
            let client_count = self
                .clients
                .iter()
                .filter(|c| c.vlan.is_some_and(|v| Some(v) == net.vlan_id))
                .count();
            let client_str = if client_count > 0 {
                format!(" {client_count}c")
            } else {
                String::new()
            };

            // Network line: name (cyan), VLAN (coral), subnet (dim), client count
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {name:<8}"),
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{vlan:<3}"), Style::default().fg(theme::CORAL)),
                Span::styled(subnet.to_string(), Style::default().fg(theme::DIM_WHITE)),
                Span::styled(client_str, Style::default().fg(theme::ELECTRIC_YELLOW)),
            ]));

            // IPv6 sub-line (compact)
            if lines.len() < max_lines && net.ipv6_enabled {
                let mode = match net.ipv6_mode {
                    Some(Ipv6Mode::PrefixDelegation) => "PD",
                    Some(Ipv6Mode::Static) => "Static",
                    Some(_) | None => "On",
                };
                let prefix = net.ipv6_prefix.as_deref().unwrap_or("─");
                let mut extras = Vec::new();
                if net.slaac_enabled {
                    extras.push("SLAAC");
                }
                if net.dhcpv6_enabled {
                    extras.push("DHCPv6");
                }
                let extras_str = if extras.is_empty() {
                    String::new()
                } else {
                    format!(" {}", extras.join("+"))
                };

                lines.push(Line::from(vec![
                    Span::styled(" ⬡ ", Style::default().fg(theme::BORDER_GRAY)),
                    Span::styled(
                        format!("{mode} {prefix}{extras_str}"),
                        Style::default().fg(theme::LIGHT_BLUE),
                    ),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// WiFi / APs panel — tabular layout with header, aligned columns.
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn render_wifi_aps(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" WiFi / APs ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let aps: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::AccessPoint)
            .collect();

        if aps.is_empty() {
            frame.render_widget(
                Paragraph::new("  No APs").style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        let max_rows = usize::from(inner.height);
        let w = usize::from(inner.width);
        let mut lines = Vec::new();

        // Check if any AP has radio data for the optional Chan column
        let has_radios = aps.iter().any(|ap| !ap.radios.is_empty());

        // Column layout: " Name     Cli  Exp [Chan]"
        let cli_col = 4_usize; // "  XX"
        let exp_col = 5_usize; // "  XX%"
        let fixed_cols = 1 + cli_col + exp_col; // leading space + numeric cols
        let remaining = w.saturating_sub(fixed_cols);

        let (name_width, chan_width) = if has_radios {
            let nw = remaining.saturating_sub(1).clamp(6, 16);
            let cw = remaining.saturating_sub(nw + 1);
            (nw, cw)
        } else {
            // No channels — give all space to name
            (remaining.clamp(6, 24), 0)
        };

        // ── Header row ──────────────────────────────────────────
        let mut hdr = vec![
            Span::styled(
                format!(" {:<name_width$}", "AP"),
                Style::default()
                    .fg(theme::BORDER_GRAY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>cli_col$}", "Cli"),
                Style::default()
                    .fg(theme::BORDER_GRAY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>exp_col$}", "Exp"),
                Style::default()
                    .fg(theme::BORDER_GRAY)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if chan_width >= 4 {
            hdr.push(Span::styled(
                format!(" {:<chan_width$}", "Chan"),
                Style::default()
                    .fg(theme::BORDER_GRAY)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(hdr));

        // Sort by client count descending
        let mut aps_sorted: Vec<_> = aps.iter().collect();
        aps_sorted.sort_by(|a, b| {
            b.client_count
                .unwrap_or(0)
                .cmp(&a.client_count.unwrap_or(0))
        });

        for ap in &aps_sorted {
            if lines.len() >= max_rows {
                break;
            }

            let ap_name: String = ap
                .name
                .as_deref()
                .unwrap_or("AP")
                .chars()
                .take(name_width)
                .collect();
            let cli = ap.client_count.unwrap_or(0);

            // Channel summary (compact: "5G:44 6G:149")
            let channels: Vec<String> = ap
                .radios
                .iter()
                .map(|r| {
                    let ch = r.channel.map_or_else(|| "─".into(), |c| c.to_string());
                    if r.frequency_ghz >= 5.9 {
                        format!("6G:{ch}")
                    } else if r.frequency_ghz >= 4.9 {
                        format!("5G:{ch}")
                    } else {
                        format!("2G:{ch}")
                    }
                })
                .collect();
            let ch_str: String = channels.join(" ").chars().take(chan_width).collect();

            // Average WiFi experience from connected clients
            let satisfaction: Vec<u8> = self
                .clients
                .iter()
                .filter(|c| {
                    c.uplink_device_mac.as_ref() == Some(&ap.mac)
                        || c.wireless
                            .as_ref()
                            .and_then(|wl| wl.bssid.as_ref())
                            .is_some_and(|bssid| *bssid == ap.mac)
                })
                .filter_map(|c| c.wireless.as_ref()?.satisfaction)
                .collect();
            let avg_exp = if satisfaction.is_empty() {
                None
            } else {
                Some(
                    satisfaction.iter().map(|s| u32::from(*s)).sum::<u32>()
                        / u32::try_from(satisfaction.len()).unwrap_or(1),
                )
            };

            let exp_color = |e: u32| -> ratatui::style::Color {
                if e >= 80 {
                    theme::SUCCESS_GREEN
                } else if e >= 50 {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::ERROR_RED
                }
            };

            let mut spans = vec![
                Span::styled(
                    format!(" {ap_name:<name_width$}"),
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{cli:>cli_col$}"),
                    Style::default().fg(theme::ELECTRIC_YELLOW),
                ),
            ];

            if let Some(exp) = avg_exp {
                spans.push(Span::styled(
                    format!("{exp:>4}%"),
                    Style::default().fg(exp_color(exp)),
                ));
            } else {
                spans.push(Span::styled(
                    format!("{:>exp_col$}", "─"),
                    Style::default().fg(theme::BORDER_GRAY),
                ));
            }

            if chan_width >= 4 {
                spans.push(Span::styled(
                    format!(" {ch_str}"),
                    Style::default().fg(theme::BORDER_GRAY),
                ));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Top Clients panel with proportional traffic bars.
    #[allow(clippy::cast_possible_truncation)]
    fn render_top_clients(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Top Clients ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let max_rows = usize::from(inner.height);
        let mut sorted: Vec<_> = self.clients.iter().collect();
        sorted.sort_by(|a, b| {
            let a_total = a.tx_bytes.unwrap_or(0) + a.rx_bytes.unwrap_or(0);
            let b_total = b.tx_bytes.unwrap_or(0) + b.rx_bytes.unwrap_or(0);
            b_total.cmp(&a_total)
        });

        let visible: Vec<_> = sorted.iter().take(max_rows.min(8)).collect();

        let max_traffic = visible
            .first()
            .map_or(1, |c| c.tx_bytes.unwrap_or(0) + c.rx_bytes.unwrap_or(0))
            .max(1);

        // Layout:  " name  bar  traffic "
        // traffic label is 7 chars max (e.g. " 33.1G"), padding is 3
        let traffic_width = 7u16;
        let padding = 3u16;

        // Dynamic name width: fit the longest visible name up to a cap
        let longest_name = visible
            .iter()
            .map(|c| {
                c.name
                    .as_deref()
                    .or(c.hostname.as_deref())
                    .unwrap_or("unknown")
                    .len()
            })
            .max()
            .unwrap_or(8);
        let name_cap = usize::from(inner.width.saturating_sub(traffic_width + padding + 4));
        let name_width = longest_name.min(name_cap).max(8);

        // Bar gets whatever is left
        let bar_width = inner.width.saturating_sub(
            u16::try_from(name_width).unwrap_or(u16::MAX) + traffic_width + padding + 1,
        );

        let mut lines = Vec::new();
        for client in &visible {
            let name = client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("unknown");
            let total = client.tx_bytes.unwrap_or(0) + client.rx_bytes.unwrap_or(0);
            let traffic = bytes_fmt::fmt_bytes_short(total);
            let bar = bytes_fmt::fmt_traffic_bar(total, max_traffic, bar_width);

            let display_name: String = name.chars().take(name_width).collect();
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {display_name:<name_width$}"),
                    Style::default().fg(theme::NEON_CYAN),
                ),
                Span::styled(bar, Style::default().fg(theme::ELECTRIC_PURPLE)),
                Span::styled(
                    format!(" {traffic:>6}"),
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

    /// Compact Recent Events — two-column when wide enough.
    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn render_recent_events(&self, frame: &mut Frame, area: Rect) {
        let event_count = self.events.len();
        let title = Line::from(vec![Span::styled(" Recent Events ", theme::title_style())]);
        let footer = if event_count > 0 {
            Line::from(vec![Span::styled(
                format!(" ↓ {event_count} event log "),
                Style::default().fg(theme::BORDER_GRAY),
            )])
        } else {
            Line::from("")
        };

        let block = Block::default()
            .title(title)
            .title_bottom(footer)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let max_rows = usize::from(inner.height);
        let recent: Vec<_> = self.events.iter().rev().take(max_rows * 2).collect();
        let wide = inner.width > 80;

        let format_event = |evt: &Event, max_msg_width: usize| -> Vec<Span<'static>> {
            let time_str = evt.timestamp.format("%H:%M").to_string();
            let severity_color = match evt.severity {
                EventSeverity::Error | EventSeverity::Critical => theme::ERROR_RED,
                EventSeverity::Warning => theme::ELECTRIC_YELLOW,
                EventSeverity::Info => theme::NEON_CYAN,
                _ => theme::DIM_WHITE,
            };
            let dot_color = match evt.severity {
                EventSeverity::Error | EventSeverity::Critical => theme::ERROR_RED,
                EventSeverity::Warning => theme::ELECTRIC_YELLOW,
                _ => theme::SUCCESS_GREEN,
            };
            let msg: String = evt.message.chars().take(max_msg_width).collect();
            vec![
                Span::styled(time_str, Style::default().fg(theme::ELECTRIC_YELLOW)),
                Span::styled(" ● ", Style::default().fg(dot_color)),
                Span::styled(msg, Style::default().fg(severity_color)),
            ]
        };

        let mut lines = Vec::new();
        if recent.is_empty() {
            lines.push(Line::from(Span::styled(
                " No events",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        } else if wide {
            // Two events per line
            let col_width = usize::from((inner.width / 2).saturating_sub(10));
            let mut iter = recent.iter();
            for _ in 0..max_rows {
                let Some(left) = iter.next() else { break };
                let mut spans = vec![Span::raw(" ")];
                spans.extend(format_event(left, col_width));
                if let Some(right) = iter.next() {
                    // Pad to align columns
                    let left_msg_len = left.message.chars().take(col_width).count();
                    let padding = col_width.saturating_sub(left_msg_len) + 3;
                    spans.push(Span::raw(" ".repeat(padding)));
                    spans.extend(format_event(right, col_width));
                }
                lines.push(Line::from(spans));
            }
        } else {
            // One event per line
            let msg_width = usize::from(inner.width.saturating_sub(10));
            for evt in recent.iter().take(max_rows) {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(format_event(evt, msg_width));
                lines.push(Line::from(spans));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

impl Component for DashboardScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                self.last_data_update = Some(Instant::now());
                // Extract bandwidth from gateway stats
                if let Some(gw) = self
                    .devices
                    .iter()
                    .find(|d| d.device_type == DeviceType::Gateway)
                {
                    if let Some(ref bw) = gw.stats.uplink_bandwidth {
                        self.push_bandwidth_sample(bw.tx_bytes_per_sec, bw.rx_bytes_per_sec);
                    }
                }
            }
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
            }
            Action::NetworksUpdated(networks) => {
                self.networks = Arc::clone(networks);
            }
            Action::EventReceived(event) => {
                self.events.push(Arc::clone(event));
                if self.events.len() > 100 {
                    self.events.remove(0);
                }
            }
            Action::HealthUpdated(health) => {
                self.health = Arc::clone(health);
                self.last_data_update = Some(Instant::now());
                // Use WAN health bandwidth when device stats lack it
                if let Some(wan) = self.health.iter().find(|h| h.subsystem == "wan") {
                    let tx = wan.tx_bytes_r.unwrap_or(0);
                    let rx = wan.rx_bytes_r.unwrap_or(0);
                    if tx > 0 || rx > 0 {
                        self.push_bandwidth_sample(tx, rx);
                    }
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
            let summary = format!(
                "Devices: {} │ Clients: {}",
                self.devices.len(),
                self.clients.len()
            );
            frame.render_widget(Paragraph::new(summary).style(theme::table_row()), inner);
            return;
        }

        // 4-row dense layout
        let rows = Layout::vertical([
            Constraint::Length(9),  // Row 1: WAN Traffic Chart
            Constraint::Length(11), // Row 2: Gateway | Connectivity | Capacity
            Constraint::Min(8),     // Row 3: Networks | Top Clients
            Constraint::Length(4),  // Row 4: Recent Events
        ])
        .split(inner);

        self.render_traffic_chart(frame, rows[0]);

        let mid_row = Layout::horizontal([
            Constraint::Percentage(30), // Gateway panel
            Constraint::Percentage(35), // Connectivity panel
            Constraint::Percentage(35), // Capacity panel
        ])
        .split(rows[1]);

        self.render_gateway(frame, mid_row[0]);
        self.render_system_health(frame, mid_row[1]);
        self.render_capacity(frame, mid_row[2]);

        let bottom_row = Layout::horizontal([
            Constraint::Percentage(33), // Networks
            Constraint::Percentage(33), // WiFi / APs
            Constraint::Percentage(34), // Top Clients
        ])
        .split(rows[2]);

        self.render_networks(frame, bottom_row[0]);
        self.render_wifi_aps(frame, bottom_row[1]);
        self.render_top_clients(frame, bottom_row[2]);

        self.render_recent_events(frame, rows[3]);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Dashboard"
    }
}
