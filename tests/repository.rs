use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tenancy::ports::memory::MemoryCache;
use tenancy::repository::cached::CachedTenantRepository;
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::{
    ConfigValue, Domain, Tenant, TenantId, TenantKey, TenantRepository, TENANCY_MIGRATION,
};

fn tenant(id: i64, slug: &str, domain: &str) -> Tenant {
    let mut tenant = Tenant::new("Acme", slug).expect("valid tenant");
    tenant.id = TenantId(id);
    tenant.domains.push(Domain::primary(domain));
    tenant
}

#[tokio::test]
async fn memory_repository_finds_by_id_slug_and_domain() {
    let repository = MemoryTenantRepository::new();
    repository.insert(tenant(1, "acme", "acme.example.com"));

    let found = repository
        .find(TenantId(1))
        .await
        .expect("find")
        .expect("tenant");
    assert_eq!(found.slug, "acme");

    let found = repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("find by slug")
        .expect("tenant");
    assert_eq!(found.id, TenantId(1));

    let found = repository
        .find_by_key(&TenantKey::Domain("ACME.example.com:8443".to_string()))
        .await
        .expect("find by domain")
        .expect("tenant");
    assert_eq!(found.id, TenantId(1));

    assert_eq!(repository.all().await.expect("all").len(), 1);
    assert!(repository.delete(TenantId(1)).await.expect("delete"));
    assert!(!repository.delete(TenantId(1)).await.expect("delete again"));
    assert!(repository.find(TenantId(1)).await.expect("find").is_none());
}

#[tokio::test]
async fn memory_repository_create_assigns_ids_and_updates() {
    let repository = MemoryTenantRepository::new();
    let mut draft = tenant(0, "acme", "acme.example.com");
    let created = repository.create(&draft).await.expect("create");
    assert_ne!(created.id, TenantId(0));

    draft.id = created.id;
    draft.name = "Acme Updated".to_string();
    let updated = repository.update(&draft).await.expect("update");
    assert_eq!(updated.name, "Acme Updated");

    let missing = tenant(404, "missing", "missing.example.com");
    assert!(matches!(
        repository.update(&missing).await,
        Err(tenancy::TenancyError::NotFound(_))
    ));
}

struct CountingRepository {
    inner: MemoryTenantRepository,
    lookups: AtomicUsize,
}

impl CountingRepository {
    fn new() -> Self {
        Self {
            inner: MemoryTenantRepository::new(),
            lookups: AtomicUsize::new(0),
        }
    }

    fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TenantRepository for CountingRepository {
    async fn all(&self) -> Result<Vec<Tenant>, tenancy::TenancyError> {
        self.inner.all().await
    }

    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, tenancy::TenancyError> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        self.inner.find(id).await
    }

    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, tenancy::TenancyError> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        self.inner.find_by_key(key).await
    }

    async fn create(&self, tenant: &Tenant) -> Result<Tenant, tenancy::TenancyError> {
        self.inner.create(tenant).await
    }

    async fn update(&self, tenant: &Tenant) -> Result<Tenant, tenancy::TenancyError> {
        self.inner.update(tenant).await
    }

    async fn delete(&self, id: TenantId) -> Result<bool, tenancy::TenancyError> {
        self.inner.delete(id).await
    }

    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), tenancy::TenancyError> {
        self.inner.save_config(id, key, value).await
    }
}

fn cached(inner: Arc<CountingRepository>, cache: Arc<MemoryCache>) -> CachedTenantRepository {
    CachedTenantRepository::new(inner, cache, Duration::from_secs(60))
}

#[tokio::test]
async fn cached_repository_skips_inner_on_second_lookup() {
    let inner = Arc::new(CountingRepository::new());
    inner.inner.insert(tenant(1, "acme", "acme.example.com"));
    let repository = cached(inner.clone(), Arc::new(MemoryCache::new()));

    let first = repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("first lookup");
    let second = repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("second lookup");

    assert_eq!(first.map(|tenant| tenant.id), Some(TenantId(1)));
    assert_eq!(second.map(|tenant| tenant.id), Some(TenantId(1)));
    assert_eq!(inner.lookups(), 1, "second lookup must hit the cache");
}

