// ── Generic reactive entity collection ──
//
// Lock-free concurrent storage with O(1) lookups and push-based
// change notification via `watch` channels.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::watch;

use crate::model::EntityId;

/// A lock-free, reactive collection for a single entity type.
///
/// Uses `DashMap` for O(1) concurrent lookups and `watch` channels
/// for push-based change notification. Every mutation bumps a version
/// counter and rebuilds the snapshot that subscribers receive.
pub(crate) struct EntityCollection<T: Clone + Send + Sync + 'static> {
    /// Primary storage: key string -> entity.
    /// Keys are MAC addresses for devices/clients, synthetic prefixed IDs
    /// (e.g. `"net:{id}"`) for other entities.
    by_key: DashMap<String, Arc<T>>,

    /// Secondary index: EntityId -> key string.
    id_to_key: DashMap<EntityId, String>,

    /// Reverse of `id_to_key` for efficient removal.
    key_to_id: DashMap<String, EntityId>,

    /// Version counter, bumped on every mutation.
    version: watch::Sender<u64>,

    /// Full snapshot, rebuilt on mutation for efficient subscription.
    snapshot: watch::Sender<Arc<Vec<Arc<T>>>>,
}

impl<T: Clone + Send + Sync + 'static> EntityCollection<T> {
    pub(crate) fn new() -> Self {
        let (version, _) = watch::channel(0u64);
        let (snapshot, _) = watch::channel(Arc::new(Vec::new()));

        Self {
            by_key: DashMap::new(),
            id_to_key: DashMap::new(),
            key_to_id: DashMap::new(),
            version,
            snapshot,
        }
    }

    /// Insert or update an entity. Returns `true` if the key was new.
    pub(crate) fn upsert(&self, key: String, id: EntityId, entity: T) -> bool {
        // Clean up stale id mapping if the key already existed with a different id.
        if let Some(old_id) = self.key_to_id.get(&key) {
            if *old_id != id {
                self.id_to_key.remove(&*old_id);
            }
        }

        let is_new = !self.by_key.contains_key(&key);
        self.by_key.insert(key.clone(), Arc::new(entity));
        self.id_to_key.insert(id.clone(), key.clone());
        self.key_to_id.insert(key, id);

        self.rebuild_snapshot();
        self.bump_version();

        is_new
    }

    /// Remove an entity by key. Returns the removed entity if it existed.
    pub(crate) fn remove(&self, key: &str) -> Option<Arc<T>> {
        let removed = self.by_key.remove(key).map(|(_, v)| v);
        if removed.is_some() {
            if let Some((_, id)) = self.key_to_id.remove(key) {
                self.id_to_key.remove(&id);
            }
            self.rebuild_snapshot();
            self.bump_version();
        }
        removed
    }

    /// Look up an entity by its primary key string.
    pub(crate) fn get_by_key(&self, key: &str) -> Option<Arc<T>> {
        self.by_key.get(key).map(|r| Arc::clone(r.value()))
    }

    /// Look up an entity by its `EntityId` (secondary index).
    pub(crate) fn get_by_id(&self, id: &EntityId) -> Option<Arc<T>> {
        let key = self.id_to_key.get(id)?;
        self.by_key
            .get(key.value().as_str())
            .map(|r| Arc::clone(r.value()))
    }

    /// Get the current snapshot (cheap `Arc` clone).
    pub(crate) fn snapshot(&self) -> Arc<Vec<Arc<T>>> {
        self.snapshot.borrow().clone()
    }

    /// Subscribe to snapshot changes via a `watch::Receiver`.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<Vec<Arc<T>>>> {
        self.snapshot.subscribe()
    }

    /// Remove all entities.
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.by_key.clear();
        self.id_to_key.clear();
        self.key_to_id.clear();
        self.rebuild_snapshot();
        self.bump_version();
    }

    pub(crate) fn len(&self) -> usize {
        self.by_key.len()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Return all current primary keys in the collection.
    pub(crate) fn keys(&self) -> Vec<String> {
        self.by_key.iter().map(|r| r.key().clone()).collect()
    }

    // ── Private helpers ──────────────────────────────────────────────

    /// Collect all values into a snapshot vec and broadcast to subscribers.
    fn rebuild_snapshot(&self) {
        let values: Vec<Arc<T>> = self.by_key.iter().map(|r| Arc::clone(r.value())).collect();
        // `send_modify` updates unconditionally, even with zero receivers.
        self.snapshot.send_modify(|snap| *snap = Arc::new(values));
    }

    /// Increment the version counter.
    fn bump_version(&self) {
        self.version.send_modify(|v| *v += 1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::EntityId;
    use uuid::Uuid;

    #[test]
    fn upsert_returns_true_for_new_key() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        assert!(col.upsert("key1".into(), id, "hello".into()));
    }

    #[test]
    fn upsert_returns_false_for_existing_key() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        col.upsert("key1".into(), id.clone(), "hello".into());
        assert!(!col.upsert("key1".into(), id, "world".into()));
    }

    #[test]
    fn get_by_key_and_id() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::Uuid(Uuid::new_v4());
        col.upsert("key1".into(), id.clone(), "hello".into());

        assert_eq!(*col.get_by_key("key1").unwrap(), "hello");
        assert_eq!(*col.get_by_id(&id).unwrap(), "hello");
    }

    #[test]
    fn remove_cleans_up_indexes() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        col.upsert("key1".into(), id.clone(), "hello".into());

        let removed = col.remove("key1");
        assert_eq!(*removed.unwrap(), "hello");
        assert!(col.get_by_key("key1").is_none());
        assert!(col.get_by_id(&id).is_none());
        assert!(col.is_empty());
    }

    #[test]
    fn clear_empties_everything() {
        let col: EntityCollection<String> = EntityCollection::new();
        col.upsert("a".into(), EntityId::from("1"), "x".into());
        col.upsert("b".into(), EntityId::from("2"), "y".into());
        assert_eq!(col.len(), 2);

        col.clear();
        assert!(col.is_empty());
        assert!(col.snapshot().is_empty());
    }

    #[test]
    fn snapshot_reflects_current_state() {
        let col: EntityCollection<String> = EntityCollection::new();
        assert!(col.snapshot().is_empty());

        col.upsert("a".into(), EntityId::from("1"), "x".into());
        col.upsert("b".into(), EntityId::from("2"), "y".into());

        let snap = col.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn upsert_with_changed_id_cleans_old_mapping() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id1 = EntityId::from("old-id");
        let id2 = EntityId::from("new-id");

        col.upsert("key1".into(), id1.clone(), "v1".into());
        assert!(col.get_by_id(&id1).is_some());

        // Re-upsert same key with different id
        col.upsert("key1".into(), id2.clone(), "v2".into());
        assert!(col.get_by_id(&id1).is_none()); // old id cleaned up
        assert_eq!(*col.get_by_id(&id2).unwrap(), "v2");
    }
}
