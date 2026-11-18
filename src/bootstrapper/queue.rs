//! Queue namespace bootstrapper.

use async_trait::async_trait;

use crate::bootstrapper::Bootstrapper;
use crate::context::TenantContext;
use crate::error::TenancyError;

/// Sets the queue namespace from the tenant slug.
#[derive(Default)]
pub struct QueueBootstrapper;

impl QueueBootstrapper {
    /// Creates the bootstrapper.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Bootstrapper for QueueBootstrapper {
    async fn bootstrap(&self, context: &TenantContext) -> Result<(), TenancyError> {
        context.set_queue_namespace(format!("tenant:{}", context.tenant().slug));
        Ok(())
    }

    async fn revert(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        Ok(())
    }
}
