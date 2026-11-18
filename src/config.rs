//! Tenancy configuration.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::resolver::RequestInfo;

/// Predicate letting selected requests bypass maintenance mode.
pub type MaintenanceBypass = Arc<dyn Fn(&RequestInfo) -> bool + Send + Sync>;

/// Which database layout the instance expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatabaseMode {
    /// Every tenant shares the central database and scopes by `tenant_id`.
    Single,
    /// Every tenant owns a separate database.
    Multi,
    /// Decided per tenant through [`crate::Tenant::database`] (default).
    #[default]
    PerTenant,
}

/// Behaviour tuning for a [`crate::Tenancy`] instance.
pub struct TenancyConfig {
    /// TTL applied by cached repository lookups.
    pub cache_ttl: Duration,
    /// Base directory for tenant storage.
    pub storage_root: PathBuf,
    /// Database layout expected by the instance.
    pub database_mode: DatabaseMode,
    /// Returns `true` to let a request bypass maintenance mode.
    pub maintenance_bypass: Option<MaintenanceBypass>,
    /// URL scheme used by [`crate::axum::tenant_url`].
    pub public_scheme: String,
    /// `Retry-After` sent with maintenance responses.
    pub maintenance_retry_after: Duration,
}

impl Default for TenancyConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(300),
            storage_root: PathBuf::from("storage"),
            database_mode: DatabaseMode::PerTenant,
            maintenance_bypass: None,
            public_scheme: "https".to_string(),
            maintenance_retry_after: Duration::from_secs(60),
        }
    }
}

impl std::fmt::Debug for TenancyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenancyConfig")
            .field("cache_ttl", &self.cache_ttl)
            .field("storage_root", &self.storage_root)
            .field("database_mode", &self.database_mode)
            .field("maintenance_bypass", &self.maintenance_bypass.is_some())
            .field("public_scheme", &self.public_scheme)
            .field("maintenance_retry_after", &self.maintenance_retry_after)
            .finish()
    }
}