#[tokio::test]
async fn cached_repository_invalidates_old_identity_on_update() {
    let inner = Arc::new(CountingRepository::new());
    inner.inner.insert(tenant(1, "acme", "acme.example.com"));
    let repository = cached(inner, Arc::new(MemoryCache::new()));

    // isi cache dengan identitas lama
    assert!(repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("warm")
        .is_some());
    assert!(repository
        .find_by_key(&TenantKey::Domain("acme.example.com".to_string()))
        .await
        .expect("warm")
        .is_some());

    let mut updated = tenant(1, "acme", "acme.example.com");
    updated.slug = "acme-two".to_string();
    updated.domains = vec![Domain::primary("acme-two.example.com")];
    repository.update(&updated).await.expect("update");

    assert!(repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("old slug")
        .is_none());
    assert!(repository
        .find_by_key(&TenantKey::Domain("acme.example.com".to_string()))
        .await
        .expect("old domain")
        .is_none());

    let current = repository
        .find_by_key(&TenantKey::Slug("acme-two".to_string()))
        .await
        .expect("new slug")
        .expect("tenant");
    assert_eq!(current.slug, "acme-two");

    let by_id = repository
        .find(TenantId(1))
        .await
        .expect("by id")
        .expect("tenant");
    assert_eq!(by_id.slug, "acme-two");
}

#[tokio::test]
async fn cached_repository_invalidates_on_delete() {
    let inner = Arc::new(CountingRepository::new());
    inner.inner.insert(tenant(1, "acme", "acme.example.com"));
    let repository = cached(inner, Arc::new(MemoryCache::new()));

    assert!(repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("warm")
        .is_some());
    assert!(repository.delete(TenantId(1)).await.expect("delete"));
    assert!(repository
        .find_by_key(&TenantKey::Slug("acme".to_string()))
        .await
        .expect("after delete")
        .is_none());
}

#[test]
fn migration_defines_core_tables() {
    for table in ["tenants", "tenant_domains", "tenant_configs"] {
        assert!(
            TENANCY_MIGRATION.contains(table),
            "migration must create {table}"
        );
    }
}

#[cfg(feature = "postgres")]
mod postgres {
    use super::*;
    use tenancy::repository::postgres::PgTenantRepository;

    async fn pool() -> Option<sqlx::PgPool> {
        match std::env::var("TEST_DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect TEST_DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
                None
            }
        }
    }

    #[tokio::test]
    async fn postgres_repository_roundtrip() {
        let Some(pool) = pool().await else { return };
        sqlx::raw_sql(TENANCY_MIGRATION)
            .execute(&pool)
            .await
            .expect("migrate tenancy schema");

        let repository = PgTenantRepository::new(pool.clone());
        let slug = format!("acme-{}", std::process::id());
        let mut draft = tenant(0, &slug, &format!("{slug}.example.com"));
        draft.config.set("timezone", "Asia/Jakarta");

        let created = repository.create(&draft).await.expect("create");
        assert_ne!(created.id, TenantId(0));
        assert_eq!(created.config.get("timezone"), Some("Asia/Jakarta"));

        let by_slug = repository
            .find_by_key(&TenantKey::Slug(slug.clone()))
            .await
            .expect("find by slug")
            .expect("tenant");
        assert_eq!(by_slug.id, created.id);

        let by_domain = repository
            .find_by_key(&TenantKey::Domain(format!("{slug}.example.com")))
            .await
            .expect("find by domain")
            .expect("tenant");
        assert_eq!(by_domain.id, created.id);

        repository
            .save_config(
                created.id,
                "smtp_password",
                &ConfigValue {
                    value: "ciphertext".to_string(),
                    encrypted: true,
                },
            )
            .await
            .expect("save config");

        let reloaded = repository
            .find(created.id)
            .await
            .expect("find")
            .expect("tenant");
        assert_eq!(reloaded.config.get("smtp_password"), Some("ciphertext"));

        let mut updated = reloaded.clone();
        updated.name = "Acme Updated".to_string();
        repository.update(&updated).await.expect("update");
        assert_eq!(
            repository
                .find(created.id)
                .await
                .expect("find")
                .expect("tenant")
                .name,
            "Acme Updated"
        );

        assert!(repository.delete(created.id).await.expect("delete"));
        assert!(repository.find(created.id).await.expect("find").is_none());
    }
}
