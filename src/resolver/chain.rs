//! First-match resolver chain.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};

/// Runs resolvers in order and stops at the first one that resolves a tenant.
#[derive(Default)]
pub struct ResolverChain {
    resolvers: Vec<Arc<dyn TenantResolver>>,
}

impl ResolverChain {
    /// Creates a chain from `resolvers`.
    pub fn new(resolvers: Vec<Arc<dyn TenantResolver>>) -> Self {
        Self { resolvers }
    }

    /// Appends a resolver at the end of the chain.
    pub fn push(&mut self, resolver: Arc<dyn TenantResolver>) {
        self.resolvers.push(resolver);
    }
}

#[async_trait]
impl TenantResolver for ResolverChain {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        for resolver in &self.resolvers {
            if let Some(key) = resolver.resolve(request).await? {
                return Ok(Some(key));
            }
        }
        Ok(None)
    }
}
