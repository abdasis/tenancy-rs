//! Tenant scoped command helpers for workers and CLI entry points.

use std::future::Future;

use crate::error::TenancyError;
use crate::event::TenancyEvent;
use crate::manager::Tenancy;
use crate::tenant::{Tenant, TenantId};

/// Runs `f` with the tenant behind `id` active.
pub async fn run_for_tenant<T, F, Fut>(
    tenancy: &Tenancy,
    id: TenantId,
    f: F,
) -> Result<T, TenancyError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let tenant = tenancy
        .repository()
        .find(id)
        .await?
        .ok_or_else(|| TenancyError::NotFound(id.to_string()))?;
    tenancy.run(tenant, f).await
}

/// Runs `f` once per tenant, sequentially.
pub async fn run_for_all_tenants<T, F, Fut>(tenancy: &Tenancy, f: F) -> Result<Vec<T>, TenancyError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let tenants = tenancy.repository().all().await?;
    let mut results = Vec::with_capacity(tenants.len());
    for tenant in tenants {
        results.push(tenancy.run(tenant, &f).await?);
    }
    Ok(results)
}

#[cfg(feature = "postgres")]
fn databases(
    tenancy: &Tenancy,
) -> Result<std::sync::Arc<dyn crate::multi_db::TenantDatabase>, TenancyError> {
    tenancy.databases().ok_or_else(|| {
        TenancyError::Config("tenant database manager is not configured".to_string())
    })
}

#[cfg(feature = "postgres")]
/// Runs `migrator` on every tenant database.
pub async fn migrate_all(
    tenancy: &Tenancy,
    migrator: &sqlx::migrate::Migrator,
) -> Result<(), TenancyError> {
    let databases = databases(tenancy)?;
    for tenant in tenancy.repository().all().await? {
        databases.migrate(&tenant, migrator).await?;
    }
    Ok(())
}

#[cfg(feature = "postgres")]
/// Runs the host seed callback on every tenant database.
pub async fn seed_all(
    tenancy: &Tenancy,
    seed: &crate::multi_db::SeedFn,
) -> Result<(), TenancyError> {
    let databases = databases(tenancy)?;
    for tenant in tenancy.repository().all().await? {
        databases.seed(&tenant, seed).await?;
    }
    Ok(())
}

/// Resources removed alongside the tenant row. Every flag defaults to `false`;
/// the crate never drops a database, files, cache or sessions without it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Purge {
    /// Also drop the tenant database (requires the `postgres` feature).
    pub database: bool,
    /// Also delete `{slug}/` from tenant storage.
    pub storage: bool,
    /// Also delete cache keys under `tenant:{slug}:`.
    pub cache: bool,
    /// Also delete session keys under `tenant:{slug}:`.
    pub sessions: bool,
}

/// Deletes the tenant row, emits `TenantDeleted`, then purges opted-in resources.
pub async fn delete_tenant(
    tenancy: &Tenancy,
    id: TenantId,
    purge: Purge,
) -> Result<(), TenancyError> {
    let tenant = tenancy
        .repository()
        .find(id)
        .await?
        .ok_or_else(|| TenancyError::NotFound(id.to_string()))?;

    tenancy.emit(&TenancyEvent::TenantDeleted(id)).await?;
    tenancy.repository().delete(id).await?;

    purge_resources(tenancy, &tenant, purge).await
}

async fn purge_resources(
    tenancy: &Tenancy,
    tenant: &Tenant,
    purge: Purge,
) -> Result<(), TenancyError> {
    if purge.storage {
        let storage = tenancy
            .raw_storage()
            .ok_or_else(|| TenancyError::Config("storage is not configured".to_string()))?;
        storage.delete_prefix(&format!("{}/", tenant.slug)).await?;
    }
    if purge.cache {
        let cache = tenancy
            .raw_cache()
            .ok_or_else(|| TenancyError::Config("cache store is not configured".to_string()))?;
        cache
            .delete_prefix(&format!("tenant:{}:", tenant.slug))
            .await?;
    }
    if purge.sessions {
        let sessions = tenancy
            .raw_sessions()
            .ok_or_else(|| TenancyError::Config("session store is not configured".to_string()))?;
        sessions
            .delete_prefix(&format!("tenant:{}:", tenant.slug))
            .await?;
    }
    if purge.database {
        purge_database(tenancy, tenant).await?;
    }
    Ok(())
}

#[cfg(feature = "postgres")]
async fn purge_database(tenancy: &Tenancy, tenant: &Tenant) -> Result<(), TenancyError> {
    let databases = databases(tenancy)?;
    databases.drop_database(tenant).await
}

#[cfg(not(feature = "postgres"))]
async fn purge_database(_tenancy: &Tenancy, _tenant: &Tenant) -> Result<(), TenancyError> {
    Err(TenancyError::Config(
        "postgres feature is required to drop tenant databases".to_string(),
    ))
}
