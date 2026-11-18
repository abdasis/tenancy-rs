//! Tenant domain model: identifier, domains, configuration and the tenant itself.

use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::TenancyError;
use crate::ports::Encryptor;

/// Stable tenant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TenantId(pub i64);

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Hostname owned by a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    /// Normalized host.
    pub host: String,
    /// Whether this is the primary domain.
    pub is_primary: bool,
}

impl Domain {
    /// Builds a domain, normalizing the host (lowercase, port and trailing dot removed).
    pub fn new(host: impl Into<String>, is_primary: bool) -> Self {
        Self {
            host: normalize_host(&host.into()),
            is_primary,
        }
    }

    /// Builds a primary domain.
    pub fn primary(host: impl Into<String>) -> Self {
        Self::new(host, true)
    }
}

/// Lowercases a host and strips its port and trailing dot.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    let without_port = match host.rsplit_once(':') {
        // shortcut: IPv6 literals without brackets are not handled, upgrade when IPv6 hosts are supported
        Some((name, port))
            if !name.contains(':')
                && !name.is_empty()
                && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            name.to_string()
        }
        _ => host,
    };
    without_port.trim_end_matches('.').to_string()
}

/// A configured tenant.
///
/// Project specific attributes live in [`Tenant::data`] so the crate stays
/// concrete instead of leaking generics through every adapter.
///
/// ```
/// use tenancy::Tenant;
///
/// let tenant = Tenant::new("Acme", "acme")?;
/// assert_eq!(tenant.slug, "acme");
/// assert_eq!(tenant.id, tenancy::TenantId(0));
/// # Ok::<_, tenancy::TenancyError>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// Identifier; `0` means unsaved.
    pub id: TenantId,
    /// Display name.
    pub name: String,
    /// Unique slug used in namespaces and database names.
    pub slug: String,
    /// Domains owned by the tenant.
    pub domains: Vec<Domain>,
    /// Database name for multi-DB tenancy; `None` means the central database.
    pub database: Option<String>,
    /// Maintenance flag checked by the axum layer.
    pub maintenance: bool,
    /// Tenant configuration.
    pub config: TenantConfig,
    /// Project specific attributes.
    pub data: serde_json::Map<String, serde_json::Value>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

impl Tenant {
    /// Builds an unsaved tenant (`id == TenantId(0)`) and validates its slug.
    pub fn new(name: impl Into<String>, slug: impl Into<String>) -> Result<Self, TenancyError> {
        let slug = slug.into();
        validate_slug(&slug)?;
        let now = Utc::now();
        Ok(Self {
            id: TenantId(0),
            name: name.into(),
            slug,
            domains: Vec::new(),
            database: None,
            maintenance: false,
            config: TenantConfig::default(),
            data: serde_json::Map::new(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Primary domain host, falling back to the first domain.
    pub fn primary_domain(&self) -> Option<&str> {
        self.domains
            .iter()
            .find(|domain| domain.is_primary)
            .or_else(|| self.domains.first())
            .map(|domain| domain.host.as_str())
    }

    /// Database name for this tenant: the explicit name when set, otherwise
    /// `prefix` plus the slug with `-` replaced by `_`.
    pub fn database_name(&self, prefix: &str) -> String {
        if let Some(database) = &self.database {
            return database.clone();
        }
        let candidate = format!("{prefix}{}", self.slug);
        let sanitized: String = candidate
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        debug_assert!(sanitized
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        sanitized
    }
}

/// Validates that a slug only contains `[a-z0-9-]` and is not empty.
pub fn validate_slug(slug: &str) -> Result<(), TenancyError> {
    let valid = !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(TenancyError::InvalidSlug(slug.to_string()))
    }
}

/// One stored configuration entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigValue {
    /// Stored value.
    pub value: String,
    /// Whether `value` holds sealed ciphertext.
    pub encrypted: bool,
}

/// Key-value configuration attached to a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantConfig(HashMap<String, ConfigValue>);

impl TenantConfig {
    /// Raw stored value for `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|entry| entry.value.as_str())
    }

    /// Stores a plaintext value.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(
            key.into(),
            ConfigValue {
                value: value.into(),
                encrypted: false,
            },
        );
    }

    /// Stores an already sealed (hex encoded ciphertext) value and marks it encrypted.
    pub fn set_encrypted(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(
            key.into(),
            ConfigValue {
                value: value.into(),
                encrypted: true,
            },
        );
    }

    /// Removes an entry.
    pub fn remove(&mut self, key: &str) -> Option<ConfigValue> {
        self.0.remove(key)
    }

    /// Iterates over stored entries.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ConfigValue)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Decrypts an entry with `encryptor`. Returns `Ok(None)` when absent.
    pub async fn get_decrypted(
        &self,
        key: &str,
        encryptor: &dyn Encryptor,
    ) -> Result<Option<String>, TenancyError> {
        let Some(entry) = self.0.get(key) else {
            return Ok(None);
        };
        if !entry.encrypted {
            return Ok(Some(entry.value.clone()));
        }
        let ciphertext = decode_hex(&entry.value)?;
        let plaintext = encryptor.open(&ciphertext).await?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|error| TenancyError::Crypto(error.to_string()))
    }
}

fn decode_hex(input: &str) -> Result<Vec<u8>, TenancyError> {
    if !input.len().is_multiple_of(2) {
        return Err(TenancyError::Crypto("odd hex length".to_string()));
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().chunks(2) {
        let pair =
            std::str::from_utf8(pair).map_err(|error| TenancyError::Crypto(error.to_string()))?;
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|error| TenancyError::Crypto(error.to_string()))?;
        out.push(byte);
    }
    Ok(out)
}
