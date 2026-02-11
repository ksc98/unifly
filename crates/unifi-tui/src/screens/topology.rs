//! Topology screen — canvas-based network graph (spec §2.6).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Rectangle};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::{Device, DeviceState, DeviceType};

use crate::action::Action;
use crate::component::Component;
use crate::theme;

/// A positioned device node on the topology canvas.
struct TopoNode {
    label: String,
    ip: String,
    device_type: DeviceType,
    state: DeviceState,
    client_count: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub struct TopologyScreen {
    focused: bool,
    devices: Arc<Vec<Arc<Device>>>,
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
}

impl TopologyScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }

    /// Build positioned topology nodes from devices using a simple tree layout.
    fn build_nodes(&self) -> Vec<TopoNode> {
        let mut nodes = Vec::new();

        // Separate by type
        let gateways: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::Gateway)
            .collect();
        let switches: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::Switch)
            .collect();
        let aps: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::AccessPoint)
            .collect();
        let others: Vec<_> = self
            .devices
            .iter()
            .filter(|d| {
                d.device_type != DeviceType::Gateway
                    && d.device_type != DeviceType::Switch
                    && d.device_type != DeviceType::AccessPoint
            })
            .collect();

        // Level 0: Gateways at top center
        let gw_total = gateways.len().max(1);
        for (i, gw) in gateways.iter().enumerate() {
            let x = 50.0 - (gw_total as f64 * 10.0) / 2.0 + i as f64 * 10.0;
            nodes.push(TopoNode {
                label: gw.name.clone().unwrap_or_else(|| "Gateway".into()),
                ip: gw.ip.map(|ip| ip.to_string()).unwrap_or_default(),
                device_type: gw.device_type,
                state: gw.state.clone(),
                client_count: gw.client_count.unwrap_or(0),
                x,
                y: 80.0,
                width: 16.0,
                height: 5.0,
            });
        }

        // Level 1: Switches
        let sw_total = switches.len().max(1);
        for (i, sw) in switches.iter().enumerate() {
            let spacing = 80.0 / sw_total as f64;
            let x = 10.0 + spacing * i as f64;
            nodes.push(TopoNode {
                label: sw.name.clone().unwrap_or_else(|| "Switch".into()),
                ip: sw.ip.map(|ip| ip.to_string()).unwrap_or_default(),
                device_type: sw.device_type,
                state: sw.state.clone(),
                client_count: sw.client_count.unwrap_or(0),
                x,
                y: 55.0,
                width: 13.0,
                height: 4.0,
            });
        }

        // Level 2: Access Points
        let ap_total = aps.len().max(1);
        for (i, ap) in aps.iter().enumerate() {
            let spacing = 80.0 / ap_total as f64;
            let x = 10.0 + spacing * i as f64;
            nodes.push(TopoNode {
                label: ap.name.clone().unwrap_or_else(|| "AP".into()),
                ip: ap.ip.map(|ip| ip.to_string()).unwrap_or_default(),
                device_type: ap.device_type,
                state: ap.state.clone(),
                client_count: ap.client_count.unwrap_or(0),
                x,
                y: 30.0,
                width: 9.0,
                height: 4.0,
            });
        }

        // Level 3: Others at bottom
        for (i, dev) in others.iter().enumerate() {
            let x = 10.0 + i as f64 * 12.0;
            nodes.push(TopoNode {
                label: dev.name.clone().unwrap_or_else(|| "Device".into()),
                ip: dev.ip.map(|ip| ip.to_string()).unwrap_or_default(),
                device_type: dev.device_type,
                state: dev.state.clone(),
                client_count: dev.client_count.unwrap_or(0),
                x,
                y: 10.0,
                width: 10.0,
                height: 3.0,
            });
        }

        nodes
    }
}

