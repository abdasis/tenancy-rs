//! Exact host resolver.

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};

/// Resolves the request host as a tenant domain.
#[derive(Default)]
pub struct DomainResolver;

impl DomainResolver {
    /// Creates the resolver.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TenantResolver for DomainResolver {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        if request.host.is_empty() {
            return Ok(None);
        }
        Ok(Some(TenantKey::Domain(request.host.clone())))
    }
}
