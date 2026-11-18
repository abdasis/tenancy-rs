//! Ports implemented by the host application: cache, storage, sessions, queue and encryption.
//!
//! The crate ships memory/local implementations and tenant-scoping wrappers;
//! adapters for Redis and AES-GCM live behind the `redis` and `encryption`
//! features.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::current_context;
use crate::error::TenancyError;
use crate::tenant::TenantId;

pub mod local;
pub mod memory;
#[cfg(feature = "redis")]
pub mod redis;

/// Deferred value loader used by [`CacheStore::remember`].
pub type Loader<'a> = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TenancyError>> + Send + 'a>>
        + Send
        + 'a,
>;

/// Byte-oriented cache with optional TTL.
#[async_trait]
pub trait CacheStore: Send + Sync {
    /// Returns the stored bytes, if any and not expired.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError>;
    /// Stores bytes with an optional TTL.
    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError>;
    /// Deletes one key.
    async fn delete(&self, key: &str) -> Result<(), TenancyError>;
    /// Deletes every key starting with `prefix`.
    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError>;

    /// Returns the cached value, or runs `loader`, stores and returns its value on a miss.
    async fn remember(
        &self,
        key: &str,
        ttl: Option<Duration>,
        loader: Loader<'_>,
    ) -> Result<Vec<u8>, TenancyError> {
        if let Some(value) = self.get(key).await? {
            return Ok(value);
        }
        let value = loader().await?;
        self.set(key, value.clone(), ttl).await?;
        Ok(value)
    }
}

/// Tenant-rooted file storage with relative paths.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Writes bytes at a tenant relative path.
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<(), TenancyError>;
    /// Reads bytes, `NotFound` when the file is missing.
    async fn get(&self, path: &str) -> Result<Vec<u8>, TenancyError>;
    /// Returns whether the path exists.
    async fn exists(&self, path: &str) -> Result<bool, TenancyError>;
    /// Deletes a file; missing paths are ignored.
    async fn delete(&self, path: &str) -> Result<(), TenancyError>;
    /// Deletes a file or directory tree below `prefix`.
    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError>;
}

/// Session storage without TTL (sessions expire at the host layer).
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Returns the stored bytes, if any.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError>;
    /// Stores bytes.
    async fn set(&self, key: &str, value: Vec<u8>) -> Result<(), TenancyError>;
    /// Deletes one key.
    async fn delete(&self, key: &str) -> Result<(), TenancyError>;
    /// Deletes every key starting with `prefix`.
    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError>;
}

/// Authenticated encryption for tenant configuration values.
#[async_trait]
pub trait Encryptor: Send + Sync {
    /// Encrypts plaintext, returning nonce-prefixed ciphertext.
    async fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, TenancyError>;
    /// Decrypts ciphertext produced by `seal`.
    async fn open(&self, ciphertext: &[u8]) -> Result<Vec<u8>, TenancyError>;
}

/// Job carrying its tenant so a worker can call `tenancy.run` before executing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenantJob {
    /// Tenant that must be active before the job runs.
    pub tenant_id: TenantId,
    /// Job name used by the worker.
    pub name: String,
    /// Job payload.
    pub payload: serde_json::Value,
}

/// Queue port; implementations must preserve the job's tenant id.
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Enqueues the job, keeping its tenant id.
    async fn enqueue(&self, job: TenantJob) -> Result<(), TenancyError>;
}

/// Cache wrapper that prefixes every key with the active tenant namespace.
#[derive(Clone)]
pub struct ScopedCache {
    inner: Arc<dyn CacheStore>,
}

impl ScopedCache {
    /// Wraps `inner`.
    pub fn new(inner: Arc<dyn CacheStore>) -> Self {
        Self { inner }
    }

    fn prefix() -> Result<String, TenancyError> {
        current_context()
            .map(|context| format!("{}:", context.cache_namespace()))
            .ok_or(TenancyError::NotResolved)
    }
}

#[async_trait]
impl CacheStore for ScopedCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.get(&key).await
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.set(&key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.delete(&key).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        let prefix = format!("{}{}", Self::prefix()?, prefix);
        self.inner.delete_prefix(&prefix).await
    }
}

/// Session wrapper that prefixes every key with the active tenant namespace.
#[derive(Clone)]
pub struct ScopedSession {
    inner: Arc<dyn SessionStore>,
}

impl ScopedSession {
    /// Wraps `inner`.
    pub fn new(inner: Arc<dyn SessionStore>) -> Self {
        Self { inner }
    }

    fn prefix() -> Result<String, TenancyError> {
        current_context()
            .map(|context| format!("{}:", context.session_namespace()))
            .ok_or(TenancyError::NotResolved)
    }
}

#[async_trait]
impl SessionStore for ScopedSession {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.get(&key).await
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<(), TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.set(&key, value).await
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        let key = format!("{}{}", Self::prefix()?, key);
        self.inner.delete(&key).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        let prefix = format!("{}{}", Self::prefix()?, prefix);
        self.inner.delete_prefix(&prefix).await
    }
}

/// Storage wrapper that prefixes every path with the active tenant slug.
#[derive(Clone)]
pub struct ScopedStorage {
    inner: Arc<dyn Storage>,
}

impl ScopedStorage {
    /// Wraps `inner`.
    pub fn new(inner: Arc<dyn Storage>) -> Self {
        Self { inner }
    }

    fn prefix() -> Result<String, TenancyError> {
        current_context()
            .map(|context| format!("{}/", context.tenant().slug))
            .ok_or(TenancyError::NotResolved)
    }
}

#[async_trait]
impl Storage for ScopedStorage {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<(), TenancyError> {
        let path = format!("{}{}", Self::prefix()?, path);
        self.inner.put(&path, data).await
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, TenancyError> {
        let path = format!("{}{}", Self::prefix()?, path);
        self.inner.get(&path).await
    }

    async fn exists(&self, path: &str) -> Result<bool, TenancyError> {
        let path = format!("{}{}", Self::prefix()?, path);
        self.inner.exists(&path).await
    }

    async fn delete(&self, path: &str) -> Result<(), TenancyError> {
        let path = format!("{}{}", Self::prefix()?, path);
        self.inner.delete(&path).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        let prefix = format!("{}{}", Self::prefix()?, prefix);
        self.inner.delete_prefix(&prefix).await
    }
}
