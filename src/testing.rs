//! Test helpers (feature `testing`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::bootstrapper::Bootstrapper;
use crate::context::TenantContext;
use crate::error::TenancyError;
use crate::event::{TenancyEvent, TenancyListener};
use crate::manager::Tenancy;
use crate::ports::local::LocalStorage;
use crate::ports::memory::{MemoryCache, MemoryJobQueue, MemorySessionStore};
use crate::repository::memory::MemoryTenantRepository;
use crate::tenant::Tenant;

/// Tenancy wired with in-memory ports and storage under the temp directory.
pub fn fake_tenancy() -> Tenancy {
    let storage_root = storage_root();
    let storage = LocalStorage::new(storage_root).expect("temp dir is absolute");
    Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .cache(Arc::new(MemoryCache::new()))
        .sessions(Arc::new(MemorySessionStore::new()))
        .queue(Arc::new(MemoryJobQueue::new()))
        .storage(Arc::new(storage))
        .build()
        .expect("fake tenancy builder is complete")
}

/// Storage root used by [`fake_tenancy`].
pub fn storage_root() -> PathBuf {
    std::env::temp_dir().join("tenancy-test")
}

/// Test tenant factory; the tenant is unsaved (`id == 0`).
pub fn tenant(slug: &str) -> Tenant {
    Tenant::new(format!("Tenant {slug}"), slug).expect("valid slug")
}

/// Bootstrapper recording `bootstrap:{name}` and `revert:{name}` calls.
pub struct FakeBootstrapper {
    name: String,
    log: Arc<Mutex<Vec<String>>>,
    fail: bool,
}

impl FakeBootstrapper {
    /// Creates a bootstrapper named `name`, recording into `log`.
    pub fn new(name: impl Into<String>, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name: name.into(),
            log,
            fail: false,
        }
    }

    /// Makes `bootstrap` fail with [`TenancyError::Bootstrapper`].
    pub fn failing(mut self) -> Self {
        self.fail = true;
        self
    }

    fn record(&self, entry: String) {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(entry);
    }
}

#[async_trait]
impl Bootstrapper for FakeBootstrapper {
    async fn bootstrap(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        self.record(format!("bootstrap:{}", self.name));
        if self.fail {
            return Err(TenancyError::Bootstrapper(format!("{} failed", self.name)));
        }
        Ok(())
    }

    async fn revert(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        self.record(format!("revert:{}", self.name));
        Ok(())
    }
}

/// Listener recording every event for assertions.
#[derive(Default)]
pub struct RecordingListener {
    events: Arc<Mutex<Vec<TenancyEvent>>>,
}

impl RecordingListener {
    /// Creates an empty listener log holder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Recorded events, in order.
    pub fn events(&self) -> Vec<TenancyEvent> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl TenancyListener for RecordingListener {
    async fn handle(&self, event: &TenancyEvent) -> Result<(), TenancyError> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event.clone());
        Ok(())
    }
}
