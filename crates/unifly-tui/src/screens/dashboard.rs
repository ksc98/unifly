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

use std::cell::Cell;
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
use ratatui::widgets::{
    Axis, Block, BorderType, Borders, Chart, Dataset, GraphType, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState,
};
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

fn fmt_rate_compact(bytes_per_sec: u64) -> String {
    let bits = bytes_per_sec.saturating_mul(8);
    let with_decimal = |value: u64, unit: u64, suffix: &str| -> String {
        let scaled = value.saturating_mul(10) / unit;
        format!("{}.{}{}", scaled / 10, scaled % 10, suffix)
    };

    if bits >= 1_000_000_000 {
        with_decimal(bits, 1_000_000_000, "G")
    } else if bits >= 1_000_000 {
        with_decimal(bits, 1_000_000, "M")
    } else if bits >= 1_000 {
        with_decimal(bits, 1_000, "K")
    } else {
        format!("{bits}b")
    }
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
    /// Dedup: when we last pushed a sample from `HealthUpdated`.
    last_health_sample: Option<Instant>,
    /// Monthly WAN usage: (tx_bytes, rx_bytes).
    monthly_wan: (u64, u64),
    /// Scroll offset for the all-clients panel.
    client_scroll: usize,
    /// Last known visible row count for scroll clamping (Cell for interior mutability in render).
    client_visible_rows: Cell<usize>,
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
            last_health_sample: None,
            monthly_wan: (0, 0),
            client_scroll: 0,
            client_visible_rows: Cell::new(0),
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
        // Keep last 300 samples (~10 min at 2s health poll)
        if self.bandwidth_tx.len() > 300 {
            self.bandwidth_tx.remove(0);
            self.bandwidth_rx.remove(0);
        }
    }

    /// Linearly interpolate data points to create dense fill data for area charts.
    /// `target_density` is the approximate number of output points to generate.
    fn interpolate_fill(data: &[(f64, f64)], target_density: usize) -> Vec<(f64, f64)> {
        if data.len() < 2 {
            return data.to_vec();
        }
        let x_min = data.first().map_or(0.0, |&(x, _)| x);
        let x_max = data.last().map_or(1.0, |&(x, _)| x);
        let x_range = (x_max - x_min).max(1.0);
        let step = x_range / target_density as f64;

        let mut result = Vec::with_capacity(target_density + 1);
        let mut data_idx = 0;

        let mut x = x_min;
        while x <= x_max + step * 0.5 {
            // Advance data_idx to bracket x
            while data_idx + 1 < data.len() && data[data_idx + 1].0 < x {
                data_idx += 1;
            }
            let y = if data_idx + 1 < data.len() {
                let (x0, y0) = data[data_idx];
                let (x1, y1) = data[data_idx + 1];
                let dx = x1 - x0;
                if dx.abs() < f64::EPSILON {
                    y0
                } else {
                    y0 + (y1 - y0) * ((x - x0) / dx)
                }
            } else {
                data[data.len() - 1].1
            };
            result.push((x, y));
            x += step;
        }
        result
    }

    // ── Render Methods ──────────────────────────────────────────────────

    /// Hero panel: WAN traffic chart with area fill and Braille line overlay.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render_traffic_chart(&self, frame: &mut Frame, area: Rect) {
        let current_tx = self.bandwidth_tx.last().map_or(0, |&(_, v)| v as u64);
        let current_rx = self.bandwidth_rx.last().map_or(0, |&(_, v)| v as u64);

        let title = Line::from(vec![
            Span::styled(" Throughput ", theme::title_style()),
            Span::styled("── ", Style::default().fg(theme::BORDER_GRAY)),
            Span::styled(
                format!("TX {} ↑", bytes_fmt::fmt_rate(current_tx)),
                Style::default().fg(theme::ELECTRIC_PURPLE),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("RX {} ↓", bytes_fmt::fmt_rate(current_rx)),
                Style::default().fg(theme::LIGHT_BLUE),
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

        // Scale Y to the max of ALL visible data so nothing clips.
        let y_max_raw = self
            .bandwidth_tx
            .iter()
            .chain(self.bandwidth_rx.iter())
            .map(|&(_, v)| v)
            .fold(0.0_f64, f64::max);
        let y_max = if y_max_raw < 1000.0 {
            10_000.0
        } else {
            y_max_raw * 1.2
        };

        // ── Area fills (HalfBlock bars — rendered first, behind lines) ──
        // Two separate fills: RX (rose) first, then TX (teal) on top.
        // Where RX > TX the rose peeks above the teal; where TX > RX it's all teal.
        // 3× chart width eliminates float→pixel rounding gaps between bars.
        let fill_density = (usize::from(area.width.saturating_sub(8)) * 3).max(120);
        let rx_fill_data = Self::interpolate_fill(&self.bandwidth_rx, fill_density);
        let tx_fill_data = Self::interpolate_fill(&self.bandwidth_tx, fill_density);

        let rx_fill = Dataset::default()
            .marker(Marker::HalfBlock)
            .graph_type(GraphType::Bar)
            .style(Style::default().fg(theme::RX_FILL))
            .data(&rx_fill_data);

        let tx_fill = Dataset::default()
            .marker(Marker::HalfBlock)
            .graph_type(GraphType::Bar)
            .style(Style::default().fg(theme::TX_FILL))
            .data(&tx_fill_data);

        // ── Line edge datasets (Braille — rendered on top for crisp edges) ──

        let tx_line = Dataset::default()
            .name("TX")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme::ELECTRIC_PURPLE))
            .data(&self.bandwidth_tx);

        let rx_line = Dataset::default()
            .name("RX")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme::LIGHT_BLUE))
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

        // RX fill → TX fill → TX line → RX line (later datasets render on top)
        let chart = Chart::new(vec![rx_fill, tx_fill, tx_line, rx_line])
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

    /// Gateway panel (standalone, unused — see `render_gateway_capacity`).
    #[allow(clippy::too_many_lines, dead_code)]
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

    /// Connectivity card (standalone, unused — see `render_udm_info`).
    #[allow(clippy::too_many_lines, dead_code)]
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
        let bar_width = u16::try_from(w.saturating_sub(13).clamp(6, 14)).unwrap_or(8);
        let mut lines = Vec::new();

        let status_color = |sub: &str| -> ratatui::style::Color {
            match self
                .health
                .iter()
                .find(|h| h.subsystem == sub)
                .map(|h| h.status.as_str())
            {
                Some("ok") => theme::SUCCESS_GREEN,
                Some("warn" | "warning") => theme::ELECTRIC_YELLOW,
                Some("error") => theme::ERROR_RED,
                _ => theme::BORDER_GRAY,
            }
        };

        let status_text = |sub: &str| -> &'static str {
            match self
                .health
                .iter()
                .find(|h| h.subsystem == sub)
                .map(|h| h.status.as_str())
            {
                Some("ok") => "ok",
                Some("warn" | "warning") => "warn",
                Some("error") => "err",
                _ => "─",
            }
        };

        let subsystems = ["wan", "www", "wlan", "lan", "vpn"];
        let mut primary = vec![Span::raw(" ")];
        for (i, &sub) in subsystems.iter().take(3).enumerate() {
            if i > 0 {
                primary.push(Span::raw("  "));
            }
            let dot_color = status_color(sub);
            primary.push(Span::styled(
                sub.to_uppercase(),
                Style::default().fg(theme::DIM_WHITE),
            ));
            primary.push(Span::styled(" ●", Style::default().fg(dot_color)));
            primary.push(Span::styled(
                format!(" {}", status_text(sub)),
                Style::default().fg(dot_color),
            ));
        }
        lines.push(Line::from(primary));

        let mut secondary = vec![Span::raw(" ")];
        for (i, &sub) in subsystems.iter().skip(3).enumerate() {
            if i > 0 {
                secondary.push(Span::raw("  "));
            }
            let dot_color = status_color(sub);
            secondary.push(Span::styled(
                sub.to_uppercase(),
                Style::default().fg(theme::DIM_WHITE),
            ));
            secondary.push(Span::styled(" ●", Style::default().fg(dot_color)));
            secondary.push(Span::styled(
                format!(" {}", status_text(sub)),
                Style::default().fg(dot_color),
            ));
        }
        lines.push(Line::from(secondary));
        lines.push(Line::from(" "));

        let wan_link = self.health.iter().find(|h| h.subsystem == "wan");
        let wifi_link = self.health.iter().find(|h| h.subsystem == "wlan");
        let wired_link = self.health.iter().find(|h| h.subsystem == "lan");

        let wan_tx = wan_link.and_then(|h| h.tx_bytes_r).unwrap_or(0);
        let wan_rx = wan_link.and_then(|h| h.rx_bytes_r).unwrap_or(0);
        let wlan_tx = wifi_link.and_then(|h| h.tx_bytes_r).unwrap_or(0);
        let wlan_rx = wifi_link.and_then(|h| h.rx_bytes_r).unwrap_or(0);
        let lan_tx = wired_link.and_then(|h| h.tx_bytes_r).unwrap_or(0);
        let lan_rx = wired_link.and_then(|h| h.rx_bytes_r).unwrap_or(0);

        let link_totals = [
            ("wan", "WAN", wan_tx.saturating_add(wan_rx)),
            ("wlan", "WLAN", wlan_tx.saturating_add(wlan_rx)),
            ("lan", "LAN", lan_tx.saturating_add(lan_rx)),
        ];
        let max_total = link_totals
            .iter()
            .map(|(_, _, total)| *total)
            .max()
            .unwrap_or(0);

        let mut push_link_bar = |sub: &str, label: &str, total: u64| {
            let bar = bytes_fmt::fmt_traffic_bar(total, max_total, bar_width);
            let rate = if total > 0 {
                fmt_rate_compact(total)
            } else {
                "─".to_owned()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {label:<5}"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(bar, Style::default().fg(status_color(sub))),
                Span::raw(" "),
                Span::styled(
                    truncate_text(&rate, content_w.saturating_sub(8 + usize::from(bar_width))),
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        };

        for (sub, label, total) in link_totals {
            push_link_bar(sub, label, total);
        }

        let total_tx = wan_tx.saturating_add(wlan_tx).saturating_add(lan_tx);
        let total_rx = wan_rx.saturating_add(wlan_rx).saturating_add(lan_rx);
        let aggregate = format!(
            "↑{}  ↓{}",
            fmt_rate_compact(total_tx),
            fmt_rate_compact(total_rx)
        );
        lines.push(Line::from(vec![
            Span::styled(" Total", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(
                format!(" {}", truncate_text(&aggregate, content_w)),
                Style::default().fg(theme::LIGHT_BLUE),
            ),
        ]));

        let wlan_ap_count = wifi_link.and_then(|h| h.num_adopted).unwrap_or(0);
        let lan_sw_count = wired_link.and_then(|h| h.num_adopted).unwrap_or(0);
        let wireless_clients = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifly_core::ClientType::Wireless)
            .count();
        let wired_clients = self
            .clients
            .iter()
            .filter(|c| c.client_type == unifly_core::ClientType::Wired)
            .count();

        let infra = truncate_text(
            &format!("AP {wlan_ap_count} · SW {lan_sw_count}"),
            content_w,
        );
        let clients = truncate_text(
            &format!("WiFi {wireless_clients} · Wired {wired_clients}"),
            content_w,
        );
        lines.push(Line::from(vec![
            Span::styled(" Infra", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!(" {infra}"), Style::default().fg(theme::NEON_CYAN)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Cli  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!(" {clients}"), Style::default().fg(theme::NEON_CYAN)),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Capacity card (standalone, unused — see `render_gateway_capacity`).
    #[allow(clippy::cast_possible_truncation, dead_code)]
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

    /// Merged Gateway + Capacity (standalone, unused — see `render_udm_info`).
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation, dead_code)]
    fn render_gateway_capacity(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Gateway ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let cols = Layout::horizontal([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(inner);

        // ── Left column: gateway info ──
        let gateway = self
            .devices
            .iter()
            .find(|d| d.device_type == DeviceType::Gateway);
        let wan_health = self.health.iter().find(|h| h.subsystem == "wan");
        let www_health = self.health.iter().find(|h| h.subsystem == "www");

        let w = usize::from(cols[0].width);

        let isp_name = wan_health
            .and_then(|h| h.extra.get("isp_name").and_then(|v| v.as_str()))
            .or_else(|| {
                wan_health.and_then(|h| h.extra.get("isp_organization").and_then(|v| v.as_str()))
            });
        let gw_version =
            wan_health.and_then(|h| h.extra.get("gw_version").and_then(|v| v.as_str()));
        let latency = www_health.and_then(|h| h.latency);
        let uptime = gateway.and_then(|g| g.stats.uptime_secs);
        let wan_ip = gateway
            .and_then(|g| g.ip)
            .map(|ip| ip.to_string())
            .or_else(|| wan_health.and_then(|h| h.wan_ip.clone()));
        let wan_ipv6 = wan_health
            .and_then(|h| {
                const IPV6_KEYS: &[&str] = &[
                    "wan_ip6", "wan_ip6s", "wan_ipv6", "wan_ipv6s",
                    "ipv6", "ipv6Address", "ipv6_address",
                ];
                for key in IPV6_KEYS {
                    if let Some(ipv6) = h.extra.get(*key).and_then(parse_ipv6_from_value) {
                        return Some(ipv6);
                    }
                }
                h.extra
                    .get("wan_ip")
                    .and_then(parse_ipv6_from_value)
                    .or_else(|| h.wan_ip.as_deref().and_then(parse_ipv6_from_text))
            })
            .or_else(|| gateway.and_then(|g| g.wan_ipv6.clone()));

        let mut left = Vec::new();

        // Header
        if let Some(gw) = gateway {
            let model = gw.model.as_deref().unwrap_or("Gateway");
            let fw = gw_version.or(gw.firmware_version.as_deref()).unwrap_or("─");
            let header = truncate_text(&format!("{model} ({fw})"), w.saturating_sub(4));
            left.push(Line::from(vec![
                Span::styled(" ◈ ", Style::default().fg(theme::ELECTRIC_PURPLE)),
                Span::styled(
                    header,
                    Style::default()
                        .fg(theme::NEON_CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

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

        left.push(kv(
            "WAN",
            &wan_ip.unwrap_or_else(|| "─".into()),
            theme::CORAL,
        ));
        left.push(kv(
            "IPv6",
            wan_ipv6.as_deref().unwrap_or("─"),
            theme::LIGHT_BLUE,
        ));
        if let Some(isp) = isp_name {
            left.push(kv("ISP", isp, theme::DIM_WHITE));
        }

        let lat_str = latency.map_or_else(|| "─".into(), |l| format!("{l:.0}ms"));
        let up_str = uptime.map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
        left.push(Line::from(vec![
            Span::styled(" Lat  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(lat_str, Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  Up ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(up_str, Style::default().fg(theme::NEON_CYAN)),
        ]));

        frame.render_widget(Paragraph::new(left), cols[0]);

        // ── Right column: capacity ──
        let rw = usize::from(cols[1].width);
        let content_w = rw.saturating_sub(7);
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

        let mut right = Vec::new();
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
            right.push(line);
        };

        push_bar("CPU", gateway.and_then(|g| g.stats.cpu_utilization_pct));
        push_bar("MEM", gateway.and_then(|g| g.stats.memory_utilization_pct));

        if let Some(gw) = gateway {
            if let (Some(l1), Some(l5), Some(l15)) = (
                gw.stats.load_average_1m,
                gw.stats.load_average_5m,
                gw.stats.load_average_15m,
            ) {
                let load = truncate_text(&format!("{l1:.2} / {l5:.2} / {l15:.2}"), content_w);
                right.push(Line::from(vec![
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
            &format!("{total_devices} dev · {online} on"),
            content_w,
        );
        let cli_summary = truncate_text(
            &format!("{total_clients} cli · {wireless}w · {wired}e"),
            content_w,
        );

        right.push(Line::from(vec![
            Span::styled(" Dev  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(dev_summary, Style::default().fg(theme::NEON_CYAN)),
        ]));
        right.push(Line::from(vec![
            Span::styled(" Cli  ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(cli_summary, Style::default().fg(theme::NEON_CYAN)),
        ]));

        frame.render_widget(Paragraph::new(right), cols[1]);
    }

    /// UDM Info — 2-column device card. Left: device + network stats. Right: WiFi/APs.
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn render_udm_info(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" UDM Info ", theme::title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let cols = Layout::horizontal([
            Constraint::Percentage(58),
            Constraint::Percentage(42),
        ])
        .split(inner);

        // ── Left column: device info + network ──
        let gateway = self.devices.iter().find(|d| d.device_type == DeviceType::Gateway);
        let wan_health = self.health.iter().find(|h| h.subsystem == "wan");

        let gw_name = wan_health
            .and_then(|h| h.extra.get("gw_name").and_then(|v| v.as_str()))
            .or_else(|| gateway.and_then(|g| g.model.as_deref()))
            .unwrap_or("Gateway");
        let gw_version = wan_health
            .and_then(|h| h.extra.get("gw_version").and_then(|v| v.as_str()))
            .or_else(|| gateway.and_then(|g| g.firmware_version.as_deref()));
        let wan_ip = gateway
            .and_then(|g| g.ip).map(|ip| ip.to_string())
            .or_else(|| wan_health.and_then(|h| h.wan_ip.clone()))
            .unwrap_or_else(|| "─".into());
        let uptime = gateway.and_then(|g| g.stats.uptime_secs);
        let up_str = uptime.map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
        let isp = wan_health
            .and_then(|h| h.extra.get("isp_name").and_then(|v| v.as_str()))
            .or_else(|| wan_health.and_then(|h| h.extra.get("isp_organization").and_then(|v| v.as_str())));
        let availability = wan_health
            .and_then(|h| h.extra.get("uptime_stats"))
            .and_then(|u| u.get("WAN"))
            .and_then(|w| w.get("availability"))
            .and_then(|a| a.as_f64());

        let n_gw = self.devices.iter().filter(|d| d.device_type == DeviceType::Gateway).count();
        let n_sw = self.devices.iter().filter(|d| d.device_type == DeviceType::Switch).count();
        let n_ap = self.devices.iter().filter(|d| d.device_type == DeviceType::AccessPoint).count();
        let n_cli = self.clients.len();

        let lw = usize::from(cols[0].width);

        let mut left = Vec::new();

        // Row 1: ◈ Device name + fleet counts + monthly usage
        let mut row1 = vec![
            Span::styled(" ◈ ", Style::default().fg(theme::ELECTRIC_PURPLE)),
            Span::styled(
                gw_name.to_string(),
                Style::default().fg(theme::NEON_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  󰒍 ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!("{n_gw}"), Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  󰈀 ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!("{n_sw}"), Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  󰤥 ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!("{n_ap}"), Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  󰌘 ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(format!("{n_cli}"), Style::default().fg(theme::ELECTRIC_YELLOW)),
        ];
        let (mtx, mrx) = self.monthly_wan;
        let monthly_total = mtx.saturating_add(mrx);
        if monthly_total > 0 {
            row1.push(Span::styled("  ", Style::default()));
            row1.push(Span::styled(
                bytes_fmt::fmt_bytes_short(monthly_total),
                Style::default().fg(theme::DIM_WHITE),
            ));
            row1.push(Span::styled("/mo", Style::default().fg(theme::BORDER_GRAY)));
        }
        left.push(Line::from(row1));

        // Row 2: WAN IP + Uptime + FW
        let fw_str = gw_version.unwrap_or("─");
        left.push(Line::from(vec![
            Span::styled(format!(" {wan_ip}"), Style::default().fg(theme::CORAL)),
            Span::styled("  Up ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(up_str, Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  FW ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled(fw_str.to_string(), Style::default().fg(theme::DIM_WHITE)),
        ]));

        // Row 3-4: CPU and MEM bars (stacked)
        if let Some(gw) = gateway {
            let cpu = gw.stats.cpu_utilization_pct.unwrap_or(0.0);
            let mem = gw.stats.memory_utilization_pct.unwrap_or(0.0);
            let pct_color = |p: f64| -> ratatui::style::Color {
                if p > 80.0 { theme::ERROR_RED }
                else if p > 50.0 { theme::ELECTRIC_YELLOW }
                else { theme::NEON_CYAN }
            };
            let bar_w = u16::try_from(lw.saturating_sub(14).clamp(6, 24)).unwrap_or(10);
            let (cf, ce) = bytes_fmt::fmt_pct_bar(cpu, bar_w);
            let (mf, me) = bytes_fmt::fmt_pct_bar(mem, bar_w);
            left.push(Line::from(vec![
                Span::styled(" CPU ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(cf, Style::default().fg(pct_color(cpu))),
                Span::styled(ce, Style::default().fg(theme::BORDER_GRAY)),
                Span::styled(format!(" {cpu:>3.0}%"), Style::default().fg(theme::DIM_WHITE)),
            ]));
            left.push(Line::from(vec![
                Span::styled(" MEM ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(mf, Style::default().fg(pct_color(mem))),
                Span::styled(me, Style::default().fg(theme::BORDER_GRAY)),
                Span::styled(format!(" {mem:>3.0}%"), Style::default().fg(theme::DIM_WHITE)),
            ]));
        }

        // Row 4: ISP + availability
        if let Some(isp_name) = isp {
            let avail_str = availability.map_or_else(String::new, |a| format!("  {a:.2}%"));
            let avail_color = match availability {
                Some(a) if a >= 99.9 => theme::SUCCESS_GREEN,
                Some(a) if a >= 99.0 => theme::ELECTRIC_YELLOW,
                Some(_) => theme::ERROR_RED,
                None => theme::BORDER_GRAY,
            };
            left.push(Line::from(vec![
                Span::styled(format!(" {isp_name}"), Style::default().fg(theme::LIGHT_BLUE)),
                Span::styled(avail_str, Style::default().fg(avail_color)),
            ]));
        }

        // Row 5: Throughput
        let wan_tx = wan_health.and_then(|h| h.tx_bytes_r).unwrap_or(0);
        let wan_rx = wan_health.and_then(|h| h.rx_bytes_r).unwrap_or(0);
        left.push(Line::from(vec![
            Span::styled(" ↓ ", Style::default().fg(theme::LIGHT_BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(
                bytes_fmt::fmt_rate(wan_rx),
                Style::default().fg(theme::LIGHT_BLUE).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ↑ ", Style::default().fg(theme::ELECTRIC_PURPLE).add_modifier(Modifier::BOLD)),
            Span::styled(
                bytes_fmt::fmt_rate(wan_tx),
                Style::default().fg(theme::ELECTRIC_PURPLE).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Row 6: Latency monitors
        let monitors = wan_health
            .and_then(|h| h.extra.get("uptime_stats"))
            .and_then(|u| u.get("WAN"))
            .and_then(|w| w.get("monitors"))
            .and_then(|m| m.as_array());
        if let Some(mons) = monitors {
            let mut lat_spans = vec![Span::styled(" Latency ", Style::default().fg(theme::DIM_WHITE))];
            for mon in mons.iter().take(4) {
                let target = mon.get("target").and_then(|t| t.as_str()).unwrap_or("?");
                let lat = mon.get("latency_average").and_then(|l| l.as_u64()).unwrap_or(0);
                // Nerd font icons for known targets, short name for others
                let (short, icon_color): (&str, ratatui::style::Color) = if target.contains("google") {
                    ("󰊭 ", theme::ELECTRIC_YELLOW)
                } else if target.contains("cloudflare") || target == "1.1.1.1" {
                    ("󰸏 ", theme::CORAL)
                } else if target.contains("microsoft") || target.contains("bing") {
                    ("󰨡 ", theme::LIGHT_BLUE)
                } else if target.starts_with(|c: char| c.is_ascii_digit()) {
                    (target, theme::DIM_WHITE)
                } else {
                    let s = target.split('.').next().unwrap_or(target);
                    if s.len() > 8 { (&s[..8], theme::DIM_WHITE) } else { (s, theme::DIM_WHITE) }
                };
                let color = if lat < 20 { theme::SUCCESS_GREEN }
                    else if lat < 50 { theme::ELECTRIC_YELLOW }
                    else { theme::ERROR_RED };
                lat_spans.push(Span::styled(format!(" {short} "), Style::default().fg(icon_color)));
                lat_spans.push(Span::styled(format!("{lat}ms"), Style::default().fg(color)));
            }
            left.push(Line::from(lat_spans));
        }

        // Row 7: Subsystem status dots
        let status_color = |sub: &str| -> ratatui::style::Color {
            match self.health.iter().find(|h| h.subsystem == sub).map(|h| h.status.as_str()) {
                Some("ok") => theme::SUCCESS_GREEN,
                Some("warn" | "warning") => theme::ELECTRIC_YELLOW,
                Some("error") => theme::ERROR_RED,
                _ => theme::BORDER_GRAY,
            }
        };
        let mut status_spans = vec![Span::raw(" ")];
        for (i, sub) in ["wan", "www", "wlan", "lan", "vpn"].iter().enumerate() {
            if i > 0 { status_spans.push(Span::raw(" ")); }
            status_spans.push(Span::styled(sub.to_uppercase(), Style::default().fg(theme::DIM_WHITE)));
            status_spans.push(Span::styled(" ●", Style::default().fg(status_color(sub))));
        }
        left.push(Line::from(status_spans));

        frame.render_widget(Paragraph::new(left), cols[0]);

        // ── Right column: WiFi / APs ──
        let aps: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::AccessPoint)
            .collect();

        let rh = usize::from(cols[1].height);
        let mut right = Vec::new();

        if aps.is_empty() {
            right.push(Line::from(Span::styled(
                " No APs",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        } else {
            let name_w = 14;
            let cli_w = 4;

            right.push(Line::from(vec![
                Span::styled(
                    format!(" {:<name_w$}", "AP"),
                    Style::default().fg(theme::BORDER_GRAY).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:>cli_w$}", "Cli"),
                    Style::default().fg(theme::BORDER_GRAY).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Exp", Style::default().fg(theme::BORDER_GRAY).add_modifier(Modifier::BOLD)),
            ]));

            let mut aps_sorted: Vec<_> = aps.iter().collect();
            aps_sorted.sort_by(|a, b| {
                b.client_count.unwrap_or(0).cmp(&a.client_count.unwrap_or(0))
            });

            for ap in aps_sorted.iter().take(rh.saturating_sub(1)) {
                let ap_name: String = ap.name.as_deref().unwrap_or("AP").chars().take(name_w).collect();
                // Compute client count from live client data (2s refresh) not device data (10s)
                let cli = self.clients.iter().filter(|c| {
                    c.uplink_device_mac.as_ref() == Some(&ap.mac)
                        || c.wireless.as_ref()
                            .and_then(|wl| wl.bssid.as_ref())
                            .is_some_and(|bssid| *bssid == ap.mac)
                }).count();

                let satisfaction: Vec<u8> = self
                    .clients
                    .iter()
                    .filter(|c| {
                        c.uplink_device_mac.as_ref() == Some(&ap.mac)
                            || c.wireless.as_ref()
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
                    if e >= 80 { theme::SUCCESS_GREEN }
                    else if e >= 50 { theme::ELECTRIC_YELLOW }
                    else { theme::ERROR_RED }
                };

                let exp_span = if let Some(exp) = avg_exp {
                    Span::styled(format!("{exp:>4}%"), Style::default().fg(exp_color(exp)))
                } else {
                    Span::styled("    ─", Style::default().fg(theme::BORDER_GRAY))
                };

                right.push(Line::from(vec![
                    Span::styled(
                        format!(" {ap_name:<name_w$}"),
                        Style::default().fg(theme::NEON_CYAN).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{cli:>cli_w$}"),
                        Style::default().fg(theme::ELECTRIC_YELLOW),
                    ),
                    exp_span,
                ]));
            }
        }

        frame.render_widget(Paragraph::new(right), cols[1]);
    }

    /// All Clients panel — scrollable list with live rates.
    #[allow(clippy::cast_possible_truncation)]
    fn render_all_clients(&self, frame: &mut Frame, area: Rect) {
        let total = self.clients.len();
        let title = Line::from(vec![
            Span::styled(" Clients ", theme::title_style()),
            Span::styled(
                format!(" {total} "),
                Style::default().fg(theme::BORDER_GRAY),
            ),
        ]);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let max_rows = usize::from(inner.height);
        self.client_visible_rows.set(max_rows);
        if total == 0 {
            frame.render_widget(
                Paragraph::new("  No clients").style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        // Sort by activity (live bandwidth) descending
        let mut sorted: Vec<_> = self.clients.iter().collect();
        sorted.sort_by(|a, b| {
            let activity = |c: &unifly_core::model::client::Client| {
                c.bandwidth.as_ref().map_or(0, |bw| bw.tx_bytes_per_sec.saturating_add(bw.rx_bytes_per_sec))
            };
            activity(b).cmp(&activity(a))
        });

        let visible: Vec<_> = sorted.iter().skip(self.client_scroll).take(max_rows).collect();

        let cw = usize::from(inner.width);
        let name_w = if cw > 60 { 18 } else { 14 };
        let ip_w = if cw > 60 { 16 } else { 0 };

        let mut lines = Vec::new();
        for client in &visible {
            let name = client
                .name.as_deref().filter(|s| !s.is_empty())
                .or(client.hostname.as_deref().filter(|s| !s.is_empty()))
                .or(client.mac.as_str().get(client.mac.as_str().len().saturating_sub(8)..))
                .unwrap_or("unknown");
            let rx_bps = client.bandwidth.as_ref().map_or(0, |bw| bw.rx_bytes_per_sec);
            let tx_bps = client.bandwidth.as_ref().map_or(0, |bw| bw.tx_bytes_per_sec);
            let activity = fmt_rate_compact(rx_bps.saturating_add(tx_bps));
            let rx_rate = fmt_rate_compact(rx_bps);
            let tx_rate = fmt_rate_compact(tx_bps);
            let display_name: String = name.chars().take(name_w).collect();

            // Type icon: nerd font
            let (type_icon, type_color) = match client.client_type {
                unifly_core::ClientType::Wireless => ("󰤥 ", theme::NEON_CYAN),
                unifly_core::ClientType::Wired => ("󰈀 ", theme::DIM_WHITE),
                unifly_core::ClientType::Vpn => ("󰌘 ", theme::ELECTRIC_PURPLE),
                _ => ("? ", theme::BORDER_GRAY),
            };

            let mut spans = vec![
                Span::styled(type_icon, Style::default().fg(type_color)),
                Span::styled(
                    format!("{display_name:<name_w$}"),
                    Style::default().fg(theme::NEON_CYAN),
                ),
            ];
            if ip_w > 0 {
                let ip_str = client.ip.map_or_else(|| "─".into(), |ip| ip.to_string());
                let ip_display: String = ip_str.chars().take(ip_w).collect();
                spans.push(Span::styled(
                    format!(" {ip_display:<ip_w$}"),
                    Style::default().fg(theme::DIM_WHITE),
                ));
            }
            spans.push(Span::styled(
                format!(" {activity:>6}"),
                Style::default().fg(theme::NEON_CYAN),
            ));
            spans.push(Span::styled(
                format!(" ↓ {rx_rate:<6}"),
                Style::default().fg(theme::LIGHT_BLUE),
            ));
            spans.push(Span::styled(
                format!(" ↑ {tx_rate}"),
                Style::default().fg(theme::ELECTRIC_PURPLE),
            ));
            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), inner);

        // Scrollbar
        if total > max_rows {
            let mut state = ScrollbarState::new(total.saturating_sub(max_rows))
                .position(self.client_scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .thumb_symbol("█"),
                inner,
                &mut state,
            );
        }
    }

    /// Networks panel (standalone, unused — merged into UDM Info).
    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines, dead_code)]
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

    /// WiFi / APs panel (standalone, unused — merged into UDM Info).
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation, dead_code)]
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

    /// Top Clients panel (standalone, unused — see `render_all_clients`).
    #[allow(clippy::cast_possible_truncation, dead_code)]
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
        let recent: Vec<_> = self.events.iter().rev().take(max_rows).collect();

        // Build MAC→name lookups for resolving event references
        let device_names: std::collections::HashMap<String, String> = self
            .devices
            .iter()
            .map(|d| {
                let name = d.name.as_deref().unwrap_or(&d.mac.to_string()).to_string();
                (d.mac.to_string(), name)
            })
            .collect();
        let client_names: std::collections::HashMap<String, String> = self
            .clients
            .iter()
            .map(|c| {
                let name = c.name.as_deref()
                    .or(c.hostname.as_deref())
                    .unwrap_or(&c.mac.to_string())
                    .to_string();
                (c.mac.to_string(), name)
            })
            .collect();

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
            let raw_msg = if evt.message.is_empty() {
                evt.event_type.clone()
            } else {
                let mut msg = evt.message.clone();
                // Resolve device MAC references like "AP[xx:xx:xx:xx:xx:xx]"
                if let Some(ref mac) = evt.device_mac {
                    if let Some(name) = device_names.get(mac.as_str()) {
                        let patterns = [
                            format!("AP[{}]", mac),
                            format!("USW[{}]", mac),
                            format!("UGW[{}]", mac),
                            mac.to_string(),
                        ];
                        for pat in &patterns {
                            if msg.contains(pat.as_str()) {
                                msg = msg.replace(pat.as_str(), name);
                                break;
                            }
                        }
                    }
                }
                // Resolve client MAC references like "User[xx:xx:xx:xx:xx:xx]"
                if let Some(ref mac) = evt.client_mac {
                    if let Some(name) = client_names.get(mac.as_str()) {
                        let patterns = [
                            format!("User[{}]", mac),
                            format!("Guest[{}]", mac),
                            mac.to_string(),
                        ];
                        for pat in &patterns {
                            if msg.contains(pat.as_str()) {
                                msg = msg.replace(pat.as_str(), name);
                                break;
                            }
                        }
                    }
                }
                msg
            };
            let msg: String = raw_msg.chars().take(max_msg_width).collect();
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

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        use crossterm::event::KeyCode;
        let visible = self.client_visible_rows.get().max(1);
        let max_scroll = self.clients.len().saturating_sub(visible);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.client_scroll = (self.client_scroll + 1).min(max_scroll);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.client_scroll = self.client_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.client_scroll = (self.client_scroll + 10).min(max_scroll);
            }
            KeyCode::PageUp => {
                self.client_scroll = self.client_scroll.saturating_sub(10);
            }
            KeyCode::Home => self.client_scroll = 0,
            KeyCode::End => {
                self.client_scroll = max_scroll;
            }
            _ => {}
        }
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                self.last_data_update = Some(Instant::now());
            }
            Action::ClientsUpdated(clients) => {
                if !(clients.is_empty() && !self.clients.is_empty()) {
                    self.clients = Arc::clone(clients);
                }
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
            Action::MonthlyWanUsage(tx, rx) => {
                self.monthly_wan = (*tx, *rx);
            }
            Action::HealthUpdated(health) => {
                self.health = Arc::clone(health);
                self.last_data_update = Some(Instant::now());
                // Deduplicate: only push if >1s since last health sample
                let dominated = self
                    .last_health_sample
                    .is_some_and(|t| t.elapsed().as_millis() < 400);
                if !dominated {
                    if let Some(wan) = self.health.iter().find(|h| h.subsystem == "wan") {
                        let tx = wan.tx_bytes_r.unwrap_or(0);
                        let rx = wan.rx_bytes_r.unwrap_or(0);
                        if tx > 0 || rx > 0 {
                            self.push_bandwidth_sample(tx, rx);
                            self.last_health_sample = Some(Instant::now());
                        }
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

        // 2-row layout: chart | body (left stack + right clients)
        let rows = Layout::vertical([
            Constraint::Length(9), // WAN Traffic Chart
            Constraint::Min(14),  // Body
        ])
        .split(inner);

        self.render_traffic_chart(frame, rows[0]);

        // Body: left (UDM + Events stacked) | right (Clients full height)
        let body = Layout::horizontal([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(rows[1]);

        let left = Layout::vertical([
            Constraint::Length(12), // UDM Info (2-col: device + APs)
            Constraint::Min(6),    // Events fill remaining
        ])
        .split(body[0]);

        self.render_udm_info(frame, left[0]);
        self.render_recent_events(frame, left[1]);
        self.render_all_clients(frame, body[1]);
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
