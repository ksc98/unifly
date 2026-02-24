//! Topology screen — tree-based network hierarchy with rich device info.

use std::collections::HashMap;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use unifly_core::{Client, Device, DeviceType};

use crate::action::Action;
use crate::component::Component;
use crate::theme;
use crate::widgets::bytes_fmt::{fmt_bytes_short, fmt_uptime};
use crate::widgets::signal_bars::signal_span;

// ── Tree node ────────────────────────────────────────────────────────

struct TreeNode {
    device_idx: usize,
    depth: u32,
    is_last_child: bool,
}

/// Sort key for device types: Gateway < Switch < AP < Other.
fn device_type_ord(dt: DeviceType) -> u8 {
    match dt {
        DeviceType::Gateway => 0,
        DeviceType::Switch => 1,
        DeviceType::AccessPoint => 2,
        _ => 3,
    }
}

// ── Screen state ─────────────────────────────────────────────────────

pub struct TopologyScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    scroll_offset: usize,
    selected_idx: usize,
    /// Flat node list in render order (pre-order DFS).
    nodes: Vec<TreeNode>,
    /// Scroll offset for the client list in the right panel.
    client_scroll: usize,
}

impl TopologyScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            scroll_offset: 0,
            selected_idx: 0,
            nodes: Vec::new(),
            client_scroll: 0,
        }
    }

    /// Rebuild the tree from current device list.
    fn rebuild_tree(&mut self) {
        let devices = &self.devices;
        if devices.is_empty() {
            self.nodes.clear();
            return;
        }

        // Map MAC → device index for uplink resolution.
        let mac_to_idx: HashMap<&str, usize> = devices
            .iter()
            .enumerate()
            .map(|(i, d)| (d.mac.as_str(), i))
            .collect();

        // parent_of[child_device_idx] = parent_device_idx
        let mut parent_of: HashMap<usize, usize> = HashMap::new();
        let mut children_of: HashMap<usize, Vec<usize>> = HashMap::new();

        for (i, dev) in devices.iter().enumerate() {
            if let Some(ref uplink_mac) = dev.uplink_device_mac {
                if let Some(&parent_dev_idx) = mac_to_idx.get(uplink_mac.as_str()) {
                    if parent_dev_idx != i {
                        parent_of.insert(i, parent_dev_idx);
                        children_of.entry(parent_dev_idx).or_default().push(i);
                    }
                }
            }
        }

        // Roots = devices with no known parent
        let mut root_device_idxs: Vec<usize> = (0..devices.len())
            .filter(|i| !parent_of.contains_key(i))
            .collect();

        // Fallback: orphan devices (no uplink MAC) attach by type hierarchy
        let first_gateway = devices
            .iter()
            .position(|d| d.device_type == DeviceType::Gateway);
        let first_switch = devices
            .iter()
            .position(|d| d.device_type == DeviceType::Switch);

        let mut orphan_attached: Vec<usize> = Vec::new();
        for &root_idx in &root_device_idxs {
            let dev = &devices[root_idx];
            let attach_to = match dev.device_type {
                DeviceType::Gateway => None,
                DeviceType::Switch => first_gateway,
                DeviceType::AccessPoint => first_switch.or(first_gateway),
                _ => first_switch.or(first_gateway),
            };
            if let Some(parent_idx) = attach_to {
                if parent_idx != root_idx {
                    parent_of.insert(root_idx, parent_idx);
                    children_of.entry(parent_idx).or_default().push(root_idx);
                    orphan_attached.push(root_idx);
                }
            }
        }
        root_device_idxs.retain(|i| !orphan_attached.contains(i));

        // Sort roots by type
        root_device_idxs.sort_by_key(|&i| device_type_ord(devices[i].device_type));

        // Pre-order DFS to build flat node list
        let mut nodes: Vec<TreeNode> = Vec::with_capacity(devices.len());
        let mut stack: Vec<(usize, u32)> = Vec::new();

        for &root_idx in root_device_idxs.iter().rev() {
            stack.push((root_idx, 0));
        }

        let mut visited = vec![false; devices.len()];
        while let Some((dev_idx, depth)) = stack.pop() {
            if visited[dev_idx] {
                continue;
            }
            visited[dev_idx] = true;

            nodes.push(TreeNode {
                device_idx: dev_idx,
                depth,
                is_last_child: false, // computed below
            });

            if let Some(kids) = children_of.get(&dev_idx) {
                let mut sorted_kids = kids.clone();
                sorted_kids.sort_by(|&a, &b| {
                    devices[a]
                        .state
                        .is_online()
                        .cmp(&devices[b].state.is_online())
                        .reverse()
                        .then_with(|| {
                            device_type_ord(devices[a].device_type)
                                .cmp(&device_type_ord(devices[b].device_type))
                        })
                        .then_with(|| {
                            devices[a]
                                .name
                                .as_deref()
                                .unwrap_or("")
                                .cmp(devices[b].name.as_deref().unwrap_or(""))
                        })
                });

                for &kid in sorted_kids.iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
        }

        // Compute is_last_child from the flat list
        let len = nodes.len();
        for i in 0..len {
            let d = nodes[i].depth;
            let mut is_last = true;
            for j in (i + 1)..len {
                if nodes[j].depth == d {
                    is_last = false;
                    break;
                }
                if nodes[j].depth < d {
                    break;
                }
            }
            nodes[i].is_last_child = is_last;
        }

        self.nodes = nodes;

        if self.selected_idx >= self.nodes.len() {
            self.selected_idx = self.nodes.len().saturating_sub(1);
        }
    }

    /// Lines per node in the tree (name line + stats line).
    const NODE_HEIGHT: usize = 2;

    /// Adjust `scroll_offset` so the selected node is visible.
    fn ensure_visible(&mut self, viewport_height: usize) {
        let line_start = self.selected_idx * Self::NODE_HEIGHT;
        let line_end = line_start + Self::NODE_HEIGHT;

        if line_start < self.scroll_offset {
            self.scroll_offset = line_start;
        } else if line_end > self.scroll_offset + viewport_height {
            self.scroll_offset = line_end.saturating_sub(viewport_height);
        }
    }

    /// Get clients connected to a specific device, sorted by bandwidth descending.
    fn clients_for_device(&self, device: &Device) -> Vec<&Arc<Client>> {
        let mut clients: Vec<&Arc<Client>> = self
            .clients
            .iter()
            .filter(|c| c.uplink_device_mac.as_ref() == Some(&device.mac))
            .collect();
        clients.sort_by(|a, b| {
            let bw = |c: &Client| {
                c.bandwidth
                    .as_ref()
                    .map_or(0u64, |bw| bw.tx_bytes_per_sec.saturating_add(bw.rx_bytes_per_sec))
            };
            bw(b).cmp(&bw(a))
        });
        clients
    }

    /// Smooth RGB gradient: cool blue → cyan → green → yellow → hot coral.
    /// Uses log-scale over 0 → 125 MB/s (gigabit).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn bandwidth_color(bytes_per_sec: u64) -> Color {
        if bytes_per_sec == 0 {
            return theme::BORDER_GRAY;
        }

        // Log-scale normalization: 100 B/s (floor) → 125 MB/s (ceiling)
        const FLOOR: f64 = 100.0;
        const CEIL: f64 = 125_000_000.0;
        let clamped = (bytes_per_sec as f64).clamp(FLOOR, CEIL);
        let t = (clamped.ln() - FLOOR.ln()) / (CEIL.ln() - FLOOR.ln()); // 0.0 → 1.0

        // 5-stop gradient: blue → cyan → green → yellow → coral
        let stops: [(f64, u8, u8, u8); 5] = [
            (0.00, 100, 150, 230),
            (0.25, 128, 255, 234),
            (0.50, 80, 250, 123),
            (0.75, 241, 250, 140),
            (1.00, 255, 106, 193),
        ];

        let mut i = 0;
        while i < stops.len() - 2 && t > stops[i + 1].0 {
            i += 1;
        }
        let (t0, r0, g0, b0) = stops[i];
        let (t1, r1, g1, b1) = stops[i + 1];
        let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);

        let lerp =
            |a: u8, b: u8| -> u8 { (a as f64 + (b as f64 - a as f64) * f).round() as u8 };
        Color::Rgb(lerp(r0, r1), lerp(g0, g1), lerp(b0, b1))
    }

    /// Build tree guide prefix spans for a given depth using current guide state.
    /// `mode` controls what to draw at the deepest level:
    ///   - `PrefixMode::Connector` draws ├── or └── based on is_last_child
    ///   - `PrefixMode::Continuation` draws │   or     based on guides
    fn build_prefix<'a>(
        guides: &[bool],
        depth: usize,
        mode: PrefixMode,
        is_last_child: bool,
        guide_style: Style,
    ) -> Vec<Span<'a>> {
        let mut spans = Vec::new();
        let connector_depth = depth.saturating_sub(1);

        match mode {
            PrefixMode::Connector => {
                // Ancestor guide columns (0..d-1)
                for l in 0..connector_depth {
                    let ch = if guides.get(l).copied().unwrap_or(false) {
                        "│   "
                    } else {
                        "    "
                    };
                    spans.push(Span::styled(ch.to_string(), guide_style));
                }
                // Connector at depth d-1
                if depth > 0 {
                    let ch = if is_last_child { "└── " } else { "├── " };
                    spans.push(Span::styled(ch.to_string(), guide_style));
                }
            }
            PrefixMode::Continuation => {
                // All guide columns (0..d)
                for l in 0..depth {
                    let ch = if guides.get(l).copied().unwrap_or(false) {
                        "│   "
                    } else {
                        "    "
                    };
                    spans.push(Span::styled(ch.to_string(), guide_style));
                }
            }
        }
        spans
    }

    #[allow(clippy::too_many_lines, clippy::as_conversions)]
    fn render_right_panel(&self, frame: &mut Frame, area: Rect) {
        if self.selected_idx >= self.nodes.len() {
            return;
        }

        let node = &self.nodes[self.selected_idx];
        let device = &self.devices[node.device_idx];

        let name = device.name.as_deref().unwrap_or("Unknown");
        let device_type_str = format!("{:?}", device.device_type);
        let ip_str = device
            .ip
            .map_or_else(|| "─".into(), |ip| ip.to_string());

        let title = format!(" {name} ");
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 2 || inner.width < 10 {
            return;
        }

        // Device info section
        let model = device.model.as_deref().unwrap_or("─");
        let firmware = device.firmware_version.as_deref().unwrap_or("─");
        let device_state = if device.state.is_online() {
            "Online"
        } else if device.state.is_transitional() {
            "Transitional"
        } else {
            "Offline"
        };
        let state_color = if device.state.is_online() {
            theme::SUCCESS_GREEN
        } else if device.state.is_transitional() {
            theme::ELECTRIC_YELLOW
        } else {
            theme::ERROR_RED
        };

        let uptime_str = device
            .stats
            .uptime_secs
            .map(fmt_uptime)
            .unwrap_or_else(|| "─".into());

        let cpu_str = device
            .stats
            .cpu_utilization_pct
            .map(|c| format!("{c:.0}%"))
            .unwrap_or_else(|| "─".into());
        let mem_str = device
            .stats
            .memory_utilization_pct
            .map(|m| format!("{m:.0}%"))
            .unwrap_or_else(|| "─".into());

        let cpu_color = device
            .stats
            .cpu_utilization_pct
            .map_or(theme::DIM_WHITE, |c| {
                if c >= 80.0 {
                    theme::ERROR_RED
                } else if c >= 50.0 {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::SUCCESS_GREEN
                }
            });
        let mem_color = device
            .stats
            .memory_utilization_pct
            .map_or(theme::DIM_WHITE, |m| {
                if m >= 80.0 {
                    theme::ERROR_RED
                } else if m >= 50.0 {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::SUCCESS_GREEN
                }
            });

        let bw_str = device
            .stats
            .uplink_bandwidth
            .as_ref()
            .map(|bw| {
                format!(
                    "↓{}  ↑{}",
                    fmt_bytes_short(bw.rx_bytes_per_sec),
                    fmt_bytes_short(bw.tx_bytes_per_sec)
                )
            })
            .unwrap_or_else(|| "─".into());

        let label = Style::default().fg(theme::BORDER_GRAY);
        let val = Style::default().fg(theme::NEON_CYAN);

        let mut lines: Vec<Line<'_>> = vec![
            Line::from(vec![
                Span::styled(" Model  ", label),
                Span::styled(model, val),
            ]),
            Line::from(vec![
                Span::styled(" Type   ", label),
                Span::styled(&device_type_str, val),
                Span::styled("  ", label),
                Span::styled(device_state, Style::default().fg(state_color)),
            ]),
            Line::from(vec![
                Span::styled(" IP     ", label),
                Span::styled(&ip_str, val),
            ]),
            Line::from(vec![
                Span::styled(" Uptime ", label),
                Span::styled(&uptime_str, val),
                Span::styled("  FW ", label),
                Span::styled(firmware, Style::default().fg(theme::DIM_WHITE)),
            ]),
            Line::from(vec![
                Span::styled(" CPU ", label),
                Span::styled(&cpu_str, Style::default().fg(cpu_color)),
                Span::styled("  Mem ", label),
                Span::styled(&mem_str, Style::default().fg(mem_color)),
                Span::styled("  BW ", label),
                Span::styled(&bw_str, Style::default().fg(theme::LIGHT_BLUE)),
            ]),
            Line::from(""),
        ];

        // ── Client list section ──
        let device_clients = self.clients_for_device(device);
        let client_count = device_clients.len();
        let header_text = format!(" Clients ({client_count})");

        lines.push(Line::from(vec![Span::styled(
            header_text,
            Style::default()
                .fg(theme::NEON_CYAN)
                .add_modifier(Modifier::BOLD),
        )]));

        if device_clients.is_empty() {
            lines.push(Line::from(Span::styled(
                "   (no clients connected)",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        } else {
            let name_col = 20_usize.min(inner.width as usize / 2);

            for client in device_clients.iter().skip(self.client_scroll) {
                let total_bw = client.bandwidth.as_ref().map_or(0u64, |bw| {
                    bw.tx_bytes_per_sec.saturating_add(bw.rx_bytes_per_sec)
                });

                let color = Self::bandwidth_color(total_bw);

                let client_name = client
                    .name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(client.hostname.as_deref().filter(|s| !s.is_empty()))
                    .unwrap_or(client.mac.as_str());

                let truncated: String = if client_name.chars().count() > name_col {
                    let mut s: String = client_name.chars().take(name_col - 1).collect();
                    s.push('…');
                    s
                } else {
                    client_name.to_string()
                };

                let pad =
                    " ".repeat(name_col.saturating_sub(truncated.chars().count()) + 1);

                let bw_label = if total_bw > 0 {
                    let rx = client
                        .bandwidth
                        .as_ref()
                        .map(|bw| fmt_bytes_short(bw.rx_bytes_per_sec))
                        .unwrap_or_default();
                    let tx = client
                        .bandwidth
                        .as_ref()
                        .map(|bw| fmt_bytes_short(bw.tx_bytes_per_sec))
                        .unwrap_or_default();
                    format!("↓{rx} ↑{tx}")
                } else {
                    "idle".into()
                };

                let sig = signal_span(
                    client.wireless.as_ref().and_then(|w| w.signal_dbm),
                );

                let spans = vec![
                    Span::styled("  ", Style::default()),
                    sig,
                    Span::styled("  ", Style::default()),
                    Span::styled(truncated, Style::default().fg(color)),
                    Span::raw(pad),
                    Span::styled(bw_label, Style::default().fg(theme::DIM_WHITE)),
                ];

                lines.push(Line::from(spans));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

enum PrefixMode {
    Connector,
    Continuation,
}

impl Component for TopologyScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.nodes.is_empty() {
                    self.selected_idx =
                        (self.selected_idx + 1).min(self.nodes.len() - 1);
                    self.client_scroll = 0;
                    self.ensure_visible(30);
                }
                Ok(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                self.client_scroll = 0;
                self.ensure_visible(30);
                Ok(None)
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.selected_idx = 0;
                self.scroll_offset = 0;
                self.client_scroll = 0;
                Ok(None)
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.selected_idx = self.nodes.len().saturating_sub(1);
                self.client_scroll = 0;
                self.ensure_visible(30);
                Ok(None)
            }
            KeyCode::Char('r') => {
                self.selected_idx = 0;
                self.scroll_offset = 0;
                self.client_scroll = 0;
                Ok(Some(Action::TopologyReset))
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                self.rebuild_tree();
            }
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
            }
            _ => {}
        }
        Ok(None)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(format!(" Topology  ·  {} devices ", self.devices.len()))
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

        if inner.height < 3 || inner.width < 20 {
            return;
        }

        // Always split: tree on left, device+clients panel on right
        let chunks =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(inner);
        let tree_area = chunks[0];
        let right_area = chunks[1];

        let content_area = Rect {
            x: tree_area.x,
            y: tree_area.y,
            width: tree_area.width,
            height: tree_area.height.saturating_sub(1),
        };
        let hints_area = Rect {
            x: tree_area.x,
            y: tree_area.y + tree_area.height.saturating_sub(1),
            width: tree_area.width,
            height: 1,
        };

        if self.nodes.is_empty() {
            let empty = Paragraph::new(Line::from(Span::styled(
                "No devices found",
                Style::default().fg(theme::BORDER_GRAY),
            )));
            frame.render_widget(empty, content_area);
        } else {
            let mut lines: Vec<Line<'_>> = Vec::new();
            let guide_style = Style::default().fg(theme::BORDER_GRAY);

            // Track which depth levels have more siblings coming
            let mut guides: Vec<bool> = Vec::new();

            for (node_idx, node) in self.nodes.iter().enumerate() {
                let dev = &self.devices[node.device_idx];
                let d = node.depth as usize;
                let is_selected = node_idx == self.selected_idx;

                // Update guide state
                while guides.len() < d {
                    guides.push(false);
                }
                guides.truncate(d);
                if d > 0 {
                    // At parent's column: will there be more siblings after this node?
                    if guides.len() < d {
                        guides.resize(d, false);
                    }
                    guides[d - 1] = !node.is_last_child;
                }

                let type_color = match dev.device_type {
                    DeviceType::Gateway => theme::CORAL,
                    DeviceType::Switch => theme::NEON_CYAN,
                    DeviceType::AccessPoint => theme::ELECTRIC_PURPLE,
                    _ => theme::DIM_WHITE,
                };

                let status_dot = if dev.state.is_online() {
                    "●"
                } else {
                    "○"
                };
                let status_color = if dev.state.is_online() {
                    theme::SUCCESS_GREEN
                } else if dev.state.is_transitional() {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::ERROR_RED
                };

                let name = dev.name.as_deref().unwrap_or("Unknown");

                // ── Line 1: tree connector + status dot + device name ──
                let mut name_spans = Self::build_prefix(
                    &guides,
                    d,
                    PrefixMode::Connector,
                    node.is_last_child,
                    guide_style,
                );

                // Selection indicator
                if is_selected {
                    name_spans.push(Span::styled(
                        "▸ ",
                        Style::default()
                            .fg(theme::ELECTRIC_PURPLE)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    name_spans.push(Span::raw("  "));
                }

                name_spans.push(Span::styled(
                    status_dot,
                    Style::default().fg(status_color),
                ));
                name_spans.push(Span::styled(
                    format!(" {name}"),
                    Style::default()
                        .fg(type_color)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));

                // Client count badge
                if let Some(count) = dev.client_count {
                    name_spans.push(Span::styled(
                        format!("  ({count})"),
                        Style::default().fg(theme::BORDER_GRAY),
                    ));
                }

                lines.push(Line::from(name_spans));

                // ── Line 2: tree continuation + compact stats ──
                let mut stats_spans = Self::build_prefix(
                    &guides,
                    d,
                    PrefixMode::Continuation,
                    node.is_last_child,
                    guide_style,
                );

                // Indent to align with name (past the "▸ ● " prefix)
                stats_spans.push(Span::raw("    "));

                let ip_str =
                    dev.ip.map(|ip| ip.to_string()).unwrap_or_default();
                let model_str = dev.model.as_deref().unwrap_or("");
                let cpu_str = dev
                    .stats
                    .cpu_utilization_pct
                    .map(|c| format!("{c:.0}%"))
                    .unwrap_or_else(|| "--".into());
                let mem_str = dev
                    .stats
                    .memory_utilization_pct
                    .map(|m| format!("{m:.0}%"))
                    .unwrap_or_else(|| "--".into());
                let uptime_str =
                    dev.stats.uptime_secs.map(fmt_uptime).unwrap_or_default();

                let cpu_color =
                    dev.stats
                        .cpu_utilization_pct
                        .map_or(theme::DIM_WHITE, |c| {
                            if c >= 80.0 {
                                theme::ERROR_RED
                            } else if c >= 50.0 {
                                theme::ELECTRIC_YELLOW
                            } else {
                                theme::SUCCESS_GREEN
                            }
                        });
                let mem_color = dev
                    .stats
                    .memory_utilization_pct
                    .map_or(theme::DIM_WHITE, |m| {
                        if m >= 80.0 {
                            theme::ERROR_RED
                        } else if m >= 50.0 {
                            theme::ELECTRIC_YELLOW
                        } else {
                            theme::SUCCESS_GREEN
                        }
                    });

                let dim = Style::default().fg(theme::DIM_WHITE);

                if !ip_str.is_empty() {
                    stats_spans.push(Span::styled(ip_str.clone(), dim));
                    stats_spans.push(Span::raw("  "));
                }
                if !model_str.is_empty() {
                    stats_spans
                        .push(Span::styled(model_str.to_string(), dim));
                    stats_spans.push(Span::raw("  "));
                }
                stats_spans.push(Span::styled(
                    format!("C:{cpu_str}"),
                    Style::default().fg(cpu_color),
                ));
                stats_spans.push(Span::raw(" "));
                stats_spans.push(Span::styled(
                    format!("M:{mem_str}"),
                    Style::default().fg(mem_color),
                ));
                if !uptime_str.is_empty() {
                    stats_spans.push(Span::raw("  "));
                    stats_spans
                        .push(Span::styled(uptime_str.to_string(), dim));
                }

                if let Some(ref bw) = dev.stats.uplink_bandwidth {
                    let rx = fmt_bytes_short(bw.rx_bytes_per_sec);
                    let tx = fmt_bytes_short(bw.tx_bytes_per_sec);
                    let total = bw.rx_bytes_per_sec.saturating_add(bw.tx_bytes_per_sec);
                    let bw_color = Self::bandwidth_color(total);
                    stats_spans.push(Span::raw("  "));
                    stats_spans.push(Span::styled(
                        format!("↓{rx} ↑{tx}"),
                        Style::default().fg(bw_color),
                    ));
                }

                lines.push(Line::from(stats_spans));
            }

            let viewport_h = content_area.height as usize;
            let scroll = self
                .scroll_offset
                .min(lines.len().saturating_sub(viewport_h));
            let visible: Vec<Line<'_>> =
                lines.into_iter().skip(scroll).take(viewport_h).collect();

            frame.render_widget(Paragraph::new(visible), content_area);
        }

        // Hints bar
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("g/G ", theme::key_hint_key()),
            Span::styled("top/bottom  ", theme::key_hint()),
            Span::styled("r ", theme::key_hint_key()),
            Span::styled("reset", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), hints_area);

        // Always render the right panel with device info + client list
        self.render_right_panel(frame, right_area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Topo"
    }
}
