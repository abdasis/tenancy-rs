//! Database pool bootstrapper (feature `postgres`).

use std::any::TypeId;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::bootstrapper::Bootstrapper;
use crate::context::TenantContext;
use crate::error::TenancyError;
use crate::multi_db::TenantDatabase;

/// Slot payload holding the pool active for the tenant.
#[derive(Clone)]
pub struct TenantPool(pub PgPool);

/// Type key of [`TenantPool`] inside the tenant context slot store.
pub const TENANT_POOL: TypeId = TypeId::of::<TenantPool>();

/// Installs a tenant pool for multi-DB tenants and the central pool otherwise.
pub struct DatabaseBootstrapper {
    central: PgPool,
    databases: Arc<dyn TenantDatabase>,
}

impl DatabaseBootstrapper {
    /// Creates the bootstrapper with a central pool and the tenant database manager.
    pub fn new(central: PgPool, databases: Arc<dyn TenantDatabase>) -> Self {
        Self { central, databases }
    }
}

#[async_trait]
impl Bootstrapper for DatabaseBootstrapper {
    async fn bootstrap(&self, context: &TenantContext) -> Result<(), TenancyError> {
        let tenant = context.tenant();
        let pool = if tenant.database.is_some() {
            self.databases.connect(tenant).await?
        } else {
            self.central.clone()
        };
        context.insert(TenantPool(pool));
        Ok(())
    }

    async fn revert(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        Ok(())
    }
}
