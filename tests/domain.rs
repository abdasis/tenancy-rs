use tenancy::{Domain, TenancyError, Tenant, TenantConfig, TenantId};

#[test]
fn valid_slug_is_accepted() {
    let tenant = Tenant::new("Springfield High", "springfield-high").expect("valid tenant");
    assert_eq!(tenant.slug, "springfield-high");
    assert_eq!(tenant.name, "Springfield High");
    assert_eq!(tenant.id, TenantId(0));
    assert!(tenant.domains.is_empty());
    assert!(!tenant.maintenance);
}

#[test]
fn invalid_slugs_are_rejected() {
    assert!(matches!(
        Tenant::new("Springfield High", "Springfield High"),
        Err(TenancyError::InvalidSlug(_))
    ));
    assert!(matches!(
        Tenant::new("X", ""),
        Err(TenancyError::InvalidSlug(_))
    ));
}

#[test]
fn tenant_id_displays_as_number() {
    assert_eq!(TenantId(7).to_string(), "7");
}

#[test]
fn database_name_replaces_dashes_with_underscores() {
    let tenant = Tenant::new("Springfield High", "springfield-high").expect("valid tenant");
    assert_eq!(tenant.database_name("tenant_"), "tenant_springfield_high");
}

#[test]
fn database_name_prefers_explicit_database() {
    let mut tenant = Tenant::new("Springfield High", "springfield-high").expect("valid tenant");
    tenant.database = Some("custom_db".to_string());
    assert_eq!(tenant.database_name("tenant_"), "custom_db");
}

#[test]
fn primary_domain_prefers_primary_flag() {
    let mut tenant = Tenant::new("Springfield High", "springfield-high").expect("valid tenant");
    assert_eq!(tenant.primary_domain(), None);

    tenant
        .domains
        .push(Domain::new("secondary.example.com", false));
    tenant
        .domains
        .push(Domain::new("Primary.Example.com:8443", true));

    assert_eq!(tenant.primary_domain(), Some("primary.example.com"));
}

#[test]
fn domain_normalizes_host() {
    let domain = Domain::new("Springfield.Example.COM:443", true);
    assert_eq!(domain.host, "springfield.example.com");
    assert!(domain.is_primary);
}

#[test]
fn tenant_config_stores_plain_and_encrypted_values() {
    let mut config = TenantConfig::default();
    config.set("timezone", "Asia/Jakarta");
    config.set_encrypted("smtp_password", "ciphertext");

    assert_eq!(config.get("timezone"), Some("Asia/Jakarta"));
    assert_eq!(config.get("smtp_password"), Some("ciphertext"));

    let encrypted = config.iter().find(|(key, _)| *key == "smtp_password");
    assert_eq!(encrypted.map(|(_, value)| value.encrypted), Some(true));

    let plain = config.iter().find(|(key, _)| *key == "timezone");
    assert_eq!(plain.map(|(_, value)| value.encrypted), Some(false));

    assert!(config.remove("timezone").is_some());
    assert_eq!(config.get("timezone"), None);
}
