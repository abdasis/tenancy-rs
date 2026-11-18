//! In-memory tenant repository for tests and small single-process setups.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::repository::TenantRepository;
use crate::resolver::TenantKey;
use crate::tenant::{normalize_host, ConfigValue, Tenant, TenantId};

/// [`TenantRepository`] backed by a `HashMap`.
#[derive(Default)]
pub struct MemoryTenantRepository {
    tenants: RwLock<HashMap<TenantId, Tenant>>,
}

impl MemoryTenantRepository {
    /// Creates an empty repository.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `tenant` as-is, keyed by its id.
    pub fn insert(&self, tenant: Tenant) {
        let mut tenants = self
            .tenants
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        tenants.insert(tenant.id, tenant);
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<TenantId, Tenant>> {
        self.tenants
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl TenantRepository for MemoryTenantRepository {
    async fn all(&self) -> Result<Vec<Tenant>, TenancyError> {
        let mut tenants: Vec<Tenant> = self.read().values().cloned().collect();
        tenants.sort_by_key(|tenant| tenant.id);
        Ok(tenants)
    }

    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, TenancyError> {
        Ok(self.read().get(&id).cloned())
    }

    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, TenancyError> {
        let tenants = self.read();
        let found = match key {
            TenantKey::Id(id) => tenants.get(id).cloned(),
            TenantKey::Slug(slug) => tenants
                .values()
                .find(|tenant| tenant.slug == *slug)
                .cloned(),
            TenantKey::Domain(host) => {
                let host = normalize_host(host);
                tenants
                    .values()
                    .find(|tenant| tenant.domains.iter().any(|domain| domain.host == host))
                    .cloned()
            }
        };
        Ok(found)
    }

    async fn create(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let mut tenants = self
            .tenants
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = if tenant.id.0 == 0 {
            let next = tenants.keys().map(|id| id.0).max().unwrap_or(0) + 1;
            TenantId(next)
        } else {
            tenant.id
        };
        let mut created = tenant.clone();
        created.id = id;
        tenants.insert(id, created.clone());
        Ok(created)
    }

    async fn update(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let mut tenants = self
            .tenants
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !tenants.contains_key(&tenant.id) {
            return Err(TenancyError::NotFound(tenant.id.to_string()));
        }
        tenants.insert(tenant.id, tenant.clone());
        Ok(tenant.clone())
    }

    async fn delete(&self, id: TenantId) -> Result<bool, TenancyError> {
        let removed = self
            .tenants
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id)
            .is_some();
        Ok(removed)
    }

    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), TenancyError> {
        let mut tenants = self
            .tenants
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tenant = tenants
            .get_mut(&id)
            .ok_or_else(|| TenancyError::NotFound(id.to_string()))?;
        if value.encrypted {
            tenant.config.set_encrypted(key, value.value.clone());
        } else {
            tenant.config.set(key, value.value.clone());
        }
        Ok(())
    }
}
