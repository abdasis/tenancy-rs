//! Tenant persistence contract.

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::TenantKey;
use crate::tenant::{ConfigValue, Tenant, TenantId};

pub mod cached;
pub mod memory;
#[cfg(feature = "postgres")]
pub mod postgres;

pub use cached::CachedTenantRepository;
pub use memory::MemoryTenantRepository;

/// Storage backend for tenants, domains and configuration.
///
/// Implementations do not emit [`crate::event::TenancyEvent`]; the manager does,
/// so a repository can be swapped without losing events.
#[async_trait]
pub trait TenantRepository: Send + Sync {
    /// Returns every tenant.
    async fn all(&self) -> Result<Vec<Tenant>, TenancyError>;

    /// Finds a tenant by identifier.
    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, TenancyError>;

    /// Finds a tenant by identifier, slug or domain.
    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, TenancyError>;

    /// Persists a new tenant and returns it with its assigned id.
    async fn create(&self, tenant: &Tenant) -> Result<Tenant, TenancyError>;

    /// Replaces an existing tenant.
    async fn update(&self, tenant: &Tenant) -> Result<Tenant, TenancyError>;

    /// Deletes a tenant, returning whether a row was removed.
    async fn delete(&self, id: TenantId) -> Result<bool, TenancyError>;

    /// Upserts one configuration entry.
    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), TenancyError>;
}
