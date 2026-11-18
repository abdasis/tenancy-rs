use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tenancy::commands::{delete_tenant, Purge};
use tenancy::maintenance::{end_maintenance, put_in_maintenance};
use tenancy::ports::memory::{MemoryCache, MemorySessionStore};
use tenancy::repository::cached::CachedTenantRepository;
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::{
    CacheStore, ConfigValue, Domain, SessionStore, Storage, Tenancy, TenancyError, TenancyEvent,
    TenancyListener, Tenant, TenantId, TenantKey, TenantRepository,
};

fn tenant(id: i64, slug: &str) -> Tenant {
    let mut tenant = Tenant::new("Acme", slug).expect("valid tenant");
    tenant.id = TenantId(id);
    tenant
        .domains
        .push(Domain::primary(format!("{slug}.example.com")));
    tenant
}

#[derive(Clone, Default)]
struct EventLog(Arc<Mutex<Vec<TenancyEvent>>>);

impl EventLog {
    fn events(&self) -> Vec<TenancyEvent> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct RecordingListener {
    log: EventLog,
}

impl RecordingListener {
    fn new(log: EventLog) -> Self {
        Self { log }
    }
}

#[async_trait]
impl TenancyListener for RecordingListener {
    async fn handle(&self, event: &TenancyEvent) -> Result<(), TenancyError> {
        self.log
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event.clone());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<String>>>);

impl Calls {
    fn record(&self, kind: &str, prefix: &str) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(format!("{kind}:{prefix}"));
    }

