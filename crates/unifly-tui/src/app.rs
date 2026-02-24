//! Application core — event loop, screen management, action dispatch.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use unifly_core::{Command, Controller, EntityId, MacAddress};

use crate::action::{Action, ConfirmAction, Notification};
use crate::component::Component;
use crate::event::{Event, EventReader};
use crate::screen::ScreenId;
use crate::screens::create_screens;
use crate::theme;
use crate::tui::Tui;

/// Connection status as seen by the TUI.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Top-level application state and event loop.
pub struct App {
    /// Current active screen.
    active_screen: ScreenId,
    /// Previous screen for GoBack.
    previous_screen: Option<ScreenId>,
    /// All screen components, keyed by ScreenId.
    screens: HashMap<ScreenId, Box<dyn Component>>,
    /// Whether the app should keep running.
    running: bool,
    /// Connection status indicator.
    connection_status: ConnectionStatus,
    /// Help overlay visibility.
    help_visible: bool,
    /// Search overlay visibility.
    search_active: bool,
    /// Current search query.
    search_query: String,
    /// Terminal size for responsive layout.
    terminal_size: (u16, u16),
    /// Action sender — components can dispatch actions through this.
    action_tx: mpsc::UnboundedSender<Action>,
    /// Action receiver — main loop drains this.
    action_rx: mpsc::UnboundedReceiver<Action>,
    /// Optional controller for live data.
    controller: Option<Controller>,
    /// Cancellation token for the data bridge task.
    data_cancel: CancellationToken,
    /// Pending confirmation dialog (blocks other input while active).
    pending_confirm: Option<ConfirmAction>,
    /// Active notification toast with display timestamp.
    notification: Option<(Notification, Instant)>,
    /// When true, data updates are not forwarded to screens.
    paused: bool,
    /// Generation counter for stats requests — prevents stale responses from
    /// overwriting fresh data when the user rapidly switches periods.
    stats_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Timestamp of the last stats fetch — drives auto-refresh.
    last_stats_fetch: Option<std::time::Instant>,
    /// Currently selected stats period — preserved for auto-refresh.
    stats_period: crate::action::StatsPeriod,
}

impl App {
    /// Create a new App with all screens. Optionally accepts a [`Controller`]
    /// for live data — if `None`, the TUI shows the onboarding wizard.
    pub fn new(controller: Option<Controller>) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let mut screens: HashMap<ScreenId, Box<dyn Component>> =
            create_screens().into_iter().collect();

        // If no controller, show the onboarding wizard instead of the dashboard
        let active_screen = if controller.is_none() {
            screens.insert(
                ScreenId::Setup,
                Box::new(crate::screens::onboarding::OnboardingScreen::new()),
            );
            ScreenId::Setup
        } else {
            ScreenId::Dashboard
        };

