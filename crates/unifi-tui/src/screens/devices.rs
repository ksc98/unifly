//! Devices screen — sortable table with detail expansion (spec §2.2).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState,
};
use ratatui::Frame;
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::{Device, DeviceState};

use crate::action::{Action, DeviceDetailTab};
use crate::component::Component;
use crate::theme;
use crate::widgets::{bytes_fmt, status_indicator, sub_tabs};

pub struct DevicesScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    devices: Arc<Vec<Arc<Device>>>,
    table_state: TableState,
    detail_open: bool,
    detail_tab: DeviceDetailTab,
}

impl DevicesScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            devices: Arc::new(Vec::new()),
            table_state: TableState::default(),
            detail_open: false,
            detail_tab: DeviceDetailTab::default(),
        }
    }

    fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    fn selected_device(&self) -> Option<&Arc<Device>> {
        self.devices.get(self.selected_index())
    }

    fn select(&mut self, idx: usize) {
        let clamped = if self.devices.is_empty() {
            0
        } else {
            idx.min(self.devices.len() - 1)
        };
        self.table_state.select(Some(clamped));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.devices.is_empty() {
            return;
        }
        let current = self.selected_index() as isize;
        let next = (current + delta).clamp(0, self.devices.len() as isize - 1);
        self.select(next as usize);
    }

    /// Render the device detail panel below the table.
    fn render_detail(&self, frame: &mut Frame, area: Rect, device: &Device) {
        let name = device.name.as_deref().unwrap_or("Unknown");
        let model = device.model.as_deref().unwrap_or("─");
        let ip = device.ip.map(|ip| ip.to_string()).unwrap_or_else(|| "─".into());
        let mac = device.mac.to_string();

        let title = format!(" {name}  ·  {model}  ·  {ip}  ·  {mac} ");
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Sub-tab bar
        let tabs_layout = Layout::vertical([
            Constraint::Length(2), // tab bar
            Constraint::Min(1),   // content
            Constraint::Length(1), // hints
        ])
        .split(inner);

        let tab_labels = &["Overview", "Performance", "Radios", "Clients", "Ports"];
        let active_idx = match self.detail_tab {
            DeviceDetailTab::Overview => 0,
            DeviceDetailTab::Performance => 1,
            DeviceDetailTab::Radios => 2,
            DeviceDetailTab::Clients => 3,
            DeviceDetailTab::Ports => 4,
        };
        let tab_line = sub_tabs::render_sub_tabs(tab_labels, active_idx);
        frame.render_widget(
            Paragraph::new(vec![Line::from(""), tab_line]),
            tabs_layout[0],
        );

        // Tab content
        match self.detail_tab {
            DeviceDetailTab::Overview => self.render_overview_tab(frame, tabs_layout[1], device),
            DeviceDetailTab::Performance => {
                self.render_performance_tab(frame, tabs_layout[1], device)
            }
            DeviceDetailTab::Radios => self.render_radios_tab(frame, tabs_layout[1], device),
            DeviceDetailTab::Clients => {
                let text = format!(
                    "  Connected clients: {}",
                    device.client_count.unwrap_or(0)
                );
                frame.render_widget(
                    Paragraph::new(text).style(theme::table_row()),
                    tabs_layout[1],
                );
            }
            DeviceDetailTab::Ports => self.render_ports_tab(frame, tabs_layout[1], device),
        }

        // Key hints
        let hints = Line::from(vec![
            Span::styled("  h/l ", theme::key_hint_key()),
            Span::styled("switch tabs  ", theme::key_hint()),
            Span::styled("R ", theme::key_hint_key()),
            Span::styled("restart  ", theme::key_hint()),
            Span::styled("L ", theme::key_hint_key()),
            Span::styled("locate  ", theme::key_hint()),
            Span::styled("Esc ", theme::key_hint_key()),
            Span::styled("back", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), tabs_layout[2]);
    }

    fn render_overview_tab(&self, frame: &mut Frame, area: Rect, device: &Device) {
        let state_span = status_indicator::status_span(&device.state);
        let state_label = format!("{:?}", device.state);
        let firmware = device
            .firmware_version
            .as_deref()
            .unwrap_or("─");
        let fw_status = if device.firmware_updatable {
            "update available"
        } else {
            "up to date"
        };
        let uptime = device
            .stats
            .uptime_secs
            .map(bytes_fmt::fmt_uptime)
            .unwrap_or_else(|| "─".into());
        let adopted = device
            .adopted_at
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "─".into());

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  State          ", Style::default().fg(theme::DIM_WHITE)),
                state_span,
                Span::styled(format!(" {state_label}"), Style::default().fg(theme::DIM_WHITE)),
                Span::styled("       Adopted     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(adopted, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  Firmware       ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    format!("{firmware} ({fw_status})"),
                    Style::default().fg(theme::NEON_CYAN),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Uptime         ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(uptime, Style::default().fg(theme::NEON_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  MAC            ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(device.mac.to_string(), Style::default().fg(theme::CORAL)),
            ]),
            Line::from(vec![
                Span::styled("  Type           ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(
                    format!("{:?}", device.device_type),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]),
        ];

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_performance_tab(&self, frame: &mut Frame, area: Rect, device: &Device) {
        let cpu = device
            .stats
            .cpu_utilization_pct
            .map(|v| format!("{v:.1}%"))
            .unwrap_or_else(|| "─".into());
        let mem = device
            .stats
            .memory_utilization_pct
            .map(|v| format!("{v:.1}%"))
            .unwrap_or_else(|| "─".into());
        let load = device
            .stats
            .load_average_1m
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "─".into());

        let cpu_color = device
            .stats
            .cpu_utilization_pct
            .map(|v| {
                if v < 50.0 {
                    theme::SUCCESS_GREEN
                } else if v < 75.0 {
                    theme::NEON_CYAN
                } else if v < 90.0 {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::ERROR_RED
                }
            })
            .unwrap_or(theme::DIM_WHITE);

        let mem_color = device
            .stats
            .memory_utilization_pct
            .map(|v| {
                if v < 50.0 {
                    theme::SUCCESS_GREEN
                } else if v < 75.0 {
                    theme::NEON_CYAN
                } else if v < 90.0 {
                    theme::ELECTRIC_YELLOW
                } else {
                    theme::ERROR_RED
                }
            })
            .unwrap_or(theme::DIM_WHITE);

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  CPU     ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(cpu, Style::default().fg(cpu_color)),
            ]),
            Line::from(vec![
                Span::styled("  Memory  ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(mem, Style::default().fg(mem_color)),
            ]),
            Line::from(vec![
                Span::styled("  Load    ", Style::default().fg(theme::DIM_WHITE)),
                Span::styled(load, Style::default().fg(theme::DIM_WHITE)),
            ]),
        ];

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_radios_tab(&self, frame: &mut Frame, area: Rect, device: &Device) {
        let mut lines = vec![Line::from("")];

        if device.radios.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No radio data available",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        } else {
            for radio in &device.radios {
                let freq = format!("{:.1} GHz", radio.frequency_ghz);
                let ch = radio
                    .channel
                    .map(|c| format!("ch {c}"))
                    .unwrap_or_else(|| "─".into());
                let width = radio
                    .channel_width_mhz
                    .map(|w| format!("{w} MHz"))
                    .unwrap_or_else(|| "─".into());

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {freq:<10}"),
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{ch:<8} {width}"),
                        Style::default().fg(theme::DIM_WHITE),
                    ),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_ports_tab(&self, frame: &mut Frame, area: Rect, device: &Device) {
        let mut lines = vec![Line::from("")];

        if device.ports.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No port data available",
                Style::default().fg(theme::BORDER_GRAY),
            )));
        } else {
            // Header
            lines.push(Line::from(Span::styled(
                "  Port  State   Speed      PoE",
                theme::table_header(),
            )));

            for port in &device.ports {
                let idx_str = port.index.to_string();
                let name = port
                    .name
                    .as_deref()
                    .unwrap_or(&idx_str);
                let state_color = match port.state {
                    unifi_core::model::PortState::Up => theme::SUCCESS_GREEN,
                    unifi_core::model::PortState::Down => theme::ERROR_RED,
                    _ => theme::DIM_WHITE,
                };
                let state_str = format!("{:?}", port.state);
                let speed = port
                    .speed_mbps
                    .map(|s| {
                        if s >= 1000 {
                            format!("{}G", s / 1000)
                        } else {
                            format!("{s}M")
                        }
                    })
                    .unwrap_or_else(|| "─".into());
                let poe = port
                    .poe
                    .as_ref()
                    .map(|p| {
                        if p.enabled {
                            "✓"
                        } else {
                            "✗"
                        }
                    })
                    .unwrap_or("─");

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:<6}", name),
                        Style::default().fg(theme::NEON_CYAN),
                    ),
                    Span::styled(format!("{:<8}", state_str), Style::default().fg(state_color)),
                    Span::styled(format!("{:<11}", speed), Style::default().fg(theme::DIM_WHITE)),
                    Span::styled(poe, Style::default().fg(theme::DIM_WHITE)),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }
}

