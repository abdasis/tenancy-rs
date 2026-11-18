//! Subdomain resolver for central domains.

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};
use crate::tenant::normalize_host;

/// Resolves one label below a central domain (`springfield.example.com` ->
/// `springfield`). Deep subdomains and `www` are ignored.
pub struct SubdomainResolver {
    central_domains: Vec<String>,
}

impl SubdomainResolver {
    /// Creates the resolver for `central_domains`.
    pub fn new(central_domains: Vec<String>) -> Self {
        Self {
            central_domains: central_domains
                .into_iter()
                .map(|domain| normalize_host(&domain))
                .collect(),
        }
    }
}

#[async_trait]
impl TenantResolver for SubdomainResolver {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        let host = request.host.as_str();
        for central in &self.central_domains {
            let Some(label) = host.strip_suffix(&format!(".{central}")) else {
                continue;
            };
            if !label.is_empty() && !label.contains('.') && label != "www" {
                return Ok(Some(TenantKey::Slug(label.to_string())));
            }
        }
        Ok(None)
    }
}
