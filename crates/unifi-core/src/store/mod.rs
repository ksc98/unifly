// ── Reactive data store ──
//
// Lock-free entity storage with push-based change notification.

mod collection;
mod data_store;
mod refresh;

pub use data_store::DataStore;
