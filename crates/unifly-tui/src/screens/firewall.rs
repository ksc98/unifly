//! Firewall screen — zone-pair policies with sub-tabs (spec §2.5).

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use tokio::sync::mpsc::UnboundedSender;

use unifi_core::model::{AclRule, FirewallPolicy, FirewallZone};

use crate::action::{Action, Direction, FirewallSubTab};
use crate::component::Component;
use crate::theme;
use crate::widgets::sub_tabs;

pub struct FirewallScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    sub_tab: FirewallSubTab,
    policies: Arc<Vec<Arc<FirewallPolicy>>>,
    zones: Arc<Vec<Arc<FirewallZone>>>,
    acl_rules: Arc<Vec<Arc<AclRule>>>,
    policy_table: TableState,
    zone_table: TableState,
    acl_table: TableState,
}

impl FirewallScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            sub_tab: FirewallSubTab::default(),
            policies: Arc::new(Vec::new()),
            zones: Arc::new(Vec::new()),
            acl_rules: Arc::new(Vec::new()),
            policy_table: TableState::default(),
            zone_table: TableState::default(),
            acl_table: TableState::default(),
        }
    }

    fn active_table_state(&mut self) -> &mut TableState {
        match self.sub_tab {
            FirewallSubTab::Policies => &mut self.policy_table,
            FirewallSubTab::Zones => &mut self.zone_table,
            FirewallSubTab::AclRules => &mut self.acl_table,
        }
    }

    fn active_len(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => self.policies.len(),
            FirewallSubTab::Zones => self.zones.len(),
            FirewallSubTab::AclRules => self.acl_rules.len(),
        }
    }

    fn selected_index(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => self.policy_table.selected().unwrap_or(0),
            FirewallSubTab::Zones => self.zone_table.selected().unwrap_or(0),
            FirewallSubTab::AclRules => self.acl_table.selected().unwrap_or(0),
        }
    }

    fn select(&mut self, idx: usize) {
        let len = self.active_len();
        let clamped = if len == 0 { 0 } else { idx.min(len - 1) };
        self.active_table_state().select(Some(clamped));
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    fn move_selection(&mut self, delta: isize) {
        let len = self.active_len();
        if len == 0 {
            return;
        }
        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, len as isize - 1);
        self.select(next as usize);
    }

    fn sub_tab_index(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => 0,
            FirewallSubTab::Zones => 1,
            FirewallSubTab::AclRules => 2,
        }
    }

    fn render_policies(&self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("#").style(theme::table_header()),
            Cell::from("Enabled").style(theme::table_header()),
            Cell::from("Name").style(theme::table_header()),
            Cell::from("Action").style(theme::table_header()),
            Cell::from("Protocol").style(theme::table_header()),
            Cell::from("Source").style(theme::table_header()),
            Cell::from("Destination").style(theme::table_header()),
        ]);

        let selected_idx = self.policy_table.selected().unwrap_or(0);
        let rows: Vec<Row> = self
            .policies
            .iter()
            .enumerate()
            .map(|(i, policy)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▸" } else { " " };

                let idx = policy
                    .index
                    .map_or_else(|| (i + 1).to_string(), |n| n.to_string());
                let enabled = if policy.enabled { "✓" } else { "✗" };
                let action_str = format!("{:?}", policy.action);
                let action_color = match policy.action {
                    unifi_core::model::FirewallAction::Allow => theme::SUCCESS_GREEN,
                    unifi_core::model::FirewallAction::Block => theme::ERROR_RED,
                    unifi_core::model::FirewallAction::Reject => theme::CORAL,
                };
                let protocol = policy.protocol_summary.as_deref().unwrap_or("Any");
                let src = policy.source_summary.as_deref().unwrap_or("─");
                let dst = policy.destination_summary.as_deref().unwrap_or("─");

                let row_style = if is_selected {
                    theme::table_selected()
                } else {
                    theme::table_row()
                };

                Row::new(vec![
                    Cell::from(format!("{prefix}{idx}")),
                    Cell::from(enabled.to_string()).style(Style::default().fg(if policy.enabled {
                        theme::SUCCESS_GREEN
                    } else {
                        theme::BORDER_GRAY
                    })),
                    Cell::from(policy.name.clone()).style(
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(action_str).style(Style::default().fg(action_color)),
                    Cell::from(protocol.to_string()),
                    Cell::from(src.to_string()),
                    Cell::from(dst.to_string()),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Length(4),
            Constraint::Length(7),
            Constraint::Min(16),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(14),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.policy_table;
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_zones(&self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("Name").style(theme::table_header()),
            Cell::from("Networks").style(theme::table_header()),
        ]);

        let selected_idx = self.zone_table.selected().unwrap_or(0);
        let rows: Vec<Row> = self
            .zones
            .iter()
            .enumerate()
            .map(|(i, zone)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▸" } else { " " };
                let net_count = zone.network_ids.len();

                let row_style = if is_selected {
                    theme::table_selected()
                } else {
                    theme::table_row()
                };

                Row::new(vec![
                    Cell::from(format!("{prefix}{}", zone.name)).style(
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(format!("{net_count} networks")),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [Constraint::Min(20), Constraint::Length(14)];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.zone_table;
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_acl_rules(&self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("Name").style(theme::table_header()),
            Cell::from("Enabled").style(theme::table_header()),
            Cell::from("Type").style(theme::table_header()),
            Cell::from("Action").style(theme::table_header()),
            Cell::from("Source").style(theme::table_header()),
            Cell::from("Destination").style(theme::table_header()),
        ]);

        let selected_idx = self.acl_table.selected().unwrap_or(0);
        let rows: Vec<Row> = self
            .acl_rules
            .iter()
            .enumerate()
            .map(|(i, rule)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▸" } else { " " };
                let enabled = if rule.enabled { "✓" } else { "✗" };
                let rule_type = format!("{:?}", rule.rule_type);
                let action_str = format!("{:?}", rule.action);
                let action_color = match rule.action {
                    unifi_core::model::AclAction::Allow => theme::SUCCESS_GREEN,
                    unifi_core::model::AclAction::Block => theme::ERROR_RED,
                };
                let src = rule.source_summary.as_deref().unwrap_or("─");
                let dst = rule.destination_summary.as_deref().unwrap_or("─");

                let row_style = if is_selected {
                    theme::table_selected()
                } else {
                    theme::table_row()
                };

                Row::new(vec![
                    Cell::from(format!("{prefix}{}", rule.name)).style(
                        Style::default()
                            .fg(theme::NEON_CYAN)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Cell::from(enabled.to_string()),
                    Cell::from(rule_type),
                    Cell::from(action_str).style(Style::default().fg(action_color)),
                    Cell::from(src.to_string()),
                    Cell::from(dst.to_string()),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Min(16),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Length(14),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.acl_table;
        frame.render_stateful_widget(table, area, &mut state);
    }
}

impl Component for FirewallScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
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
                let len = self.active_len();
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
            // Sub-tab cycling (h/l — consistent with device detail tabs)
            KeyCode::Char('l') => {
                self.sub_tab = match self.sub_tab {
                    FirewallSubTab::Policies => FirewallSubTab::Zones,
                    FirewallSubTab::Zones => FirewallSubTab::AclRules,
                    FirewallSubTab::AclRules => FirewallSubTab::Policies,
                };
                Ok(Some(Action::FirewallSubTab(self.sub_tab)))
            }
            KeyCode::Char('h') => {
                self.sub_tab = match self.sub_tab {
                    FirewallSubTab::Policies => FirewallSubTab::AclRules,
                    FirewallSubTab::Zones => FirewallSubTab::Policies,
                    FirewallSubTab::AclRules => FirewallSubTab::Zones,
                };
                Ok(Some(Action::FirewallSubTab(self.sub_tab)))
            }
            // Reorder policies with K/J (shift)
            KeyCode::Char('K') if self.sub_tab == FirewallSubTab::Policies => {
                let idx = self.selected_index();
                Ok(Some(Action::ReorderPolicy(idx, Direction::Up)))
            }
            KeyCode::Char('J') if self.sub_tab == FirewallSubTab::Policies => {
                let idx = self.selected_index();
                Ok(Some(Action::ReorderPolicy(idx, Direction::Down)))
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::FirewallPoliciesUpdated(policies) => {
                self.policies = Arc::clone(policies);
            }
            Action::FirewallZonesUpdated(zones) => {
                self.zones = Arc::clone(zones);
            }
            Action::AclRulesUpdated(rules) => {
                self.acl_rules = Arc::clone(rules);
            }
            Action::FirewallSubTab(tab) => {
                self.sub_tab = *tab;
            }
            Action::ReorderPolicy(idx, direction) => {
                let len = self.policies.len();
                if len < 2 {
                    return Ok(None);
                }
                let target = match direction {
                    Direction::Up if *idx > 0 => idx - 1,
                    Direction::Down if *idx + 1 < len => idx + 1,
                    _ => return Ok(None),
                };
                // Swap in a mutable copy
                let policies = Arc::make_mut(&mut self.policies);
                policies.swap(*idx, target);
                self.select(target);
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let title = " Firewall ";
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
            Constraint::Length(1), // sub-tabs
            Constraint::Min(1),    // content
            Constraint::Length(1), // hints
        ])
        .split(inner);

        // Sub-tabs
        let tab_labels = &["Policies", "Zones", "ACL Rules"];
        let tab_line = sub_tabs::render_sub_tabs(tab_labels, self.sub_tab_index());
        frame.render_widget(Paragraph::new(tab_line), layout[0]);

        // Content
        match self.sub_tab {
            FirewallSubTab::Policies => self.render_policies(frame, layout[1]),
            FirewallSubTab::Zones => self.render_zones(frame, layout[1]),
            FirewallSubTab::AclRules => self.render_acl_rules(frame, layout[1]),
        }

        // Hints
        let hints = match self.sub_tab {
            FirewallSubTab::Policies => Line::from(vec![
                Span::styled("  j/k ", theme::key_hint_key()),
                Span::styled("navigate  ", theme::key_hint()),
                Span::styled("K/J ", theme::key_hint_key()),
                Span::styled("reorder  ", theme::key_hint()),
                Span::styled("h/l ", theme::key_hint_key()),
                Span::styled("sub-tab  ", theme::key_hint()),
                Span::styled("Enter ", theme::key_hint_key()),
                Span::styled("detail", theme::key_hint()),
            ]),
            _ => Line::from(vec![
                Span::styled("  j/k ", theme::key_hint_key()),
                Span::styled("navigate  ", theme::key_hint()),
                Span::styled("h/l ", theme::key_hint_key()),
                Span::styled("sub-tab  ", theme::key_hint()),
                Span::styled("Enter ", theme::key_hint_key()),
                Span::styled("detail", theme::key_hint()),
            ]),
        };
        frame.render_widget(Paragraph::new(hints), layout[2]);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Firewall"
    }
}
