//! Tenant lookup cache decorator.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::ports::CacheStore;
use crate::repository::TenantRepository;
use crate::resolver::TenantKey;
use crate::tenant::{normalize_host, ConfigValue, Tenant, TenantId};

/// Caches tenant lookups by id, slug and domain; writes invalidate old keys.
pub struct CachedTenantRepository {
    inner: Arc<dyn TenantRepository>,
    cache: Arc<dyn CacheStore>,
    ttl: Duration,
}

impl CachedTenantRepository {
    /// Wraps `inner` with `cache`, caching lookups for `ttl`.
    pub fn new(
        inner: Arc<dyn TenantRepository>,
        cache: Arc<dyn CacheStore>,
        ttl: Duration,
    ) -> Self {
        Self { inner, cache, ttl }
    }

    fn cache_key(key: &TenantKey) -> String {
        match key {
            TenantKey::Id(id) => format!("repository:id:{id}"),
            TenantKey::Slug(slug) => format!("repository:slug:{slug}"),
            TenantKey::Domain(host) => format!("repository:domain:{}", normalize_host(host)),
        }
    }

    async fn invalidate(&self, tenant: &Tenant) -> Result<(), TenancyError> {
        self.cache
            .delete(&Self::cache_key(&TenantKey::Id(tenant.id)))
            .await?;
        self.cache
            .delete(&Self::cache_key(&TenantKey::Slug(tenant.slug.clone())))
            .await?;
        for domain in &tenant.domains {
            self.cache
                .delete(&Self::cache_key(&TenantKey::Domain(domain.host.clone())))
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl TenantRepository for CachedTenantRepository {
    async fn all(&self) -> Result<Vec<Tenant>, TenancyError> {
        self.inner.all().await
    }

    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, TenancyError> {
        self.find_by_key(&TenantKey::Id(id)).await
    }

    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, TenancyError> {
        let cache_key = Self::cache_key(key);
        if let Some(bytes) = self.cache.get(&cache_key).await? {
            match serde_json::from_slice::<Tenant>(&bytes) {
                Ok(tenant) => return Ok(Some(tenant)),
                Err(_) => {
                    // entri rusak/format lama: buang lalu baca dari repository
                    self.cache.delete(&cache_key).await?;
                }
            }
        }

        let found = self.inner.find_by_key(key).await?;
        if let Some(tenant) = &found {
            let bytes = serde_json::to_vec(tenant)
                .map_err(|error| TenancyError::Repository(error.to_string()))?;
            self.cache.set(&cache_key, bytes, Some(self.ttl)).await?;
        }
        Ok(found)
    }

    async fn create(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let created = self.inner.create(tenant).await?;
        self.invalidate(&created).await?;
        Ok(created)
    }

    async fn update(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let previous = self.inner.find(tenant.id).await?;
        let updated = self.inner.update(tenant).await?;
        if let Some(previous) = previous {
            self.invalidate(&previous).await?;
        }
        self.invalidate(&updated).await?;
        Ok(updated)
    }

    async fn delete(&self, id: TenantId) -> Result<bool, TenancyError> {
        let previous = self.inner.find(id).await?;
        let deleted = self.inner.delete(id).await?;
        match previous {
            Some(previous) => self.invalidate(&previous).await?,
            None => {
                self.cache
                    .delete(&Self::cache_key(&TenantKey::Id(id)))
                    .await?;
            }
        }
        Ok(deleted)
    }

    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), TenancyError> {
        self.inner.save_config(id, key, value).await?;
        self.cache
            .delete(&Self::cache_key(&TenantKey::Id(id)))
            .await
    }
}