    fn entries(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct CountingRepository {
    inner: MemoryTenantRepository,
    finds: AtomicUsize,
    deletes: AtomicUsize,
    saves: AtomicUsize,
}

impl CountingRepository {
    fn new() -> Self {
        Self {
            inner: MemoryTenantRepository::new(),
            finds: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
            saves: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl TenantRepository for CountingRepository {
    async fn all(&self) -> Result<Vec<Tenant>, TenancyError> {
        self.inner.all().await
    }

    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, TenancyError> {
        self.finds.fetch_add(1, Ordering::SeqCst);
        self.inner.find(id).await
    }

    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, TenancyError> {
        self.finds.fetch_add(1, Ordering::SeqCst);
        self.inner.find_by_key(key).await
    }

    async fn create(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        self.inner.create(tenant).await
    }

    async fn update(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        self.inner.update(tenant).await
    }

    async fn delete(&self, id: TenantId) -> Result<bool, TenancyError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.inner.delete(id).await
    }

    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), TenancyError> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.inner.save_config(id, key, value).await
    }
}

struct CountingCache {
    inner: MemoryCache,
    calls: Calls,
}

#[async_trait]
impl CacheStore for CountingCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        self.inner.get(key).await
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError> {
        self.inner.set(key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        self.inner.delete(key).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.calls.record("cache", prefix);
        self.inner.delete_prefix(prefix).await
    }
}

struct CountingSession {
    inner: MemorySessionStore,
    calls: Calls,
}

#[async_trait]
impl SessionStore for CountingSession {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        self.inner.get(key).await
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<(), TenancyError> {
        self.inner.set(key, value).await
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        self.inner.delete(key).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.calls.record("session", prefix);
        self.inner.delete_prefix(prefix).await
    }
}

struct CountingStorage {
    calls: Calls,
}

#[async_trait]
impl Storage for CountingStorage {
    async fn put(&self, _path: &str, _data: Vec<u8>) -> Result<(), TenancyError> {
        Ok(())
    }

    async fn get(&self, _path: &str) -> Result<Vec<u8>, TenancyError> {
        Ok(Vec::new())
    }

    async fn exists(&self, _path: &str) -> Result<bool, TenancyError> {
        Ok(false)
    }

    async fn delete(&self, _path: &str) -> Result<(), TenancyError> {
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.calls.record("storage", prefix);
        Ok(())
    }
}

#[tokio::test]
async fn maintenance_toggles_flag_and_emits_events() {
    let repository = Arc::new(MemoryTenantRepository::new());
    repository.insert(tenant(1, "acme"));
    let log = EventLog::default();
    let tenancy = Tenancy::builder()
        .repository(repository.clone())
        .listener(Arc::new(RecordingListener::new(log.clone())))
        .build()
        .expect("tenancy");

    put_in_maintenance(&tenancy, TenantId(1))
        .await
        .expect("maintenance on");
    let stored = repository
        .find(TenantId(1))
        .await
        .expect("find")
        .expect("tenant");
    assert!(stored.maintenance);
    assert_eq!(
        log.events(),
        [TenancyEvent::MaintenanceEnabled(TenantId(1))]
    );

    end_maintenance(&tenancy, TenantId(1))
        .await
        .expect("maintenance off");
    let stored = repository
        .find(TenantId(1))
        .await
        .expect("find")
        .expect("tenant");
    assert!(!stored.maintenance);
    assert_eq!(
        log.events(),
        [
            TenancyEvent::MaintenanceEnabled(TenantId(1)),
            TenancyEvent::MaintenanceDisabled(TenantId(1))
        ]
    );
}

#[tokio::test]
async fn create_and_update_tenant_emit_events() {
    let log = EventLog::default();
    let tenancy = Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .listener(Arc::new(RecordingListener::new(log.clone())))
        .build()
        .expect("tenancy");

    let created = tenancy
        .create_tenant(tenant(0, "acme"))
        .await
        .expect("create");
    assert_ne!(created.id, TenantId(0));

    let mut updated = created.clone();
    updated.name = "Acme Updated".to_string();
    tenancy.update_tenant(updated).await.expect("update");

    assert_eq!(
        log.events(),
        [
            TenancyEvent::TenantCreated(created.id),
            TenancyEvent::TenantUpdated(created.id)
        ]
    );
}

#[tokio::test]
async fn set_config_persists_and_invalidates_cache() {
    let inner = Arc::new(CountingRepository::new());
    inner.inner.insert(tenant(1, "acme"));
    let cache = Arc::new(MemoryCache::new());
    let repository = Arc::new(CachedTenantRepository::new(
        inner.clone(),
        cache,
        Duration::from_secs(60),
    ));
    let tenancy = Tenancy::builder()
        .repository(repository)
        .build()
        .expect("tenancy");

    let warmed = tenancy
        .repository()
        .find(TenantId(1))
        .await
        .expect("warm")
        .expect("tenant");
    tenancy
        .set_config(&warmed, "timezone", "Asia/Jakarta", false)
        .await
        .expect("set config");

    assert_eq!(inner.saves.load(Ordering::SeqCst), 1);
    let reloaded = tenancy
        .repository()
        .find(TenantId(1))
        .await
        .expect("find")
        .expect("tenant");
    assert_eq!(reloaded.config.get("timezone"), Some("Asia/Jakarta"));
    assert_eq!(
        inner.finds.load(Ordering::SeqCst),
        2,
        "set_config must invalidate the cached lookup"
    );
}

fn tenancy_with_ports(
    repository: Arc<MemoryTenantRepository>,
    log: EventLog,
    calls: Calls,
) -> Tenancy {
    let cache = Arc::new(CountingCache {
        inner: MemoryCache::new(),
        calls: calls.clone(),
    });
    let sessions = Arc::new(CountingSession {
        inner: MemorySessionStore::new(),
        calls: calls.clone(),
    });
    let storage = Arc::new(CountingStorage {
        calls: calls.clone(),
    });
    Tenancy::builder()
        .repository(repository)
        .cache(cache)
        .sessions(sessions)
        .storage(storage)
        .listener(Arc::new(RecordingListener::new(log)))
        .build()
        .expect("tenancy")
}

#[tokio::test]
async fn delete_tenant_default_purge_only_removes_the_row() {
    let repository = Arc::new(MemoryTenantRepository::new());
    repository.insert(tenant(1, "acme"));
    let log = EventLog::default();
    let calls = Calls::default();
    let tenancy = tenancy_with_ports(repository.clone(), log.clone(), calls.clone());

    delete_tenant(&tenancy, TenantId(1), Purge::default())
        .await
        .expect("delete");

    assert!(repository.find(TenantId(1)).await.expect("find").is_none());
    assert_eq!(log.events(), [TenancyEvent::TenantDeleted(TenantId(1))]);
    assert!(
        calls.entries().is_empty(),
        "default purge must not touch storage, cache or sessions: {:?}",
        calls.entries()
    );
}

#[tokio::test]
async fn delete_tenant_purges_requested_resources_by_prefix() {
    let repository = Arc::new(MemoryTenantRepository::new());
    repository.insert(tenant(1, "acme"));
    let log = EventLog::default();
    let calls = Calls::default();
    let tenancy = tenancy_with_ports(repository.clone(), log, calls.clone());

    delete_tenant(
        &tenancy,
        TenantId(1),
        Purge {
            storage: true,
            cache: true,
            sessions: true,
            database: false,
        },
    )
    .await
    .expect("delete");

    assert_eq!(
        calls.entries(),
        [
            "storage:acme/",
            "cache:tenant:acme:",
            "session:tenant:acme:"
        ]
    );
}

#[cfg(not(feature = "postgres"))]
#[tokio::test]
async fn delete_tenant_database_purge_requires_postgres_feature() {
    let repository = Arc::new(MemoryTenantRepository::new());
    repository.insert(tenant(1, "acme"));
    let tenancy = tenancy_with_ports(repository, EventLog::default(), Calls::default());

    let result = delete_tenant(
        &tenancy,
        TenantId(1),
        Purge {
            database: true,
            ..Purge::default()
        },
    )
    .await;

    assert!(matches!(result, Err(TenancyError::Config(_))));
}

#[cfg(feature = "impersonation")]
mod impersonation {
    use super::*;
    use tenancy::impersonation::{
        issue_token, start_impersonation, stop_impersonation, verify_token,
    };

    #[test]
    fn token_roundtrips_claims() {
        let token =
            issue_token(b"secret", TenantId(3), 7, Duration::from_secs(3600)).expect("issue");
        let claims = verify_token(b"secret", &token).expect("verify");
        assert_eq!(claims.tenant_id, TenantId(3));
        assert_eq!(claims.user_id, 7);
        assert!(claims.expires_at > 0);
    }

    #[test]
    fn expired_token_is_rejected() {
        let token = issue_token(b"secret", TenantId(3), 7, Duration::ZERO).expect("issue");
        assert!(matches!(
            verify_token(b"secret", &token),
            Err(TenancyError::Crypto(_))
        ));
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let token =
            issue_token(b"secret", TenantId(3), 7, Duration::from_secs(3600)).expect("issue");
        let mut tampered = token.clone();
        let last = tampered.pop().expect("token char");
        tampered.push(if last == 'a' { 'b' } else { 'a' });
        assert!(matches!(
            verify_token(b"secret", &tampered),
            Err(TenancyError::Crypto(_))
        ));
    }

    #[tokio::test]
    async fn impersonation_token_is_stored_in_the_tenant_session() {
        let sessions = Arc::new(MemorySessionStore::new());
        let tenancy = Tenancy::builder()
            .repository(Arc::new(MemoryTenantRepository::new()))
            .sessions(sessions.clone())
            .build()
            .expect("tenancy");

        tenancy
            .run(tenant(1, "acme"), || async {
                start_impersonation(&tenancy.sessions().expect("sessions"), "token-123")
                    .await
                    .expect("start");
            })
            .await
            .expect("run");

        assert_eq!(
            sessions
                .get("tenant:acme:impersonation")
                .await
                .expect("raw"),
            Some(b"token-123".to_vec())
        );

        tenancy
            .run(tenant(1, "acme"), || async {
                stop_impersonation(&tenancy.sessions().expect("sessions"))
                    .await
                    .expect("stop");
            })
            .await
            .expect("run");

        assert_eq!(
            sessions
                .get("tenant:acme:impersonation")
                .await
                .expect("raw"),
            None
        );
    }

    #[tokio::test]
    async fn impersonation_session_requires_a_tenant() {
        let sessions = Arc::new(MemorySessionStore::new());
        let tenancy = Tenancy::builder()
            .repository(Arc::new(MemoryTenantRepository::new()))
            .sessions(sessions)
            .build()
            .expect("tenancy");
        let scoped = tenancy.sessions().expect("sessions");

        assert!(matches!(
            start_impersonation(&scoped, "token").await,
            Err(TenancyError::NotResolved)
        ));
        assert!(matches!(
            stop_impersonation(&scoped).await,
            Err(TenancyError::NotResolved)
        ));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token =
            issue_token(b"secret", TenantId(3), 7, Duration::from_secs(3600)).expect("issue");
        assert!(matches!(
            verify_token(b"other-secret", &token),
            Err(TenancyError::Crypto(_))
        ));
    }
}

#[cfg(feature = "postgres")]
mod postgres_purge {
    use super::*;
    use tenancy::multi_db::{DatabaseConfig, PostgresTenantDatabase};
    use tenancy::TenantDatabase;

    fn unique_suffix() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        format!("{}-{nanos}", std::process::id())
    }

    fn url_template(url: &str) -> String {
        let (base, query) = match url.split_once('?') {
            Some((base, query)) => (base, Some(query)),
            None => (url, None),
        };
        let prefix = match base.rsplit_once('/') {
            Some((prefix, _database)) => prefix,
            None => base,
        };
        match query {
            Some(query) => format!("{prefix}/{{database}}?{query}"),
            None => format!("{prefix}/{{database}}"),
        }
    }

    fn scratch_config(url: &str) -> DatabaseConfig {
        DatabaseConfig {
            url_template: url_template(url),
            admin_url: url.to_string(),
            database_prefix: "tenancy_test_".to_string(),
            pool_max_connections: 2,
            idle_timeout: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn delete_tenant_database_purge_drops_the_tenant_database() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        };

        let databases = Arc::new(PostgresTenantDatabase::new(scratch_config(&url)));
        let acme = tenant(1, &format!("purge-{}", unique_suffix()));
        databases.create(&acme).await.expect("create database");
        let database_name = databases.config().database_name(&acme);

        let repository = Arc::new(MemoryTenantRepository::new());
        repository.insert(acme);
        let log = EventLog::default();
        let tenancy = Tenancy::builder()
            .repository(repository.clone())
            .databases(databases.clone())
            .listener(Arc::new(RecordingListener::new(log.clone())))
            .build()
            .expect("tenancy");

        delete_tenant(
            &tenancy,
            TenantId(1),
            Purge {
                database: true,
                ..Purge::default()
            },
        )
        .await
        .expect("delete tenant");

        // event dipancarkan sebelum resource dibersihkan
        assert_eq!(log.events(), [TenancyEvent::TenantDeleted(TenantId(1))]);
        assert!(repository.find(TenantId(1)).await.expect("find").is_none());

        let admin = sqlx::PgPool::connect(&url).await.expect("connect scratch");
        let existing: i64 =
            sqlx::query_scalar("select count(*) from pg_database where datname = $1")
                .bind(&database_name)
                .fetch_one(&admin)
                .await
                .expect("count database");
        admin.close().await;
        assert_eq!(existing, 0, "tenant database must be dropped");
    }
}
