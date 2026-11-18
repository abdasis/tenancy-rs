//! In-memory ports for tests and single-process deployments.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

use crate::error::TenancyError;
use crate::ports::{CacheStore, JobQueue, SessionStore, TenantJob};

/// Value plus optional expiry.
type Entry = (Vec<u8>, Option<Instant>);
type Entries = AsyncMutex<HashMap<String, Entry>>;

/// Cache backed by a `HashMap`; expired entries are dropped on read.
#[derive(Default)]
pub struct MemoryCache {
    // pakai Instant tokio supaya TTL deterministik di bawah `tokio::time::pause`
    entries: Entries,
}

impl MemoryCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CacheStore for MemoryCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        let mut entries = self.entries.lock().await;
        match entries.get(key) {
            Some((value, expires_at))
                if expires_at.is_none_or(|deadline| deadline > Instant::now()) =>
            {
                Ok(Some(value.clone()))
            }
            Some(_) => {
                entries.remove(key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        self.entries
            .lock()
            .await
            .insert(key.to_string(), (value, expires_at));
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        self.entries.lock().await.remove(key);
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.entries
            .lock()
            .await
            .retain(|key, _| !key.starts_with(prefix));
        Ok(())
    }
}

/// Session store backed by a `HashMap` (no TTL).
#[derive(Default)]
pub struct MemorySessionStore {
    entries: AsyncMutex<HashMap<String, Vec<u8>>>,
}

impl MemorySessionStore {
    /// Creates an empty session store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionStore for MemorySessionStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        Ok(self.entries.lock().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<(), TenancyError> {
        self.entries.lock().await.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        self.entries.lock().await.remove(key);
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.entries
            .lock()
            .await
            .retain(|key, _| !key.starts_with(prefix));
        Ok(())
    }
}

/// Job queue that keeps enqueued jobs in memory so tests can inspect them.
#[derive(Default)]
pub struct MemoryJobQueue {
    jobs: Mutex<Vec<TenantJob>>,
}

impl MemoryJobQueue {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of enqueued jobs.
    pub fn enqueued(&self) -> Vec<TenantJob> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl JobQueue for MemoryJobQueue {
    async fn enqueue(&self, job: TenantJob) -> Result<(), TenancyError> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(job);
        Ok(())
    }
}
