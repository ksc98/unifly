//! Terminal initialization, restoration, and panic-safe cleanup.
//!
//! Wraps the crossterm + ratatui terminal lifecycle so the rest of the app
//! never has to think about raw mode or alternate screen.

use std::io::{stdout, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    ExecutableCommand,
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Backend = CrosstermBackend<Stdout>;

/// Terminal wrapper that handles setup, teardown, and panic recovery.
pub struct Tui {
    pub terminal: Terminal<Backend>,
}

impl Tui {
    /// Create a new terminal instance (does NOT enter raw mode yet).
    pub fn new() -> Result<Self> {
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Enter TUI mode: alternate screen, raw mode, mouse capture, hidden cursor.
    pub fn enter(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        stdout().execute(EnableMouseCapture)?;
        stdout().execute(cursor::Hide)?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Exit TUI mode: restore terminal to its original state.
    pub fn exit(&mut self) -> Result<()> {
        // Best-effort restoration — don't bail on partial failures
        let _ = stdout().execute(cursor::Show);
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        Ok(())
    }

    /// Draw a frame using the provided render closure.
    pub fn draw<F>(&mut self, render: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(render)?;
        Ok(())
    }

    /// Get terminal size as (width, height).
    pub fn size(&self) -> Result<(u16, u16)> {
        let size = self.terminal.size()?;
        Ok((size.width, size.height))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}

/// Install panic and error hooks that restore the terminal before printing.
///
/// Must be called BEFORE entering the terminal, so panics during init
/// also get clean output.
pub fn install_hooks() -> Result<()> {
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .into_hooks();

    // color-eyre error report hook
    eyre_hook.install()?;

    // Panic hook: restore terminal, then print the panic
    let panic_hook = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restoration
        let _ = stdout().execute(cursor::Show);
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        // Now print the panic with full context
        panic_hook(info);
    }));

    Ok(())
}
