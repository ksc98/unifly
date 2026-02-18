//! Stats screen — historical charts and analytics (spec §2.8).
//!
//! Layout:
//! ┌─ Statistics ── [1h  24h  7d  30d] ────────────────────────────────┐
//! │ ┌─ WAN Bandwidth ────────────────────────────────────────────────┐│
//! │ │  area fill (HalfBlock) + Braille line overlay                  ││
//! │ │  TX cyan fill + line / RX rose fill + line                     ││
//! │ └────────────────────────────────────────────────────────────────┘│
//! │ ┌─ Client Count ──────────┐ ┌─ Top Applications ────────────────┐│
//! │ │  Braille line chart      │ │  Netflix      ████████   32.1 GB  ││
//! │ │  num_sta over time       │ │  YouTube      ██████     20.4 GB  ││
//! │ └──────────────────────────┘ │  Spotify      ███         8.2 GB  ││
//! │ ┌─ Traffic by Category ───┐ │  Discord      ██          4.1 GB  ││
//! │ │ Streaming  ████████ 65% │ └────────────────────────────────────┘│
//! │ │ Gaming     ████    12%  │                                      │
//! │ │ Social     ███      8%  │                                      │
//! │ └──────────────────────────┘                                      │
//! ├─ h 1h  d 24h  w 7d  m 30d  r refresh ───────────────────────────┤
//! └──────────────────────────────────────────────────────────────────┘

use crate::action::{Action, StatsPeriod};
use crate::component::Component;
use crate::theme;
use crate::widgets::{bytes_fmt, sub_tabs};
use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, BorderType, Borders, Chart, Dataset, GraphType, Paragraph};

pub struct StatsScreen {
    focused: bool,
    period: StatsPeriod,
    /// Bandwidth history: Vec<(timestamp_f64, value)>
    bandwidth_tx: Vec<(f64, f64)>,
    bandwidth_rx: Vec<(f64, f64)>,
    /// Client count history
    client_counts: Vec<(f64, f64)>,
    /// DPI top apps: (name, total_bytes)
    dpi_apps: Vec<(String, u64)>,
    /// DPI top categories: (name, total_bytes)
    dpi_categories: Vec<(String, u64)>,
}

/// Linearly interpolate data points to create dense fill data for area charts.
/// `target_density` is the approximate number of output points to generate.
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
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

