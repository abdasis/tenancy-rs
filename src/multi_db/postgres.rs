//! PostgreSQL tenant database registry (feature `postgres`).

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection, PgPool};

use crate::error::TenancyError;
use crate::multi_db::{DatabaseConfig, SeedFn, TenantDatabase};
use crate::tenant::{Tenant, TenantId};

/// [`TenantDatabase`] implementation keeping one pool per tenant.
///
/// shortcut: registry holds empty pools, add eviction when tenant count grows
pub struct PostgresTenantDatabase {
    config: DatabaseConfig,
    pools: RwLock<HashMap<TenantId, PgPool>>,
}

impl PostgresTenantDatabase {
    /// Creates the registry from `config`.
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            config,
            pools: RwLock::new(HashMap::new()),
        }
    }

    /// Configuration backing this registry.
    pub fn config(&self) -> &DatabaseConfig {
        &self.config
    }

    /// Number of pools currently registered.
    pub fn pool_count(&self) -> usize {
        self.pools
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    async fn admin_connection(&self) -> Result<PgConnection, TenancyError> {
        PgConnection::connect(&self.config.admin_url)
            .await
            .map_err(TenancyError::from)
    }
}

fn validate_database(name: &str) -> Result<&str, TenancyError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(TenancyError::Config(format!(
            "invalid database name: {name}"
        )));
    }
    Ok(name)
}

#[async_trait]
impl TenantDatabase for PostgresTenantDatabase {
    async fn connect(&self, tenant: &Tenant) -> Result<PgPool, TenancyError> {
        if let Some(pool) = self
            .pools
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&tenant.id)
            .cloned()
        {
            return Ok(pool);
        }

        let url = self.config.tenant_url(tenant)?;
        let pool = PgPoolOptions::new()
            .max_connections(self.config.pool_max_connections)
            .idle_timeout(self.config.idle_timeout)
            .connect(&url)
            .await?;
        self.pools
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(tenant.id, pool.clone());
        Ok(pool)
    }

    async fn create(&self, tenant: &Tenant) -> Result<(), TenancyError> {
        let name = self.config.database_name(tenant);
        let database = validate_database(&name)?;
        let mut connection = self.admin_connection().await?;
        // nama database sudah divalidasi, quote tetap dipakai agar aman
        sqlx::query(&format!("create database \"{database}\""))
            .execute(&mut connection)
            .await?;
        Ok(())
    }

    async fn drop_database(&self, tenant: &Tenant) -> Result<(), TenancyError> {
        let name = self.config.database_name(tenant);
        let database = validate_database(&name)?;
        let pool = self
            .pools
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&tenant.id);
        if let Some(pool) = pool {
            pool.close().await;
        }
        let mut connection = self.admin_connection().await?;
        sqlx::query(&format!("drop database if exists \"{database}\""))
            .execute(&mut connection)
            .await?;
        Ok(())
    }

    async fn migrate(&self, tenant: &Tenant, migrator: &Migrator) -> Result<(), TenancyError> {
        let pool = self.connect(tenant).await?;
        migrator
            .run(&pool)
            .await
            .map_err(|error| TenancyError::Repository(error.to_string()))?;
        Ok(())
    }

    async fn seed(&self, tenant: &Tenant, seed: &SeedFn) -> Result<(), TenancyError> {
        let pool = self.connect(tenant).await?;
        seed(tenant, &pool).await
    }
}
