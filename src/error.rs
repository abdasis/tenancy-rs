//! Error type shared by every tenancy operation.

use thiserror::Error;

/// Error returned by tenancy operations.
///
/// `NotFound` means a lookup missed; `NotResolved` means no tenant could be
/// resolved for the current request.
#[derive(Debug, Error)]
pub enum TenancyError {
    #[error("not found: {0}")]
    /// Lookup missed.
    NotFound(String),
    #[error("tenant could not be resolved")]
    /// No tenant could be resolved for the request.
    NotResolved,
    #[error("tenant is in maintenance mode")]
    /// Tenant is under maintenance.
    Maintenance,
    #[error("invalid tenant slug: {0}")]
    /// Slug does not match `[a-z0-9-]`.
    InvalidSlug(String),
    #[error("configuration error: {0}")]
    /// Invalid configuration or missing port.
    Config(String),
    #[error("repository error: {0}")]
    /// Repository failure.
    Repository(String),
    #[error("bootstrapper error: {0}")]
    /// Bootstrapper failure.
    Bootstrapper(String),
    #[error("path traversal detected")]
    /// Path escapes the tenant storage root.
    PathTraversal,
    #[error("crypto error: {0}")]
    /// Encryption or token verification failure.
    Crypto(String),
    #[error("{0}")]
    /// Other failure.
    Other(String),
    #[cfg(feature = "postgres")]
    #[error("database error: {0}")]
    /// Database driver failure (feature `postgres`).
    Sqlx(#[from] sqlx::Error),
}
