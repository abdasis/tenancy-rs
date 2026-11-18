//! Per-tenant maintenance mode.

use chrono::Utc;

use crate::error::TenancyError;
use crate::event::TenancyEvent;
use crate::manager::Tenancy;
use crate::tenant::TenantId;

/// Flags the tenant as under maintenance and emits
/// [`TenancyEvent::MaintenanceEnabled`].
pub async fn put_in_maintenance(tenancy: &Tenancy, id: TenantId) -> Result<(), TenancyError> {
    set_maintenance(tenancy, id, true).await
}

/// Clears the maintenance flag and emits [`TenancyEvent::MaintenanceDisabled`].
pub async fn end_maintenance(tenancy: &Tenancy, id: TenantId) -> Result<(), TenancyError> {
    set_maintenance(tenancy, id, false).await
}

async fn set_maintenance(
    tenancy: &Tenancy,
    id: TenantId,
    maintenance: bool,
) -> Result<(), TenancyError> {
    let mut tenant = tenancy
        .repository()
        .find(id)
        .await?
        .ok_or_else(|| TenancyError::NotFound(id.to_string()))?;
    tenant.maintenance = maintenance;
    tenant.updated_at = Utc::now();
    tenancy.repository().update(&tenant).await?;

    let event = if maintenance {
        TenancyEvent::MaintenanceEnabled(id)
    } else {
        TenancyEvent::MaintenanceDisabled(id)
    };
    tenancy.emit(&event).await
}
