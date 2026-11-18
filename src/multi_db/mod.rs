//! Multi-database tenancy: configuration plus the `postgres` contract.

use std::time::Duration;

use crate::error::TenancyError;
use crate::tenant::Tenant;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "postgres")]
pub use self::pg::{SeedFn, TenantDatabase};
#[cfg(feature = "postgres")]
pub use postgres::PostgresTenantDatabase;

#[cfg(feature = "postgres")]
mod pg {
    use std::future::Future;
    use std::pin::Pin;

    use async_trait::async_trait;
    use sqlx::migrate::Migrator;
    use sqlx::PgPool;

    use crate::error::TenancyError;
    use crate::tenant::Tenant;

    /// Async seed callback supplied by the host application.
    ///
    /// Receives the tenant and its connected pool so it can run SQL against the
    /// tenant database. Higher-ranked over the borrows so any `&Tenant` and
    /// `&PgPool` can be passed.
    pub type SeedFn = dyn for<'a> Fn(
            &'a Tenant,
            &'a PgPool,
        ) -> Pin<Box<dyn Future<Output = Result<(), TenancyError>> + Send + 'a>>
        + Send
        + Sync;

    /// Per-tenant database operations.
    ///
    /// `connect` must return the same pool for the same tenant; `create` and
    /// `drop_database` require an administrative connection.
    #[async_trait]
    pub trait TenantDatabase: Send + Sync {
        /// Returns the tenant pool, creating and caching it on first use.
        async fn connect(&self, tenant: &Tenant) -> Result<PgPool, TenancyError>;
        /// Creates the tenant database using the administrative connection.
        async fn create(&self, tenant: &Tenant) -> Result<(), TenancyError>;
        /// Drops the tenant database and closes its pool.
        async fn drop_database(&self, tenant: &Tenant) -> Result<(), TenancyError>;
        /// Runs `migrator` on the tenant database.
        async fn migrate(&self, tenant: &Tenant, migrator: &Migrator) -> Result<(), TenancyError>;
        /// Runs the host seed callback for the tenant.
        async fn seed(&self, tenant: &Tenant, seed: &SeedFn) -> Result<(), TenancyError>;
    }
}

/// Connection settings for per-tenant databases.
///
/// ```
/// use std::time::Duration;
///
/// use tenancy::DatabaseConfig;
///
/// let config = DatabaseConfig {
///     url_template: "postgres://localhost/{database}".to_string(),
///     admin_url: "postgres://localhost/postgres".to_string(),
///     database_prefix: "tenant_".to_string(),
///     pool_max_connections: 5,
///     idle_timeout: Duration::from_secs(60),
/// };
/// assert_eq!(config.database_name(&tenancy::Tenant::new("Acme", "acme")?), "tenant_acme");
/// # Ok::<_, tenancy::TenancyError>(())
/// ```
pub struct DatabaseConfig {
    /// Connection URL template containing the `{database}` placeholder.
    pub url_template: String,
    /// Administrative URL used for `CREATE`/`DROP DATABASE`.
    pub admin_url: String,
    /// Prefix applied to slug-derived database names.
    pub database_prefix: String,
    /// Maximum connections per tenant pool.
    pub pool_max_connections: u32,
    /// Idle timeout passed to the pool options.
    pub idle_timeout: Duration,
}

impl DatabaseConfig {
    /// Database name: explicit [`Tenant::database`] or prefix plus slug.
    pub fn database_name(&self, tenant: &Tenant) -> String {
        tenant.database_name(&self.database_prefix)
    }

    /// Tenant connection URL with `{database}` replaced.
    pub fn tenant_url(&self, tenant: &Tenant) -> Result<String, TenancyError> {
        if !self.url_template.contains("{database}") {
            return Err(TenancyError::Config(
                "url_template must contain the {database} placeholder".to_string(),
            ));
        }
        Ok(self
            .url_template
            .replace("{database}", &self.database_name(tenant)))
    }
}