        Self {
            active_screen,
            previous_screen: None,
            screens,
            running: true,
            connection_status: ConnectionStatus::default(),
            help_visible: false,
            search_active: false,
            search_query: String::new(),
            terminal_size: (0, 0),
            action_tx,
            action_rx,
            controller,
            data_cancel: CancellationToken::new(),
            pending_confirm: None,
            notification: None,
            paused: false,
            stats_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_stats_fetch: None,
            stats_period: crate::action::StatsPeriod::default(),
        }
    }

    /// Initialize all screen components with the action sender.
    fn init_screens(&mut self) -> Result<()> {
        for screen in self.screens.values_mut() {
            screen.init(self.action_tx.clone())?;
        }
        // Focus the initial screen
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            screen.set_focused(true);
        }
        Ok(())
    }

    /// Run the main event loop. This is the heart of the TUI.
    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;
        tui.enter()?;
        self.terminal_size = tui.size().unwrap_or((80, 24));
        self.init_screens()?;

        // Spawn data bridge if we have a controller
        if let Some(controller) = self.controller.clone() {
            let cancel = self.data_cancel.clone();
            let tx = self.action_tx.clone();
            tokio::spawn(async move {
                crate::data_bridge::spawn_data_bridge(controller, tx, cancel).await;
            });
        }

        let mut events = EventReader::new(
            Duration::from_millis(250), // 4 Hz tick
            Duration::from_millis(33),  // ~30 FPS render
        );

        info!("TUI event loop started");

        while self.running {
            // 1. Wait for the next event
            let Some(event) = events.next().await else {
                break;
            };

            // 2. Map event → action(s)
            match event {
                Event::Key(key) => {
                    if let Some(action) = self.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(action) = self.handle_mouse_event(mouse)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Resize(w, h) => {
                    self.action_tx.send(Action::Resize(w, h))?;
                }
                Event::Tick => {
                    self.action_tx.send(Action::Tick)?;
                }
                Event::Render => {
                    self.action_tx.send(Action::Render)?;
                }
            }

            // 3. Drain and process all queued actions
            while let Ok(action) = self.action_rx.try_recv() {
                self.process_action(&action)?;
            }

            // 4. Render on every event cycle
            tui.draw(|frame| self.render(frame))?;
        }

        // Cancel the data bridge and clean up
        self.data_cancel.cancel();
        events.stop();
        info!("TUI event loop ended");
        Ok(())
    }

    /// Map a key event to an action. Global keys are handled here;
    /// screen-specific keys are delegated to the active screen component.
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // Onboarding wizard captures all keys except Ctrl+C
        if self.active_screen == ScreenId::Setup {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                return Ok(Some(Action::Quit));
            }
            if let Some(screen) = self.screens.get_mut(&ScreenId::Setup) {
                return screen.handle_key_event(key);
            }
            return Ok(None);
        }

        // Settings screen captures all keys except Ctrl+C
        if self.active_screen == ScreenId::Settings {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                return Ok(Some(Action::Quit));
            }
            if let Some(screen) = self.screens.get_mut(&ScreenId::Settings) {
                return screen.handle_key_event(key);
            }
            return Ok(None);
        }

        // Confirmation dialog captures all input
        if self.pending_confirm.is_some() {
            return match key.code {
                KeyCode::Char('y' | 'Y') => Ok(Some(Action::ConfirmYes)),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => Ok(Some(Action::ConfirmNo)),
                _ => Ok(None),
            };
        }

        // Global keys always take priority (except when search is active)
        if self.search_active {
            return match key.code {
                KeyCode::Esc => {
                    self.search_query.clear();
                    Ok(Some(Action::CloseSearch))
                }
                KeyCode::Enter => Ok(Some(Action::SearchSubmit)),
                KeyCode::Backspace => {
                    self.search_query.pop();
                    Ok(Some(Action::SearchInput(self.search_query.clone())))
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    Ok(Some(Action::SearchInput(self.search_query.clone())))
                }
                _ => Ok(None),
            };
        }

        if self.help_visible {
            // In help mode, Esc or ? closes help
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') => Ok(Some(Action::ToggleHelp)),
                _ => Ok(None),
            };
        }

        // Global keybindings
        match (key.modifiers, key.code) {
            // Quit
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::NONE, KeyCode::Char('q')) => return Ok(Some(Action::Quit)),

            // Help
            (KeyModifiers::NONE, KeyCode::Char('?')) => return Ok(Some(Action::ToggleHelp)),

            // Search
            (KeyModifiers::NONE, KeyCode::Char('/')) => return Ok(Some(Action::OpenSearch)),

            // Settings
            (KeyModifiers::NONE, KeyCode::Char(',')) => return Ok(Some(Action::OpenSettings)),

            // Pause/resume data updates
            (KeyModifiers::NONE, KeyCode::Char('s')) => {
                self.paused = !self.paused;
                return Ok(None);
            }

            // Screen navigation via number keys
            (KeyModifiers::NONE, KeyCode::Char(c @ '1'..='8')) => {
                #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
                let n = c.to_digit(10).unwrap_or(0) as u8;
                if let Some(screen) = ScreenId::from_number(n) {
                    return Ok(Some(Action::SwitchScreen(screen)));
                }
            }

            // Tab / Shift+Tab for screen cycling
            (KeyModifiers::NONE, KeyCode::Tab) => {
                return Ok(Some(Action::SwitchScreen(self.active_screen.next())));
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                return Ok(Some(Action::SwitchScreen(self.active_screen.prev())));
            }

            // Esc — context-dependent back
            (KeyModifiers::NONE, KeyCode::Esc) => return Ok(Some(Action::GoBack)),

            _ => {}
        }

        // Delegate to active screen component
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_key_event(key);
        }

        Ok(None)
    }

    /// Handle mouse events (delegate to active screen).
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_mouse_event(mouse);
        }
        Ok(None)
    }

    /// Process a single action — update app state and propagate to components.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn process_action(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::Quit => {
                self.running = false;
            }

            Action::Resize(w, h) => {
                self.terminal_size = (*w, *h);
            }

            Action::SwitchScreen(target) => {
                if *target != self.active_screen {
                    debug!("switching screen: {} → {}", self.active_screen, target);
                    // Unfocus current screen
                    if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                        screen.set_focused(false);
                    }
                    self.previous_screen = Some(self.active_screen);
                    self.active_screen = *target;
                    // Focus new screen
                    if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                        screen.set_focused(true);
                    }

                    // Trigger stats fetch when arriving at the Stats screen
                    if *target == ScreenId::Stats {
                        self.action_tx
                            .send(Action::RequestStats(crate::action::StatsPeriod::default()))?;
                    }
                }
            }

            Action::GoBack => {
                if let Some(prev) = self.previous_screen.take() {
                    self.action_tx.send(Action::SwitchScreen(prev))?;
                }
            }

            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
            }

            Action::OpenSearch => {
                self.search_active = true;
                self.search_query.clear();
            }

            Action::CloseSearch => {
                self.search_active = false;
                self.search_query.clear();
            }

            Action::Connected => {
                self.connection_status = ConnectionStatus::Connected;
            }

            Action::Disconnected(_) => {
                self.connection_status = ConnectionStatus::Disconnected;
            }

            Action::Reconnecting => {
                self.connection_status = ConnectionStatus::Reconnecting;
            }

            Action::Render => {}

            Action::Tick => {
                // Auto-dismiss notifications after 3 seconds
                if let Some((_, created)) = &self.notification {
                    if created.elapsed() > Duration::from_secs(3) {
                        self.notification = None;
                    }
                }
                // Forward ticks to setup/settings screens for throbber animation
                if self.active_screen == ScreenId::Setup {
                    if let Some(screen) = self.screens.get_mut(&ScreenId::Setup) {
                        let _ = screen.update(action);
                    }
                }
                if self.active_screen == ScreenId::Settings {
                    if let Some(screen) = self.screens.get_mut(&ScreenId::Settings) {
                        let _ = screen.update(action);
                    }
                }
                // Auto-refresh stats every 60s while Stats screen is active
                if self.active_screen == ScreenId::Stats {
                    if let Some(last) = self.last_stats_fetch {
                        if last.elapsed() > std::time::Duration::from_secs(60) {
                            let _ = self.action_tx.send(Action::RequestStats(self.stats_period));
                        }
                    }
                }
            }

            // Data updates go to ALL screens so they stay in sync
            Action::DevicesUpdated(_)
            | Action::ClientsUpdated(_)
            | Action::NetworksUpdated(_)
            | Action::FirewallPoliciesUpdated(_)
            | Action::FirewallZonesUpdated(_)
            | Action::AclRulesUpdated(_)
            | Action::WifiBroadcastsUpdated(_)
            | Action::EventReceived(_)
            | Action::HealthUpdated(_)
            | Action::SiteUpdated(_)
            | Action::StatsUpdated(_)
            | Action::MonthlyWanUsage(_, _)
            | Action::ClientDailyUsageUpdated(_)
            | Action::NetworkEditResult(_) => {
                if !self.paused {
                    for screen in self.screens.values_mut() {
                        if let Some(follow_up) = screen.update(action)? {
                            self.action_tx.send(follow_up)?;
                        }
                    }
                }
            }

            // ── Command pipeline ──────────────────────────────────────

            // Destructive device commands → confirmation dialog
            Action::RequestRestart(id) => {
                let name = self.resolve_device_name(id);
                self.action_tx
                    .send(Action::ShowConfirm(ConfirmAction::RestartDevice {
                        id: id.clone(),
                        name,
                    }))?;
            }

            Action::RequestUnadopt(id) => {
                let name = self.resolve_device_name(id);
                self.action_tx
                    .send(Action::ShowConfirm(ConfirmAction::UnadoptDevice {
                        id: id.clone(),
                        name,
                    }))?;
            }

            // Non-destructive device commands → immediate execute
            Action::RequestLocate(id) => {
                if let Some(mac) = self.resolve_device_mac(id) {
                    self.execute_command(
                        Command::LocateDevice {
                            mac: mac.clone(),
                            enable: true,
                        },
                        format!("Locating {mac}"),
                    );
                }
            }

            // Destructive client commands → confirmation dialog
            Action::RequestBlockClient(id) => {
                let name = self.resolve_client_name(id);
                self.action_tx
                    .send(Action::ShowConfirm(ConfirmAction::BlockClient {
                        id: id.clone(),
                        name,
                    }))?;
            }

            Action::RequestUnblockClient(id) => {
                let name = self.resolve_client_name(id);
                self.action_tx
                    .send(Action::ShowConfirm(ConfirmAction::UnblockClient {
                        id: id.clone(),
                        name,
                    }))?;
            }

            Action::RequestForgetClient(id) => {
                let name = self.resolve_client_name(id);
                self.action_tx
                    .send(Action::ShowConfirm(ConfirmAction::ForgetClient {
                        id: id.clone(),
                        name,
                    }))?;
            }

            // Non-destructive client commands → immediate execute
            Action::RequestKickClient(id) => {
                if let Some(mac) = self.resolve_client_mac(id) {
                    let name = self.resolve_client_name(id);
                    self.execute_command(Command::KickClient { mac }, format!("Kicked {name}"));
                }
            }

            // Confirmation dialog management
            Action::ShowConfirm(confirm) => {
                self.pending_confirm = Some(confirm.clone());
            }

            Action::ConfirmYes => {
                if let Some(confirm) = self.pending_confirm.take() {
                    self.execute_confirm(confirm);
                }
            }

            Action::ConfirmNo => {
                self.pending_confirm = None;
            }

            // Network editing → execute update command
            Action::NetworkSave(id, update) => {
                self.execute_command(
                    Command::UpdateNetwork {
                        id: id.clone(),
                        update: *update.clone(),
                    },
                    "Updated network".into(),
                );
            }

            // Stats fetch
            Action::RequestStats(period) => {
                self.stats_period = *period;
                self.last_stats_fetch = Some(std::time::Instant::now());
                self.fetch_stats(*period);
            }

            // ── Onboarding completion ─────────────────────────────────
            Action::OnboardingComplete { config, .. } => {
                // Remove the setup screen
                self.screens.remove(&ScreenId::Setup);

                // Create controller and store it
                let controller = Controller::new(*config.clone());
                self.controller = Some(controller.clone());

                // Switch to dashboard
                self.active_screen = ScreenId::Dashboard;
                if let Some(screen) = self.screens.get_mut(&ScreenId::Dashboard) {
                    screen.set_focused(true);
                }

                // Spawn data bridge
                let cancel = self.data_cancel.clone();
                let tx = self.action_tx.clone();
                tokio::spawn(async move {
                    crate::data_bridge::spawn_data_bridge(controller, tx, cancel).await;
                });

                self.action_tx
                    .send(Action::Notify(Notification::success("Connected!")))?;
            }

            Action::OnboardingTestResult(_) => {
                // Forward to the setup screen
                if let Some(screen) = self.screens.get_mut(&ScreenId::Setup) {
                    if let Some(follow_up) = screen.update(action)? {
                        self.action_tx.send(follow_up)?;
                    }
                }
            }

            // ── Settings ─────────────────────────────────────────────
            Action::OpenSettings => {
                if self.active_screen != ScreenId::Settings && self.active_screen != ScreenId::Setup
                {
                    let mut screen = crate::screens::settings::SettingsScreen::new();
                    screen.init(self.action_tx.clone())?;
                    self.screens.insert(ScreenId::Settings, Box::new(screen));
                    self.previous_screen = Some(self.active_screen);
                    if let Some(s) = self.screens.get_mut(&self.active_screen) {
                        s.set_focused(false);
                    }
                    self.active_screen = ScreenId::Settings;
                    if let Some(s) = self.screens.get_mut(&ScreenId::Settings) {
                        s.set_focused(true);
                    }
                }
            }

            Action::CloseSettings => {
                self.screens.remove(&ScreenId::Settings);
                let target = self.previous_screen.take().unwrap_or(ScreenId::Dashboard);
                self.active_screen = target;
                if let Some(s) = self.screens.get_mut(&target) {
                    s.set_focused(true);
                }
            }

            Action::SettingsTestResult(_) => {
                if let Some(screen) = self.screens.get_mut(&ScreenId::Settings) {
                    if let Some(follow_up) = screen.update(action)? {
                        self.action_tx.send(follow_up)?;
                    }
                }
            }

            Action::SettingsApply { config, .. } => {
                // 1. Cancel old data bridge
                self.data_cancel.cancel();
                self.data_cancel = CancellationToken::new();

                // 2. Build new controller
                let controller = Controller::new(*config.clone());
                self.controller = Some(controller.clone());

                // 3. Spawn new data bridge
                let cancel = self.data_cancel.clone();
                let tx = self.action_tx.clone();
                tokio::spawn(async move {
                    crate::data_bridge::spawn_data_bridge(controller, tx, cancel).await;
                });

                // 4. Close settings, switch to dashboard
                self.screens.remove(&ScreenId::Settings);
                self.active_screen = ScreenId::Dashboard;
                if let Some(s) = self.screens.get_mut(&ScreenId::Dashboard) {
                    s.set_focused(true);
                }

                self.action_tx.send(Action::Notify(Notification::success(
                    "Settings saved, reconnecting\u{2026}",
                )))?;
            }

            // Notifications
            Action::Notify(n) => {
                self.notification = Some((n.clone(), Instant::now()));
            }

            Action::DismissNotification => {
                self.notification = None;
            }

            // Everything else goes to the active screen only
            other => {
                if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                    if let Some(follow_up) = screen.update(other)? {
                        self.action_tx.send(follow_up)?;
                    }
                }
            }
        }

        Ok(())
    }

    // ── Entity resolution helpers ────────────────────────────────

    fn resolve_device_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn resolve_device_mac(&self, id: &EntityId) -> Option<MacAddress> {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .map(|d| d.mac.clone())
    }

    fn resolve_client_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().client_by_id(id))
            .and_then(|c| c.name.clone().or(c.hostname.clone()))
            .unwrap_or_else(|| id.to_string())
    }

    fn resolve_client_mac(&self, id: &EntityId) -> Option<MacAddress> {
        self.controller
            .as_ref()
            .and_then(|c| c.store().client_by_id(id))
            .map(|c| c.mac.clone())
    }

    // ── Command execution ─────────────────────────────────────────

    /// Spawn a command execution task. Sends a Notify action on completion.
    fn execute_command(&self, cmd: Command, success_msg: String) {
        let Some(controller) = self.controller.clone() else {
            let _ = self
                .action_tx
                .send(Action::Notify(Notification::error("Not connected")));
            return;
        };
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            match controller.execute(cmd).await {
                Ok(_) => {
                    let _ = tx.send(Action::Notify(Notification::success(success_msg)));
                }
                Err(e) => {
                    warn!(error = %e, "command execution failed");
                    let _ = tx.send(Action::Notify(Notification::error(format!("{e}"))));
                }
            }
        });
    }

    /// Map a confirmed action to its Command and execute it.
    fn execute_confirm(&self, action: ConfirmAction) {
        match action {
            ConfirmAction::RestartDevice { id, name } => {
                self.execute_command(Command::RestartDevice { id }, format!("Restarting {name}"));
            }
            ConfirmAction::UnadoptDevice { id, name } => {
                self.execute_command(Command::RemoveDevice { id }, format!("Removed {name}"));
            }
            ConfirmAction::AdoptDevice { mac } => {
                self.execute_command(
                    Command::AdoptDevice {
                        mac: MacAddress::new(&mac),
                        ignore_device_limit: false,
                    },
                    format!("Adopting {mac}"),
                );
            }
            ConfirmAction::PowerCyclePort {
                device_id,
                port_idx,
            } => {
                self.execute_command(
                    Command::PowerCyclePort {
                        device_id,
                        port_idx,
                    },
                    format!("Power cycling port {port_idx}"),
                );
            }
            ConfirmAction::BlockClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(Command::BlockClient { mac }, format!("Blocked {name}"));
                }
            }
            ConfirmAction::UnblockClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(
                        Command::UnblockClient { mac },
                        format!("Unblocked {name}"),
                    );
                }
            }
            ConfirmAction::ForgetClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(Command::ForgetClient { mac }, format!("Forgot {name}"));
                }
            }
            ConfirmAction::DeleteFirewallPolicy { id, name } => {
                self.execute_command(
                    Command::DeleteFirewallPolicy { id },
                    format!("Deleted policy {name}"),
                );
            }
        }
    }

    /// Fetch historical stats from the controller and send `StatsUpdated`.
    ///
    /// Uses a generation counter so stale responses from a previous period
    /// switch are silently dropped.
    #[allow(clippy::too_many_lines)]
    fn fetch_stats(&self, period: crate::action::StatsPeriod) {
        use std::sync::atomic::Ordering;

        use crate::action::StatsData;

        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let interval = period.api_interval();

        // Bump generation — any in-flight task with an older generation will be dropped.
        let generation = self.stats_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let gen_ref = self.stats_generation.clone();

        // Compute time window for this period (UniFi expects epoch milliseconds).
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            clippy::as_conversions
        )]
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let start = Some(now_ms - period.duration_secs() * 1000);
        let end = Some(now_ms);

        tokio::spawn(async move {
            // Bandwidth + clients from Legacy API (time-windowed).
            // DPI: try Integration API first (has names), fall back to Legacy (numeric IDs).
            let (gw_res, site_res, dpi_apps_res, dpi_cats_res) = tokio::join!(
                controller.get_gateway_stats(interval, start, end, None),
                controller.get_site_stats(interval, start, end, None),
                controller.list_dpi_applications(),
                controller.list_dpi_categories(),
            );

            // If a newer request was issued while we were fetching, discard.
            if gen_ref.load(Ordering::Relaxed) != generation {
                return;
            }

            let mut data = StatsData::default();

            // Parse gateway stats → bandwidth TX/RX
            if let Ok(gw) = gw_res {
                for entry in &gw {
                    let ts = entry
                        .get("time")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0);
                    if let Some(tx_bytes) = entry
                        .get("wan-tx_bytes")
                        .or_else(|| entry.get("tx_bytes"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.bandwidth_tx.push((ts, tx_bytes));
                    }
                    if let Some(rx_bytes) = entry
                        .get("wan-rx_bytes")
                        .or_else(|| entry.get("rx_bytes"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.bandwidth_rx.push((ts, rx_bytes));
                    }
                }
            }

            // Parse site stats → client counts
            if let Ok(site) = site_res {
                for entry in &site {
                    let ts = entry
                        .get("time")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0);
                    if let Some(count) = entry
                        .get("num_sta")
                        .or_else(|| entry.get("wlan-num_sta"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.client_counts.push((ts, count));
                    }
                }
            }

            // DPI applications — prefer Integration API (has names), fall back to Legacy
            if let Ok(apps) = dpi_apps_res {
                if !apps.is_empty() {
                    let mut app_list: Vec<(String, u64)> = apps
                        .into_iter()
                        .map(|a| (a.name, a.tx_bytes + a.rx_bytes))
                        .filter(|(_, bytes)| *bytes > 0)
                        .collect();
                    app_list.sort_by(|a, b| b.1.cmp(&a.1));
                    app_list.truncate(10);
                    data.dpi_apps = app_list;
                }
            }
            // Legacy fallback: only fire if Integration API yielded nothing
            if data.dpi_apps.is_empty() {
                if let Ok(raw) = controller.get_dpi_stats("by_app", None).await {
                    data.dpi_apps = parse_legacy_dpi_apps(&raw);
                }
            }

            // DPI categories — prefer Integration API, fall back to Legacy
            if let Ok(cats) = dpi_cats_res {
                if !cats.is_empty() {
                    let mut cat_list: Vec<(String, u64)> = cats
                        .into_iter()
                        .map(|c| (c.name, c.tx_bytes + c.rx_bytes))
                        .filter(|(_, bytes)| *bytes > 0)
                        .collect();
                    cat_list.sort_by(|a, b| b.1.cmp(&a.1));
                    data.dpi_categories = cat_list;
                }
            }
            // Legacy fallback: only fire if Integration API yielded nothing
            if data.dpi_categories.is_empty() {
                if let Ok(raw) = controller.get_dpi_stats("by_cat", None).await {
                    data.dpi_categories = parse_legacy_dpi_categories(&raw);
                }
            }

            let _ = tx.send(Action::StatsUpdated(data));
        });
    }

    /// Render the full application frame.
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Onboarding and Settings get the full frame — no tab bar or status bar
        if self.active_screen == ScreenId::Setup {
            if let Some(screen) = self.screens.get(&ScreenId::Setup) {
                screen.render(frame, area);
            }
            return;
        }
        if self.active_screen == ScreenId::Settings {
            if let Some(screen) = self.screens.get(&ScreenId::Settings) {
                screen.render(frame, area);
            }
            return;
        }

        // Layout: [screen content] [tab bar] [status bar]
        let layout = Layout::vertical([
            Constraint::Min(1),    // Screen content
            Constraint::Length(1), // Tab bar
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        let content_area = layout[0];
        let tab_area = layout[1];
        let status_area = layout[2];

        // Render active screen
        if let Some(screen) = self.screens.get(&self.active_screen) {
            screen.render(frame, content_area);
        }

        // Render tab bar
        self.render_tab_bar(frame, tab_area);

        // Render status bar
        self.render_status_bar(frame, status_area);

        // Render overlays on top (order matters: last = topmost)
        if let Some((ref notif, _)) = self.notification {
            self.render_notification(frame, area, notif);
        }

        if let Some(ref confirm) = self.pending_confirm {
            self.render_confirm_dialog(frame, area, confirm);
        }

        if self.help_visible {
            self.render_help_overlay(frame, area);
        }
    }

    /// Render the bottom tab bar showing all 8 screens.
    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let compact = area.width < 100;
        let titles: Vec<Line> = ScreenId::ALL
            .iter()
            .map(|&id| {
                let style = if id == self.active_screen {
                    theme::tab_active()
                } else {
                    theme::tab_inactive()
                };
                let label = if compact {
                    id.label_short()
                } else {
                    id.label()
                };
                Line::from(Span::styled(format!(" {} {} ", id.number(), label), style))
            })
            .collect();

        let tabs = Tabs::new(titles)
            .divider(Span::styled(" ", theme::key_hint()))
            .select(
                ScreenId::ALL
                    .iter()
                    .position(|&s| s == self.active_screen)
                    .unwrap_or(0),
            );

        frame.render_widget(tabs, area);
    }

    /// Render the bottom status bar with connection status and key hints.
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        if self.search_active {
            let line = Line::from(vec![
                Span::styled(" / ", Style::default().fg(theme::ELECTRIC_PURPLE)),
                Span::styled(&self.search_query, Style::default().fg(theme::NEON_CYAN)),
                Span::styled("█", Style::default().fg(theme::NEON_CYAN)),
                Span::styled("  Esc cancel  Enter submit", theme::key_hint()),
            ]);
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let connection_indicator = if self.paused {
            Span::styled("⏸ paused", Style::default().fg(Color::Rgb(255, 165, 0)))
        } else {
            match &self.connection_status {
                ConnectionStatus::Connected => {
                    Span::styled("● connected", Style::default().fg(theme::SUCCESS_GREEN))
                }
                ConnectionStatus::Disconnected => {
                    Span::styled("○ disconnected", Style::default().fg(theme::ERROR_RED))
                }
                ConnectionStatus::Reconnecting => Span::styled(
                    "◐ reconnecting",
                    Style::default().fg(theme::ELECTRIC_YELLOW),
                ),
                ConnectionStatus::Connecting => {
                    Span::styled("◐ connecting", Style::default().fg(theme::ELECTRIC_YELLOW))
                }
            }
        };

        let hints = Span::styled(
            " │ ? help  / search  s pause  , settings  q quit",
            theme::key_hint(),
        );

        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let timestamp = Span::styled(
            format!(" │ {now}"),
            Style::default().fg(theme::BORDER_GRAY),
        );

        let line = Line::from(vec![Span::raw(" "), connection_indicator, hints, timestamp]);

        frame.render_widget(Paragraph::new(line), area);
    }

    /// Render the help overlay centered on screen with dimmed background.
    #[allow(clippy::unused_self)]
    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        // Dim the entire screen behind the overlay
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BG_DARK)),
            area,
        );

        let help_width = 60u16.min(area.width.saturating_sub(4));
        let help_height = 22u16.min(area.height.saturating_sub(4));

        let x = (area.width.saturating_sub(help_width)) / 2;
        let y = (area.height.saturating_sub(help_height)) / 2;

        let help_area = Rect::new(area.x + x, area.y + y, help_width, help_height);

        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(help_area);
        frame.render_widget(block, help_area);

        let help_text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Navigation",
                Style::default().fg(theme::NEON_CYAN),
            )]),
            Line::from(Span::styled("  ─────────────", theme::key_hint())),
            Line::from(vec![
                Span::styled("  1-8         ", theme::key_hint_key()),
                Span::styled("Jump to screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Tab         ", theme::key_hint_key()),
                Span::styled("Next screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Shift+Tab   ", theme::key_hint_key()),
                Span::styled("Previous screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  j/k ↑/↓     ", theme::key_hint_key()),
                Span::styled("Move up/down", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Enter       ", theme::key_hint_key()),
                Span::styled("Select / expand", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Esc         ", theme::key_hint_key()),
                Span::styled("Back / close", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  g/G         ", theme::key_hint_key()),
                Span::styled("Top / bottom", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+d/u    ", theme::key_hint_key()),
                Span::styled("Page down / up", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Global",
                Style::default().fg(theme::NEON_CYAN),
            )]),
            Line::from(Span::styled("  ──────────────", theme::key_hint())),
            Line::from(vec![
                Span::styled("  /           ", theme::key_hint_key()),
                Span::styled("Search            ", theme::key_hint()),
                Span::styled("?  ", theme::key_hint_key()),
                Span::styled("This help", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  s           ", theme::key_hint_key()),
                Span::styled("Sort column        ", theme::key_hint()),
                Span::styled("f  ", theme::key_hint_key()),
                Span::styled("Filter", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  ,           ", theme::key_hint_key()),
                Span::styled("Settings           ", theme::key_hint()),
                Span::styled("q  ", theme::key_hint_key()),
                Span::styled("Quit", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "                         Esc or ? to close",
                theme::key_hint(),
            )),
        ];

        let paragraph = Paragraph::new(help_text);
        frame.render_widget(paragraph, inner);
    }

    /// Render a centered confirmation dialog with dimmed background.
    #[allow(clippy::unused_self)]
    fn render_confirm_dialog(&self, frame: &mut Frame, area: Rect, confirm: &ConfirmAction) {
        // Dim the entire screen behind the dialog
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BG_DARK)),
            area,
        );

        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 5u16;

        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(area.x + x, area.y + y, width, height);

        let block = Block::default()
            .title(" Confirm ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::ELECTRIC_YELLOW));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let text = vec![
            Line::from(Span::styled(
                format!("  {confirm}"),
                Style::default().fg(theme::DIM_WHITE),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  y ", theme::key_hint_key()),
                Span::styled("confirm    ", theme::key_hint()),
                Span::styled("n ", theme::key_hint_key()),
                Span::styled("cancel", theme::key_hint()),
            ]),
        ];
        frame.render_widget(Paragraph::new(text), inner);
    }

    /// Render a notification toast in the bottom-right corner.
    #[allow(clippy::unused_self)]
    fn render_notification(&self, frame: &mut Frame, area: Rect, notif: &Notification) {
        use crate::action::NotificationLevel;

        let msg_len = u16::try_from(notif.message.len()).unwrap_or(u16::MAX);
        let width = (msg_len + 6).clamp(20, 60);
        let height = 3u16;

        let x = area.width.saturating_sub(width + 1);
        let y = area.height.saturating_sub(height + 2); // above status bar
        let toast_area = Rect::new(area.x + x, area.y + y, width, height);

        let (border_color, icon) = match notif.level {
            NotificationLevel::Success => (theme::SUCCESS_GREEN, "✓"),
            NotificationLevel::Error => (theme::ERROR_RED, "✗"),
            NotificationLevel::Warning => (theme::ELECTRIC_YELLOW, "!"),
            NotificationLevel::Info => (theme::NEON_CYAN, "·"),
        };

        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BG_DARK)),
            toast_area,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(toast_area);
        frame.render_widget(block, toast_area);

        let line = Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(border_color)),
            Span::styled(&notif.message, Style::default().fg(theme::DIM_WHITE)),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
    }
}

