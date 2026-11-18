use std::time::Duration;

use tenancy::{DatabaseMode, TenancyConfig};

#[test]
fn defaults_are_production_sane() {
    let config = TenancyConfig::default();

    assert_eq!(config.cache_ttl, Duration::from_secs(300));
    assert_eq!(config.database_mode, DatabaseMode::PerTenant);
    assert!(config.maintenance_bypass.is_none());
    assert_eq!(config.public_scheme, "https");
    assert_eq!(config.maintenance_retry_after, Duration::from_secs(60));
}

#[test]
fn fields_can_be_overridden() {
    let config = TenancyConfig {
        public_scheme: "http".to_string(),
        maintenance_retry_after: Duration::from_secs(120),
        ..TenancyConfig::default()
    };

    assert_eq!(config.public_scheme, "http");
    assert_eq!(config.maintenance_retry_after, Duration::from_secs(120));
}
