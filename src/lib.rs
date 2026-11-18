//! Multi-tenant primitives for Rust services.
//!
//! The core is free of web-framework and database dependencies. Adapters are
//! opt-in through features: `axum`, `postgres`, `postgres-rls`, `redis`,
//! `encryption`, `impersonation`, `testing`.
//!
//! Current tenant lives in a Tokio task-local, never in process global state.

#![warn(missing_docs)]

#[cfg(feature = "axum")]
pub mod axum;
pub mod bootstrapper;
pub mod commands;
pub mod config;
pub mod context;
#[cfg(feature = "encryption")]
pub mod encryption;
pub mod error;
pub mod event;
#[cfg(feature = "impersonation")]
pub mod impersonation;
pub mod maintenance;
pub mod manager;
pub mod migrations;
pub mod multi_db;
#[cfg(feature = "postgres")]
pub use multi_db::{SeedFn, TenantDatabase};
pub mod ports;
pub mod repository;
pub mod resolver;
#[cfg(feature = "postgres")]
pub mod single_db;
pub mod tenant;
#[cfg(feature = "testing")]
pub mod testing;

#[cfg(feature = "encryption")]
pub use encryption::AesGcmEncryptor;

pub use bootstrapper::Bootstrapper;
pub use commands::Purge;
pub use config::{DatabaseMode, MaintenanceBypass, TenancyConfig};
pub use context::{current_context, TenantContext};
pub use error::TenancyError;
pub use event::{TenancyEvent, TenancyListener};
#[cfg(feature = "impersonation")]
pub use impersonation::{
    issue_token, start_impersonation, stop_impersonation, verify_token, ImpersonationClaims,
};
pub use manager::{Tenancy, TenancyBuilder, TenancyHandle};
pub use migrations::TENANCY_MIGRATION;
pub use multi_db::DatabaseConfig;
pub use ports::{
    CacheStore, Encryptor, JobQueue, Loader, ScopedCache, ScopedSession, ScopedStorage,
    SessionStore, Storage, TenantJob,
};
pub use repository::TenantRepository;
pub use resolver::{RequestInfo, TenantKey, TenantResolver};
pub use tenant::{
    normalize_host, validate_slug, ConfigValue, Domain, Tenant, TenantConfig, TenantId,
};
