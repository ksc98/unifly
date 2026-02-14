//! Stats screen — historical charts and analytics (spec §2.8).

use crate::action::{Action, StatsPeriod};
use crate::component::Component;
use crate::theme;
use crate::widgets::sub_tabs;
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
    /// DPI top apps: (name, percentage)
    dpi_apps: Vec<(String, f64)>,
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

        // Determine bounds
        let x_min = self.bandwidth_tx.first().map_or(0.0, |(x, _)| *x);
        let x_max = self.bandwidth_tx.last().map_or(1.0, |(x, _)| *x);
        let y_max = self
            .bandwidth_tx
            .iter()
            .chain(self.bandwidth_rx.iter())
            .map(|(_, y)| *y)
            .fold(0.0f64, f64::max)
            * 1.1;

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

        let chart = Chart::new(vec![tx_dataset, rx_dataset])
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

    fn render_dpi_chart(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Top Applications (DPI) ")
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

        let max_bar_width = f64::from(inner.width.saturating_sub(22));
        let colors = theme::CHART_SERIES;

        let mut lines = Vec::new();
        for (i, (name, pct)) in self.dpi_apps.iter().enumerate().take(5) {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let bar_width = (pct / 100.0 * max_bar_width).round() as usize;
            let bar: String = "█".repeat(bar_width);
            let color = colors[i % colors.len()];
            let display_name: String = name.chars().take(14).collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {display_name:<14} "),
                    Style::default().fg(theme::DIM_WHITE),
                ),
                Span::styled(bar, Style::default().fg(color)),
                Span::styled(
                    format!("  {pct:.0}%"),
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

        let layout = Layout::vertical([
            Constraint::Length(1),      // period selector
            Constraint::Percentage(50), // bandwidth chart
            Constraint::Percentage(50), // bottom row: client count + DPI
            Constraint::Length(1),      // hints
        ])
        .split(inner);

        // Period selector
        let period_labels = &["1h", "24h", "7d", "30d"];
        let period_line = sub_tabs::render_sub_tabs(period_labels, self.period_index());
        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![Span::styled(
                " Period: ",
                Style::default().fg(theme::DIM_WHITE),
            )])]),
            Rect {
                height: 0,
                ..layout[0]
            },
        );
        frame.render_widget(Paragraph::new(period_line), layout[0]);

        // Bandwidth chart
        self.render_bandwidth_chart(frame, layout[1]);

        // Bottom row: client count + DPI
        let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[2]);

        self.render_client_chart(frame, bottom[0]);
        self.render_dpi_chart(frame, bottom[1]);

        // Hints
        let hints = Line::from(vec![
            Span::styled("  h ", theme::key_hint_key()),
            Span::styled("1h  ", theme::key_hint()),
            Span::styled("d ", theme::key_hint_key()),
            Span::styled("24h  ", theme::key_hint()),
            Span::styled("w ", theme::key_hint_key()),
            Span::styled("7d  ", theme::key_hint()),
            Span::styled("m ", theme::key_hint_key()),
            Span::styled("30d", theme::key_hint()),
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