// ── Legacy DPI Parsing ──────────────────────────────────────────────────

/// Well-known UniFi DPI category IDs → human-readable names.
fn dpi_category_name(id: u64) -> &'static str {
    match id {
        0 => "Instant Messaging",
        1 => "P2P",
        2 => "File Transfer",
        3 => "Streaming Media",
        4 => "Mail & Collab",
        5 => "VoIP",
        6 => "Database",
        7 => "Games",
        8 => "Network Mgmt",
        9 => "Remote Access",
        10 => "Proxies & VPN",
        11 => "Stock Market",
        13 => "Web",
        14 => "Security Update",
        18 => "Web IM",
        20 => "Business",
        23 => "Network Proto",
        24 => "Social Network",
        255 => "Unknown",
        _ => "Other",
    }
}

/// Parse Legacy `stat/sitedpi` `by_app` response into `(name, total_bytes)` tuples.
///
/// The response is `[{"by_app": [{"app": 65541, "cat": 13, "rx_bytes": N, "tx_bytes": N}]}]`.
/// App names aren't available from Legacy API, so we show "Category/AppID".
fn parse_legacy_dpi_apps(raw: &[serde_json::Value]) -> Vec<(String, u64)> {
    let mut apps: Vec<(String, u64)> = Vec::new();
    for entry in raw {
        if let Some(by_app) = entry.get("by_app").and_then(|v| v.as_array()) {
            for item in by_app {
                let cat = item
                    .get("cat")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(255);
                let app_id = item
                    .get("app")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let tx = item
                    .get("tx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let rx = item
                    .get("rx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let total = tx + rx;
                if total > 0 {
                    let cat_name = dpi_category_name(cat);
                    // Extract sub-app ID from compound: app = (cat << 16) + sub_id
                    let sub_id = app_id & 0xFFFF;
                    apps.push((format!("{cat_name} #{sub_id}"), total));
                }
            }
        }
    }
    apps.sort_by(|a, b| b.1.cmp(&a.1));
    apps.truncate(10);
    apps
}

/// Parse Legacy `stat/sitedpi` `by_cat` response into `(name, total_bytes)` tuples.
///
/// The response is `[{"by_cat": [{"cat": 3, "rx_bytes": N, "tx_bytes": N}]}]`.
fn parse_legacy_dpi_categories(raw: &[serde_json::Value]) -> Vec<(String, u64)> {
    let mut cats: Vec<(String, u64)> = Vec::new();
    for entry in raw {
        if let Some(by_cat) = entry.get("by_cat").and_then(|v| v.as_array()) {
            for item in by_cat {
                let cat_id = item
                    .get("cat")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(255);
                let tx = item
                    .get("tx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let rx = item
                    .get("rx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let total = tx + rx;
                if total > 0 {
                    cats.push((dpi_category_name(cat_id).to_owned(), total));
                }
            }
        }
    }
    cats.sort_by(|a, b| b.1.cmp(&a.1));
    cats
}
