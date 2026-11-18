//! Tenant lifecycle events and listeners.

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::tenant::TenantId;

/// Lifecycle and administration events emitted by the crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenancyEvent {
    /// A tenant row was created.
    TenantCreated(TenantId),
    /// A tenant row was updated.
    TenantUpdated(TenantId),
    /// A tenant row was deleted.
    TenantDeleted(TenantId),
    /// Lifecycle is starting.
    Bootstrapping(TenantId),
    /// Lifecycle started.
    Bootstrapped(TenantId),
    /// Lifecycle is ending.
    Ending(TenantId),
    /// Lifecycle ended.
    Ended(TenantId),
    /// Maintenance mode was enabled.
    MaintenanceEnabled(TenantId),
    /// Maintenance mode was disabled.
    MaintenanceDisabled(TenantId),
}

/// Async listener invoked for every [`TenancyEvent`].
#[async_trait]
pub trait TenancyListener: Send + Sync {
    /// Handles one event; errors abort lifecycle events, others are logged.
    async fn handle(&self, event: &TenancyEvent) -> Result<(), TenancyError>;
}
