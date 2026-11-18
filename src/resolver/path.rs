//! Path prefix resolver (`/t/{slug}/...`).

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};

const PREFIX: &str = "/t/";

/// Resolves the first path segment after the `/t/` prefix as a tenant slug.
#[derive(Default)]
pub struct PathResolver;

impl PathResolver {
    /// Creates the resolver.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TenantResolver for PathResolver {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        let Some(rest) = request.path.strip_prefix(PREFIX) else {
            return Ok(None);
        };
        let slug = rest.split('/').next().unwrap_or_default();
        if slug.is_empty() {
            return Ok(None);
        }
        Ok(Some(TenantKey::Slug(slug.to_string())))
    }
}
