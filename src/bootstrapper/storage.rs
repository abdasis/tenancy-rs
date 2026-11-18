//! Storage root bootstrapper.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::bootstrapper::Bootstrapper;
use crate::context::TenantContext;
use crate::error::TenancyError;

/// Points the tenant storage root at `root/{slug}`.
///
/// No directory is created here; the first write creates it.
pub struct StorageBootstrapper {
    root: PathBuf,
}

impl StorageBootstrapper {
    /// Creates the bootstrapper rooted at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl Bootstrapper for StorageBootstrapper {
    async fn bootstrap(&self, context: &TenantContext) -> Result<(), TenancyError> {
        context.set_storage_root(self.root.join(&context.tenant().slug));
        Ok(())
    }

    async fn revert(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        Ok(())
    }
}
