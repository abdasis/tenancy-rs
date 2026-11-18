//! Single-database scoping helpers. Compiled with the `postgres` feature.
//!
//! Scoping is explicit and opt-in: "without tenancy" simply means not calling
//! these helpers, which avoids implicit scopes leaking into central-admin
//! queries.

use sqlx::postgres::Postgres;
use sqlx::{Encode, PgPool, QueryBuilder, Type};

use crate::error::TenancyError;
use crate::tenant::Tenant;

#[cfg(feature = "postgres-rls")]
pub mod rls;

/// Marker trait for models scoped by a tenant column.
pub trait TenantScoped {
    /// Column holding the tenant id; override for a custom model.
    const TENANT_COLUMN: &'static str = "tenant_id";
}

/// Appends a tenant predicate bound to [`Tenant::id`] using the default
/// `tenant_id` column.
///
/// The caller decides where the predicate sits (`where` or `and`).
pub fn scope_query(
    query: &mut QueryBuilder<'_, Postgres>,
    tenant: &Tenant,
    alias: Option<&'static str>,
) {
    scope_query_column(query, tenant, alias, "tenant_id");
}

/// Same as [`scope_query`] but uses the column declared by a [`TenantScoped`] model.
pub fn scope_query_for<M: TenantScoped>(
    query: &mut QueryBuilder<'_, Postgres>,
    tenant: &Tenant,
    alias: Option<&'static str>,
) {
    scope_query_column(query, tenant, alias, M::TENANT_COLUMN);
}

fn scope_query_column(
    query: &mut QueryBuilder<'_, Postgres>,
    tenant: &Tenant,
    alias: Option<&'static str>,
    column: &'static str,
) {
    // shortcut: identifier must be a static trusted name, never user input
    let predicate = match alias {
        Some(alias) => format!("\"{alias}\".\"{column}\" = "),
        None => format!("\"{column}\" = "),
    };
    query.push(predicate);
    query.push_bind(tenant.id.0);
}

/// Returns `true` when `value` is still free for `column` inside the tenant
/// (the semantics of Laravel's `unique` validation rule).
pub async fn tenant_unique<V>(
    pool: &PgPool,
    tenant: &Tenant,
    table: &'static str,
    column: &'static str,
    value: V,
) -> Result<bool, TenancyError>
where
    V: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
{
    Ok(!value_exists(pool, tenant, table, column, value).await?)
}

/// Returns `true` when `value` already exists for `column` inside the tenant.
pub async fn tenant_exists<V>(
    pool: &PgPool,
    tenant: &Tenant,
    table: &'static str,
    column: &'static str,
    value: V,
) -> Result<bool, TenancyError>
where
    V: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
{
    value_exists(pool, tenant, table, column, value).await
}

async fn value_exists<V>(
    pool: &PgPool,
    tenant: &Tenant,
    table: &'static str,
    column: &'static str,
    value: V,
) -> Result<bool, TenancyError>
where
    V: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
{
    // shortcut: identifier must be a static trusted name, never user input
    let sql = format!(
        "select exists(select 1 from \"{table}\" where \"{column}\" = $1 and \"tenant_id\" = $2)"
    );
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(value)
        .bind(tenant.id.0)
        .fetch_one(pool)
        .await?;
    Ok(exists)
}