impl StatsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            period: StatsPeriod::default(),
            bandwidth_tx: Vec::new(),
            bandwidth_rx: Vec::new(),
            client_counts: Vec::new(),
            dpi_apps: Vec::new(),
            dpi_categories: Vec::new(),
        }
    }

    fn period_index(&self) -> usize {
        match self.period {
            StatsPeriod::OneHour => 0,
            StatsPeriod::TwentyFourHours => 1,
            StatsPeriod::SevenDays => 2,
            StatsPeriod::ThirtyDays => 3,
        }
    }

    /// WAN Bandwidth chart with area fills and Braille line overlays.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render_bandwidth_chart(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" WAN Bandwidth ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        if self.bandwidth_tx.is_empty() && self.bandwidth_rx.is_empty() {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new("  No bandwidth data yet")
                    .style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        // Determine bounds from ALL visible data
        let x_min = self
            .bandwidth_tx
            .first()
            .map_or(0.0, |&(x, _)| x)
            .min(self.bandwidth_rx.first().map_or(f64::MAX, |&(x, _)| x));
        let x_max = self
            .bandwidth_tx
            .last()
            .map_or(1.0, |&(x, _)| x)
            .max(self.bandwidth_rx.last().map_or(0.0, |&(x, _)| x));

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
        let fill_density = (usize::from(area.width.saturating_sub(8)) * 3).max(120);
        let rx_fill_data = interpolate_fill(&self.bandwidth_rx, fill_density);
        let tx_fill_data = interpolate_fill(&self.bandwidth_tx, fill_density);

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
            .style(Style::default().fg(theme::NEON_CYAN))
            .data(&self.bandwidth_tx);

        let rx_line = Dataset::default()
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

    /// Client count chart — Braille line graph of num_sta over time.
    fn render_client_chart(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Client Count ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        if self.client_counts.is_empty() {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new("  No client data yet")
                    .style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        let x_min = self.client_counts.first().map_or(0.0, |(x, _)| *x);
        let x_max = self.client_counts.last().map_or(1.0, |(x, _)| *x);
        let y_max = self
            .client_counts
            .iter()
            .map(|(_, y)| *y)
            .fold(0.0f64, f64::max)
            * 1.1;

        let dataset = Dataset::default()
            .name("Clients")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(theme::ELECTRIC_PURPLE))
            .data(&self.client_counts);

        let chart = Chart::new(vec![dataset])
            .block(block)
            .x_axis(
                Axis::default()
                    .style(Style::default().fg(theme::BORDER_GRAY))
                    .bounds([x_min, x_max]),
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(theme::BORDER_GRAY))
                    .bounds([0.0, y_max.max(1.0)]),
            );

        frame.render_widget(chart, area);
    }

    /// Top Applications — horizontal bars scaled relative to the largest.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render_top_apps(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Top Applications ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.dpi_apps.is_empty() {
            frame.render_widget(
                Paragraph::new("  No DPI data available")
                    .style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        let max_rows = inner.height as usize;
        let bar_budget = inner.width.saturating_sub(26) as usize;
        let colors = theme::CHART_SERIES;

        // Scale bars relative to the largest app
        let max_bytes = self.dpi_apps.first().map_or(1, |(_, b)| *b).max(1);

        let mut lines = Vec::new();
        for (i, (name, bytes)) in self.dpi_apps.iter().enumerate().take(max_rows) {
            let fraction = *bytes as f64 / max_bytes as f64;
            let bar_width = (fraction * bar_budget as f64).round().max(1.0) as usize;
            let bar: String = "█".repeat(bar_width.min(bar_budget));
            let color = colors[i % colors.len()];
            let display_name: String = name.chars().take(14).collect();
            let bytes_str = bytes_fmt::fmt_bytes_short(*bytes);

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {display_name:<14} "),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(bar, Style::default().fg(color)),
                Span::styled(
                    format!(" {:>6}", bytes_str),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Traffic by Category — percentage bars from raw byte totals.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn render_categories(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Traffic by Category ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.dpi_categories.is_empty() {
            frame.render_widget(
                Paragraph::new("  No category data")
                    .style(Style::default().fg(theme::BORDER_GRAY)),
                inner,
            );
            return;
        }

        let max_rows = inner.height as usize;
        let bar_budget = inner.width.saturating_sub(22) as usize;
        let colors = theme::CHART_SERIES;
        let total_bytes: u64 = self.dpi_categories.iter().map(|(_, b)| *b).sum();

        let mut lines = Vec::new();
        for (i, (name, bytes)) in self.dpi_categories.iter().enumerate().take(max_rows) {
            let pct = if total_bytes > 0 {
                *bytes as f64 / total_bytes as f64 * 100.0
            } else {
                0.0
            };
            let bar_width = (pct / 100.0 * bar_budget as f64).round().max(0.0) as usize;
            let bar: String = "█".repeat(bar_width.min(bar_budget));
            let color = colors[i % colors.len()];
            let display_name: String = name.chars().take(12).collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {display_name:<12} "),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(bar, Style::default().fg(color)),
                Span::styled(
                    format!(" {pct:>4.0}%"),
                    Style::default().fg(theme::DIM_WHITE),
                ),
            ]));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

impl Component for StatsScreen {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            // Period selection: h=1h, d=24h, w=7d, m=30d
            KeyCode::Char('h') => {
                self.period = StatsPeriod::OneHour;
                Ok(Some(Action::RequestStats(StatsPeriod::OneHour)))
            }
            KeyCode::Char('d') => {
                self.period = StatsPeriod::TwentyFourHours;
                Ok(Some(Action::RequestStats(StatsPeriod::TwentyFourHours)))
            }
            KeyCode::Char('w') => {
                self.period = StatsPeriod::SevenDays;
                Ok(Some(Action::RequestStats(StatsPeriod::SevenDays)))
            }
            KeyCode::Char('m') => {
                self.period = StatsPeriod::ThirtyDays;
                Ok(Some(Action::RequestStats(StatsPeriod::ThirtyDays)))
            }
            KeyCode::Char('r') => Ok(Some(Action::RequestStats(self.period))),
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::SetStatsPeriod(period) => {
                self.period = *period;
            }
            Action::StatsUpdated(data) => {
                self.bandwidth_tx.clone_from(&data.bandwidth_tx);
                self.bandwidth_rx.clone_from(&data.bandwidth_rx);
                self.client_counts.clone_from(&data.client_counts);
                self.dpi_apps.clone_from(&data.dpi_apps);
                self.dpi_categories.clone_from(&data.dpi_categories);
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let title = " Statistics ";
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

        // 4-panel layout: period selector | bandwidth | bottom row | hints
        let layout = Layout::vertical([
            Constraint::Length(1),      // period selector
            Constraint::Percentage(45), // bandwidth chart
            Constraint::Min(8),         // bottom row
            Constraint::Length(1),      // hints
        ])
        .split(inner);

        // Period selector
        let period_labels = &["1h", "24h", "7d", "30d"];
        let period_line = sub_tabs::render_sub_tabs(period_labels, self.period_index());
        frame.render_widget(Paragraph::new(period_line), layout[0]);

        // Bandwidth chart (area fills + Braille lines)
        self.render_bandwidth_chart(frame, layout[1]);

        // Bottom row: left (40%) | right (60%)
        let bottom = Layout::horizontal([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(layout[2]);

        // Left column: clients (50%) | categories (50%)
        let left_col = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(bottom[0]);

        self.render_client_chart(frame, left_col[0]);
        self.render_categories(frame, left_col[1]);

        // Right column: top applications (full height)
        self.render_top_apps(frame, bottom[1]);

        // Hints
        let hints = Line::from(vec![
            Span::styled("  h ", theme::key_hint_key()),
            Span::styled("1h  ", theme::key_hint()),
            Span::styled("d ", theme::key_hint_key()),
            Span::styled("24h  ", theme::key_hint()),
            Span::styled("w ", theme::key_hint_key()),
            Span::styled("7d  ", theme::key_hint()),
            Span::styled("m ", theme::key_hint_key()),
            Span::styled("30d  ", theme::key_hint()),
            Span::styled("r ", theme::key_hint_key()),
            Span::styled("refresh", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[3]);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Stats"
    }
}
