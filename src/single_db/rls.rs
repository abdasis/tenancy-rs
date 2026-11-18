//! Row level security transaction helper (feature `postgres-rls`).
//!
//! RLS requires a non-superuser role and `FORCE ROW LEVEL SECURITY` on the
//! tables; the crate does not enable those policies for you.

use std::ops::{Deref, DerefMut};

use sqlx::{PgConnection, PgPool, Postgres, Transaction};

use crate::error::TenancyError;
use crate::tenant::Tenant;

/// Transaction with `app.tenant_id` set locally for the current transaction.
pub struct TenantTransaction<'a> {
    tx: Transaction<'a, Postgres>,
}

impl<'a> TenantTransaction<'a> {
    /// Underlying connection, for running scoped statements.
    pub fn connection(&mut self) -> &mut PgConnection {
        self.tx.as_mut()
    }
}

impl<'a> Deref for TenantTransaction<'a> {
    type Target = Transaction<'a, Postgres>;

    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

impl DerefMut for TenantTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tx
    }
}

/// Begins a transaction and sets `app.tenant_id` for its duration.
pub async fn begin_tenant_tx<'a>(
    pool: &'a PgPool,
    tenant: &Tenant,
) -> Result<TenantTransaction<'a>, TenancyError> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.tenant_id', $1, true)")
        .bind(tenant.id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(TenantTransaction { tx })
}
