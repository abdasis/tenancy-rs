//! Tenant resolution from incoming requests.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::tenant::{normalize_host, TenantId};

pub mod chain;
pub mod domain;
pub mod path;
pub mod request_data;
pub mod subdomain;

pub use chain::ResolverChain;
pub use domain::DomainResolver;
pub use path::PathResolver;
pub use request_data::{ClosureResolver, CookieResolver, HeaderResolver};
pub use subdomain::SubdomainResolver;

/// What a resolver found; the manager uses it for the repository lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenantKey {
    /// Lookup by identifier.
    Id(TenantId),
    /// Lookup by slug.
    Slug(String),
    /// Lookup by host.
    Domain(String),
}

/// Minimal request view needed for tenant resolution.
#[derive(Debug, Clone, Default)]
pub struct RequestInfo {
    /// HTTP method.
    pub method: String,
    /// Request path, query excluded.
    pub path: String,
    /// Normalized host (lowercase, no port).
    pub host: String,
    /// Headers keyed by lowercase name.
    pub headers: HashMap<String, String>,
    /// Cookies keyed by name.
    pub cookies: HashMap<String, String>,
    /// Query parameters keyed by name.
    pub query: HashMap<String, String>,
}

impl RequestInfo {
    /// Builds a request view, normalizing the host.
    pub fn new(method: impl Into<String>, path: impl Into<String>, host: &str) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            host: normalize_host(host),
            headers: HashMap::new(),
            cookies: HashMap::new(),
            query: HashMap::new(),
        }
    }

    /// Header lookup, case-insensitive.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Cookie lookup.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    /// Query parameter lookup.
    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    /// Adds a header, normalizing its name to lowercase.
    pub fn with_header(mut self, name: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.as_ref().to_ascii_lowercase(), value.into());
        self
    }
}

/// Resolves a request into a tenant key.
///
/// Resolvers return `Ok(None)` when the request does not match; hard failures
/// are returned as `Err`.
#[async_trait]
pub trait TenantResolver: Send + Sync {
    /// Returns the matched key, `Ok(None)` when the request does not match.
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError>;
}