impl Component for DevicesScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if self.detail_open {
            // Detail panel key handling
            return match key.code {
                KeyCode::Esc => {
                    self.detail_open = false;
                    Ok(Some(Action::CloseDetail))
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    self.detail_tab = match self.detail_tab {
                        DeviceDetailTab::Overview => DeviceDetailTab::Ports,
                        DeviceDetailTab::Performance => DeviceDetailTab::Overview,
                        DeviceDetailTab::Radios => DeviceDetailTab::Performance,
                        DeviceDetailTab::Clients => DeviceDetailTab::Radios,
                        DeviceDetailTab::Ports => DeviceDetailTab::Clients,
                    };
                    Ok(None)
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.detail_tab = match self.detail_tab {
                        DeviceDetailTab::Overview => DeviceDetailTab::Performance,
                        DeviceDetailTab::Performance => DeviceDetailTab::Radios,
                        DeviceDetailTab::Radios => DeviceDetailTab::Clients,
                        DeviceDetailTab::Clients => DeviceDetailTab::Ports,
                        DeviceDetailTab::Ports => DeviceDetailTab::Overview,
                    };
                    Ok(None)
                }
                KeyCode::Char('R') => {
                    let id = self.selected_device().map(|d| d.id.clone());
                    if let Some(id) = id {
                        Ok(Some(Action::RequestRestart(id)))
                    } else {
                        Ok(None)
                    }
                }
                KeyCode::Char('L') => {
                    let id = self.selected_device().map(|d| d.id.clone());
                    if let Some(id) = id {
                        Ok(Some(Action::RequestLocate(id)))
                    } else {
                        Ok(None)
                    }
                }
                _ => Ok(None),
            };
        }

        // Table navigation
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
                if !self.devices.is_empty() {
                    self.select(self.devices.len() - 1);
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
                let id = self.selected_device().map(|d| d.id.clone());
                if let Some(id) = id {
                    self.detail_open = true;
                    self.detail_tab = DeviceDetailTab::Overview;
                    Ok(Some(Action::OpenDeviceDetail(id)))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('R') => {
                let id = self.selected_device().map(|d| d.id.clone());
                if let Some(id) = id {
                    Ok(Some(Action::RequestRestart(id)))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('L') => {
                let id = self.selected_device().map(|d| d.id.clone());
                if let Some(id) = id {
                    Ok(Some(Action::RequestLocate(id)))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                // Clamp selection
                if !self.devices.is_empty() && self.selected_index() >= self.devices.len() {
                    self.select(self.devices.len() - 1);
                }
            }
            Action::CloseDetail => {
                self.detail_open = false;
            }
            Action::DeviceDetailTab(tab) => {
                self.detail_tab = *tab;
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let device_count = self.devices.len();
        let title = format!(" Devices ({device_count}) ");
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
            let chunks = Layout::vertical([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(inner);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner, None)
        };

        // Filter/sort header line
        let header_layout = Layout::vertical([
            Constraint::Length(1), // filter line
            Constraint::Min(1),   // table
            Constraint::Length(1), // hints
        ])
        .split(table_area);

        let filter_line = Line::from(vec![
            Span::styled(" Filter: ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled("[all]", Style::default().fg(theme::NEON_CYAN)),
            Span::styled("  Sort: ", Style::default().fg(theme::DIM_WHITE)),
            Span::styled("[name ↑]", Style::default().fg(theme::NEON_CYAN)),
            Span::styled(
                format!("  {:>width$}", format!("{device_count} devices"), width = 20),
                Style::default().fg(theme::DIM_WHITE),
            ),
        ]);
        frame.render_widget(Paragraph::new(filter_line), header_layout[0]);

        // Table headers
        let header = Row::new(vec![
            Cell::from("Status").style(theme::table_header()),
            Cell::from("Name").style(theme::table_header()),
            Cell::from("Model").style(theme::table_header()),
            Cell::from("IP").style(theme::table_header()),
            Cell::from("CPU").style(theme::table_header()),
            Cell::from("Mem").style(theme::table_header()),
            Cell::from("TX/RX").style(theme::table_header()),
            Cell::from("Uptime").style(theme::table_header()),
        ]);

        // Table rows
        let rows: Vec<Row> = self
            .devices
            .iter()
            .enumerate()
            .map(|(i, dev)| {
                let is_selected = i == self.selected_index();
                let prefix = if is_selected { "▸" } else { " " };

                let status = status_indicator::status_char(&dev.state);
                let name = dev.name.as_deref().unwrap_or("Unknown");
                let model = dev.model.as_deref().unwrap_or("─");
                let ip = dev.ip.map(|ip| ip.to_string()).unwrap_or_else(|| "─".into());
                let cpu = dev
                    .stats
                    .cpu_utilization_pct
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "·····".into());
                let mem = dev
                    .stats
                    .memory_utilization_pct
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "·····".into());
                let traffic = dev
                    .stats
                    .uplink_bandwidth
                    .as_ref()
                    .map(|bw| {
                        bytes_fmt::fmt_tx_rx(bw.tx_bytes_per_sec, bw.rx_bytes_per_sec)
                    })
                    .unwrap_or_else(|| "···/···".into());
                let uptime = dev
                    .stats
                    .uptime_secs
                    .map(bytes_fmt::fmt_uptime)
                    .unwrap_or_else(|| "···".into());

                let status_color = match dev.state {
                    DeviceState::Online => theme::SUCCESS_GREEN,
                    DeviceState::Offline
                    | DeviceState::ConnectionInterrupted
                    | DeviceState::Isolated => theme::ERROR_RED,
                    DeviceState::PendingAdoption => theme::ELECTRIC_PURPLE,
                    _ => theme::ELECTRIC_YELLOW,
                };

                let row_style = if is_selected {
                    theme::table_selected()
                } else {
                    theme::table_row()
                };

                Row::new(vec![
                    Cell::from(format!("{prefix}{status}")).style(Style::default().fg(status_color)),
                    Cell::from(name.to_string()).style(
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(model.to_string()),
                    Cell::from(ip).style(Style::default().fg(theme::CORAL)),
                    Cell::from(cpu),
                    Cell::from(mem),
                    Cell::from(traffic),
                    Cell::from(uptime),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Length(3),
            Constraint::Min(14),
            Constraint::Length(12),
            Constraint::Length(15),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.table_state;
        frame.render_stateful_widget(table, header_layout[1], &mut state);

        // Key hints
        let hints = Line::from(vec![
            Span::styled("  j/k ", theme::key_hint_key()),
            Span::styled("navigate  ", theme::key_hint()),
            Span::styled("Enter ", theme::key_hint_key()),
            Span::styled("detail  ", theme::key_hint()),
            Span::styled("R ", theme::key_hint_key()),
            Span::styled("restart  ", theme::key_hint()),
            Span::styled("L ", theme::key_hint_key()),
            Span::styled("locate", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), header_layout[2]);

        // Render detail panel if open
        if let Some(detail_area) = detail_area {
            if let Some(device) = self.selected_device() {
                self.render_detail(frame, detail_area, device);
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
        "Devices"
    }
}
