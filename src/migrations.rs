//! Central database schema for tenants, domains and configuration.

/// SQL creating the `tenants`, `tenant_domains` and `tenant_configs` tables.
pub const TENANCY_MIGRATION: &str = include_str!("../migrations/0001_tenancy.sql");
