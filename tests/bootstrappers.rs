use tenancy::bootstrapper::cache::CacheBootstrapper;
use tenancy::bootstrapper::queue::QueueBootstrapper;
use tenancy::bootstrapper::session::SessionBootstrapper;
use tenancy::bootstrapper::storage::StorageBootstrapper;
use tenancy::{Bootstrapper, Tenant, TenantContext};

fn tenant() -> Tenant {
    Tenant::new("Springfield High", "springfield-high").expect("valid tenant")
}

#[tokio::test]
async fn bootstrappers_fill_slug_namespaces_and_storage_root() {
    let context = TenantContext::new(tenant());
    let root = std::env::temp_dir().join("tenancy-bootstrap");

    CacheBootstrapper::new()
        .bootstrap(&context)
        .await
        .expect("cache bootstrap");
    SessionBootstrapper::new()
        .bootstrap(&context)
        .await
        .expect("session bootstrap");
    QueueBootstrapper::new()
        .bootstrap(&context)
        .await
        .expect("queue bootstrap");
    StorageBootstrapper::new(root.clone())
        .bootstrap(&context)
        .await
        .expect("storage bootstrap");

    assert_eq!(context.cache_namespace(), "tenant:springfield-high");
    assert_eq!(context.session_namespace(), "tenant:springfield-high");
    assert_eq!(context.queue_namespace(), "tenant:springfield-high");
    assert_eq!(context.storage_root(), root.join("springfield-high"));
}

#[tokio::test]
async fn bootstrapper_revert_does_not_panic_or_reset_context() {
    let context = TenantContext::new(tenant());
    let root = std::env::temp_dir().join("tenancy-bootstrap-revert");
    let storage = StorageBootstrapper::new(root.clone());

    storage.bootstrap(&context).await.expect("bootstrap");
    storage.revert(&context).await.expect("revert");

    assert_eq!(context.storage_root(), root.join("springfield-high"));
}

#[cfg(feature = "postgres")]
mod postgres {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use sqlx::migrate::Migrator;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;
    use tenancy::bootstrapper::database::{DatabaseBootstrapper, TenantPool};
    use tenancy::{Bootstrapper, SeedFn, Tenant, TenantContext, TenantDatabase, TenantId};

    struct FakeDatabase {
        tenant_pool: PgPool,
        connected: Mutex<Vec<TenantId>>,
    }

    #[async_trait]
    impl TenantDatabase for FakeDatabase {
        async fn connect(&self, tenant: &Tenant) -> Result<PgPool, tenancy::TenancyError> {
            self.connected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(tenant.id);
            Ok(self.tenant_pool.clone())
        }

        async fn create(&self, _tenant: &Tenant) -> Result<(), tenancy::TenancyError> {
            Ok(())
        }

        async fn drop_database(&self, _tenant: &Tenant) -> Result<(), tenancy::TenancyError> {
            Ok(())
        }

        async fn migrate(
            &self,
            _tenant: &Tenant,
            _migrator: &Migrator,
        ) -> Result<(), tenancy::TenancyError> {
            Ok(())
        }

        async fn seed(
            &self,
            _tenant: &Tenant,
            _seed: &SeedFn,
        ) -> Result<(), tenancy::TenancyError> {
            Ok(())
        }
    }

    fn lazy_pool(database: &str) -> PgPool {
        PgPoolOptions::new()
            .connect_lazy(&format!("postgres://localhost/{database}"))
            .expect("lazy pool")
    }

    #[tokio::test]
    async fn database_bootstrapper_uses_tenant_pool_only_for_multi_db_tenants() {
        if std::env::var("TEST_DATABASE_URL").is_err() {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        }

        let central = lazy_pool("central");
        let fake = Arc::new(FakeDatabase {
            tenant_pool: lazy_pool("tenant_db"),
            connected: Mutex::new(Vec::new()),
        });
        let bootstrapper = DatabaseBootstrapper::new(central.clone(), fake.clone());

        let mut multi_db = Tenant::new("Multi", "multi").expect("valid tenant");
        multi_db.id = TenantId(2);
        multi_db.database = Some("tenant_db".to_string());
        let context = TenantContext::new(multi_db);
        bootstrapper.bootstrap(&context).await.expect("bootstrap");

        assert_eq!(
            fake.connected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            [TenantId(2)]
        );
        let pool = context.get_cloned::<TenantPool>().expect("tenant pool");
        assert_eq!(pool.0.connect_options().get_database(), Some("tenant_db"));

        let mut single_db = Tenant::new("Single", "single").expect("valid tenant");
        single_db.id = TenantId(3);
        let context = TenantContext::new(single_db);
        bootstrapper.bootstrap(&context).await.expect("bootstrap");

        assert_eq!(
            fake.connected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
        let pool = context.get_cloned::<TenantPool>().expect("central pool");
        assert_eq!(pool.0.connect_options().get_database(), Some("central"));
    }
}
