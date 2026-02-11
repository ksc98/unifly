//! Event system — crossterm event reader running in a background tokio task.
//!
//! Produces terminal events (key, mouse, resize) plus tick/render events
//! at configurable intervals via `tokio::sync::mpsc`.

use std::time::Duration;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind, MouseEvent};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Events produced by the terminal event reader.
#[derive(Debug)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse action occurred.
    Mouse(MouseEvent),
    /// Terminal was resized to (cols, rows).
    Resize(u16, u16),
    /// Periodic tick for data refresh / animation (4 Hz).
    Tick,
    /// Render tick (~30 FPS).
    Render,
}

/// Reads terminal events in a background task and sends them over a channel.
pub struct EventReader {
    rx: mpsc::UnboundedReceiver<Event>,
    cancel: CancellationToken,
}

impl EventReader {
    /// Spawn the background event reader.
    ///
    /// - `tick_rate`: interval for `Event::Tick` (e.g., 250ms = 4 Hz)
    /// - `render_rate`: interval for `Event::Render` (e.g., 33ms ≈ 30 FPS)
    pub fn new(tick_rate: Duration, render_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();

        let task_cancel = cancel.clone();
        tokio::spawn(async move {
            let mut event_stream = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_rate);
            let mut render_interval = tokio::time::interval(render_rate);

            // Don't burst ticks if we fall behind
            tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            render_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                let event = tokio::select! {
                    _ = task_cancel.cancelled() => break,

                    _ = tick_interval.tick() => Event::Tick,

                    _ = render_interval.tick() => Event::Render,

                    Some(Ok(crossterm_event)) = event_stream.next() => {
                        match crossterm_event {
                            CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                                Event::Key(key)
                            }
                            CrosstermEvent::Mouse(mouse) => Event::Mouse(mouse),
                            CrosstermEvent::Resize(w, h) => Event::Resize(w, h),
                            // Ignore key release/repeat and other event types
                            _ => continue,
                        }
                    }
                };

                // If the receiver is dropped, stop.
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        Self { rx, cancel }
    }

    /// Receive the next event. Returns `None` if the reader has stopped.
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }

    /// Signal the background reader to stop.
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for EventReader {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
