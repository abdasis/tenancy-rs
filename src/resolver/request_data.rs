//! Resolvers reading request headers, cookies or a custom closure.

use async_trait::async_trait;

use crate::error::TenancyError;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};

/// Resolves the tenant slug from a request header.
pub struct HeaderResolver {
    name: String,
}

impl HeaderResolver {
    /// Creates the resolver; header names are matched case-insensitively.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
        }
    }
}

#[async_trait]
impl TenantResolver for HeaderResolver {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        Ok(request
            .header(&self.name)
            .filter(|value| !value.is_empty())
            .map(|value| TenantKey::Slug(value.to_string())))
    }
}

/// Resolves the tenant slug from a request cookie.
pub struct CookieResolver {
    name: String,
}

impl CookieResolver {
    /// Creates the resolver for the cookie `name`.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl TenantResolver for CookieResolver {
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        Ok(request
            .cookie(&self.name)
            .filter(|value| !value.is_empty())
            .map(|value| TenantKey::Slug(value.to_string())))
    }
}

/// Resolver backed by a host supplied closure.
pub struct ClosureResolver<F> {
    f: F,
}

impl<F> ClosureResolver<F>
where
    F: Fn(&RequestInfo) -> Option<TenantKey> + Send + Sync,
{
    /// Creates the resolver from `f`.
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

#[async_trait]
impl<F> TenantResolver for ClosureResolver<F>
where
    F: Fn(&RequestInfo) -> Option<TenantKey> + Send + Sync,
{
    async fn resolve(&self, request: &RequestInfo) -> Result<Option<TenantKey>, TenancyError> {
        Ok((self.f)(request))
    }
}
