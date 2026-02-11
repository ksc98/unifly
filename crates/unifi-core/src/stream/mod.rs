// ── Reactive entity streams ──
//
// Subscription types for consuming entity changes from the DataStore.

mod filter;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;

pub use filter::{ClientFilter, DeviceFilter};

/// A subscription to a collection of entities.
///
/// Provides both point-in-time snapshot access and reactive change
/// notification via the `changed()` method or by converting to a `Stream`.
pub struct EntityStream<T: Clone + Send + Sync + 'static> {
    current: Arc<Vec<Arc<T>>>,
    receiver: watch::Receiver<Arc<Vec<Arc<T>>>>,
}

impl<T: Clone + Send + Sync + 'static> EntityStream<T> {
    pub(crate) fn new(receiver: watch::Receiver<Arc<Vec<Arc<T>>>>) -> Self {
        let current = receiver.borrow().clone();
        Self { current, receiver }
    }

    /// Get the snapshot captured at creation time.
    pub fn current(&self) -> &Arc<Vec<Arc<T>>> {
        &self.current
    }

    /// Get the latest snapshot (may have changed since creation).
    pub fn latest(&self) -> Arc<Vec<Arc<T>>> {
        self.receiver.borrow().clone()
    }

    /// Wait for the next change, returning the new snapshot.
    /// Returns `None` if the sender (DataStore) has been dropped.
    pub async fn changed(&mut self) -> Option<Arc<Vec<Arc<T>>>> {
        self.receiver.changed().await.ok()?;
        let snap = self.receiver.borrow_and_update().clone();
        self.current = snap.clone();
        Some(snap)
    }

    /// Convert into a `Stream` for use with `StreamExt` combinators.
    pub fn into_stream(self) -> EntityWatchStream<T> {
        EntityWatchStream {
            inner: WatchStream::new(self.receiver),
        }
    }
}

/// `Stream` adapter backed by a `watch::Receiver`.
///
/// Yields a new `Arc<Vec<Arc<T>>>` snapshot each time the underlying
/// collection is mutated.
pub struct EntityWatchStream<T: Clone + Send + Sync + 'static> {
    inner: WatchStream<Arc<Vec<Arc<T>>>>,
}

impl<T: Clone + Send + Sync + 'static> Stream for EntityWatchStream<T> {
    type Item = Arc<Vec<Arc<T>>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // WatchStream is Unpin when the inner type is Unpin.
        // Arc<Vec<Arc<T>>> is always Unpin, so this is safe.
        Pin::new(&mut self.inner).poll_next(cx)
    }
}