impl Component for TopologyScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            // Pan
            KeyCode::Left => {
                self.pan_x -= 5.0;
                Ok(Some(Action::TopologyPan(-5, 0)))
            }
            KeyCode::Right => {
                self.pan_x += 5.0;
                Ok(Some(Action::TopologyPan(5, 0)))
            }
            KeyCode::Up => {
                self.pan_y += 5.0;
                Ok(Some(Action::TopologyPan(0, 5)))
            }
            KeyCode::Down => {
                self.pan_y -= 5.0;
                Ok(Some(Action::TopologyPan(0, -5)))
            }
            // Zoom
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.zoom = (self.zoom * 1.2).min(5.0);
                Ok(Some(Action::TopologyZoom(self.zoom)))
            }
            KeyCode::Char('-') => {
                self.zoom = (self.zoom / 1.2).max(0.2);
                Ok(Some(Action::TopologyZoom(self.zoom)))
            }
            // Fit
            KeyCode::Char('f') => {
                self.pan_x = 0.0;
                self.pan_y = 0.0;
                self.zoom = 1.0;
                Ok(Some(Action::TopologyFit))
            }
            // Reset
            KeyCode::Char('r') => {
                self.pan_x = 0.0;
                self.pan_y = 0.0;
                self.zoom = 1.0;
                Ok(Some(Action::TopologyReset))
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        if let Action::DevicesUpdated(devices) = action {
            self.devices = Arc::clone(devices);
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let zoom_pct = (self.zoom * 100.0) as u32;
        let title = format!(
            " Topology  ·  Zoom: {zoom_pct}%  Pan: {:.0},{:.0} ",
            self.pan_x, self.pan_y
        );
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

        // Reserve space for hints
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };
        let hints_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };

        let nodes = self.build_nodes();

        // Canvas bounds with zoom and pan
        let x_min = -10.0 / self.zoom + self.pan_x;
        let x_max = 110.0 / self.zoom + self.pan_x;
        let y_min = -10.0 / self.zoom + self.pan_y;
        let y_max = 110.0 / self.zoom + self.pan_y;

        let canvas = Canvas::default()
            .x_bounds([x_min, x_max])
            .y_bounds([y_min, y_max])
            .paint(|ctx: &mut Context<'_>| {
                for node in &nodes {
                    let border_color = match node.device_type {
                        DeviceType::Gateway => theme::CORAL,
                        DeviceType::Switch => theme::NEON_CYAN,
                        DeviceType::AccessPoint => theme::ELECTRIC_PURPLE,
                        _ => theme::DIM_WHITE,
                    };

                    let color = if node.state.is_online() {
                        border_color
                    } else {
                        theme::ERROR_RED
                    };

                    // Draw node rectangle
                    ctx.draw(&Rectangle {
                        x: node.x,
                        y: node.y,
                        width: node.width,
                        height: node.height,
                        color,
                    });

                    // Labels
                    let short_label: String = node.label.chars().take(10).collect();
                    ctx.print(
                        node.x + 1.0,
                        node.y + node.height - 1.5,
                        Span::styled(short_label, Style::default().fg(color)),
                    );

                    if !node.ip.is_empty() {
                        ctx.print(
                            node.x + 1.0,
                            node.y + 0.5,
                            Span::styled(
                                node.ip.clone(),
                                Style::default().fg(theme::DIM_WHITE),
                            ),
                        );
                    }
                }

                // Draw connection lines from gateways to switches
                let gateway_nodes: Vec<_> =
                    nodes.iter().filter(|n| n.device_type == DeviceType::Gateway).collect();
                let switch_nodes: Vec<_> =
                    nodes.iter().filter(|n| n.device_type == DeviceType::Switch).collect();
                let ap_nodes: Vec<_> = nodes
                    .iter()
                    .filter(|n| n.device_type == DeviceType::AccessPoint)
                    .collect();

                for gw in &gateway_nodes {
                    let gw_cx = gw.x + gw.width / 2.0;
                    let gw_bottom = gw.y;
                    for sw in &switch_nodes {
                        let sw_cx = sw.x + sw.width / 2.0;
                        let sw_top = sw.y + sw.height;
                        ctx.draw(&ratatui::widgets::canvas::Line {
                            x1: gw_cx,
                            y1: gw_bottom,
                            x2: sw_cx,
                            y2: sw_top,
                            color: theme::NEON_CYAN,
                        });
                    }
                }

                // Lines from switches to APs
                for sw in &switch_nodes {
                    let sw_cx = sw.x + sw.width / 2.0;
                    let sw_bottom = sw.y;
                    for ap in &ap_nodes {
                        let ap_cx = ap.x + ap.width / 2.0;
                        let ap_top = ap.y + ap.height;
                        ctx.draw(&ratatui::widgets::canvas::Line {
                            x1: sw_cx,
                            y1: sw_bottom,
                            x2: ap_cx,
                            y2: ap_top,
                            color: theme::ELECTRIC_PURPLE,
                        });
                    }
                }
            });

        frame.render_widget(canvas, content_area);

        // Hints
        let hints = Line::from(vec![
            Span::styled("  ←→↑↓ ", theme::key_hint_key()),
            Span::styled("pan  ", theme::key_hint()),
            Span::styled("+/- ", theme::key_hint_key()),
            Span::styled("zoom  ", theme::key_hint()),
            Span::styled("f ", theme::key_hint_key()),
            Span::styled("fit  ", theme::key_hint()),
            Span::styled("r ", theme::key_hint_key()),
            Span::styled("reset", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), hints_area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &str {
        "Topo"
    }
}
