//! Bootstrapper contract: hooks executed when a tenant lifecycle starts and ends.

use async_trait::async_trait;

use crate::context::TenantContext;
use crate::error::TenancyError;

pub mod cache;
#[cfg(feature = "postgres")]
pub mod database;
pub mod queue;
pub mod session;
pub mod storage;

pub use cache::CacheBootstrapper;
#[cfg(feature = "postgres")]
pub use database::{DatabaseBootstrapper, TenantPool, TENANT_POOL};
pub use queue::QueueBootstrapper;
pub use session::SessionBootstrapper;
pub use storage::StorageBootstrapper;

/// Hook that writes tenant scoped state into [`TenantContext`].
///
/// Implementations must only touch the context (no process global state) so a
/// skipped revert after a cancelled request leaks nothing.
#[async_trait]
pub trait Bootstrapper: Send + Sync {
    /// Called when a tenant lifecycle starts; only writes tenant scoped state into `context`.
    async fn bootstrap(&self, context: &TenantContext) -> Result<(), TenancyError>;
    /// Called on teardown, in reverse registration order.
    async fn revert(&self, context: &TenantContext) -> Result<(), TenancyError>;
}
