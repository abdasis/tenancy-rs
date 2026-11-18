use std::sync::Arc;
use std::time::Duration;

use tenancy::commands::{run_for_all_tenants, run_for_tenant};
use tenancy::multi_db::DatabaseConfig;
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::{current_context, Tenancy, TenancyError, Tenant, TenantId};

fn config() -> DatabaseConfig {
    DatabaseConfig {
        url_template: "postgres://localhost/{database}".to_string(),
        admin_url: "postgres://localhost/postgres".to_string(),
        database_prefix: "tenant_".to_string(),
        pool_max_connections: 5,
        idle_timeout: Duration::from_secs(60),
    }
}

fn tenant(id: i64, slug: &str) -> Tenant {
    let mut tenant = Tenant::new("Acme", slug).expect("valid tenant");
    tenant.id = TenantId(id);
    tenant
}

fn tenancy_with(tenants: Vec<Tenant>) -> Tenancy {
    let repository = Arc::new(MemoryTenantRepository::new());
    for tenant in tenants {
        repository.insert(tenant);
    }
    Tenancy::builder()
        .repository(repository)
        .build()
        .expect("tenancy")
}

#[test]
fn database_name_derives_from_slug_and_prefix() {
    assert_eq!(
        config().database_name(&tenant(1, "springfield-high")),
        "tenant_springfield_high"
    );
}

#[test]
fn explicit_database_overrides_derivation() {
    let mut tenant = tenant(1, "springfield-high");
    tenant.database = Some("custom".to_string());
    assert_eq!(config().database_name(&tenant), "custom");
}

#[test]
fn tenant_url_substitutes_database_placeholder() {
    assert_eq!(
        config()
            .tenant_url(&tenant(1, "springfield-high"))
            .expect("url"),
        "postgres://localhost/tenant_springfield_high"
    );
}

#[test]
fn tenant_url_without_placeholder_is_config_error() {
    let mut config = config();
    config.url_template = "postgres://localhost/fixed".to_string();
    assert!(matches!(
        config.tenant_url(&tenant(1, "a")),
        Err(TenancyError::Config(_))
    ));
}

#[test]
fn two_tenants_get_distinct_database_names() {
    let config = config();
    assert_ne!(
        config.database_name(&tenant(1, "alpha")),
        config.database_name(&tenant(2, "beta"))
    );
}

#[tokio::test]
async fn run_for_tenant_looks_up_and_runs_the_tenant() {
    let tenancy = tenancy_with(vec![tenant(1, "alpha")]);

    let observed = run_for_tenant(&tenancy, TenantId(1), || async {
        current_context().map(|context| context.tenant().slug.clone())
    })
    .await
    .expect("run");

    assert_eq!(observed, Some("alpha".to_string()));
    assert!(matches!(
        run_for_tenant(&tenancy, TenantId(99), || async {}).await,
        Err(TenancyError::NotFound(_))
    ));
}

#[tokio::test]
async fn run_for_all_tenants_runs_each_tenant_sequentially() {
    let tenancy = tenancy_with(vec![tenant(1, "alpha"), tenant(2, "beta")]);

    let seen = run_for_all_tenants(&tenancy, || async {
        current_context()
            .map(|context| context.tenant().slug.clone())
            .expect("tenant context")
    })
    .await
    .expect("run all");

    assert_eq!(seen, ["alpha".to_string(), "beta".to_string()]);
}

#[cfg(feature = "postgres")]
mod postgres {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    use sqlx::migrate::Migrator;
    use sqlx::PgPool;
    use tenancy::commands::{migrate_all, seed_all};
    use tenancy::multi_db::PostgresTenantDatabase;
    use tenancy::{SeedFn, TenantDatabase};

    fn unique_suffix() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        format!("{}-{nanos}", std::process::id())
    }

    fn url_template(url: &str) -> String {
        // pertahankan query (mis. host unix socket) saat mengganti nama database
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
    async fn create_connect_migrate_and_drop_real_database() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        };

        let database = PostgresTenantDatabase::new(scratch_config(&url));
        let tenant = tenant(1, &format!("scratch-{}", unique_suffix()));
        let _ = database.drop_database(&tenant).await;

        database.create(&tenant).await.expect("create database");
        let pool = database.connect(&tenant).await.expect("connect");
        assert_eq!(
            pool.connect_options().get_database(),
            Some(database.config().database_name(&tenant).as_str())
        );
        let second = database.connect(&tenant).await.expect("connect again");
        assert_eq!(
            second.connect_options().get_database(),
            pool.connect_options().get_database()
        );
        assert_eq!(database.pool_count(), 1);

        let migrator = Migrator::new(std::path::Path::new("migrations"))
            .await
            .expect("migrator");
        database.migrate(&tenant, &migrator).await.expect("migrate");
        let tables: i64 = sqlx::query_scalar("select count(*) from tenants")
            .fetch_one(&pool)
            .await
            .expect("migrated schema");
        assert_eq!(tables, 0);

        database
            .drop_database(&tenant)
            .await
            .expect("drop database");
        assert_eq!(database.pool_count(), 0);
    }

    #[tokio::test]
    async fn migrate_all_migrates_every_registered_tenant() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        };

        let databases = Arc::new(PostgresTenantDatabase::new(scratch_config(&url)));
        let tenants = vec![
            tenant(1, &format!("all-a-{}", unique_suffix())),
            tenant(2, &format!("all-b-{}", unique_suffix())),
        ];
        for tenant in &tenants {
            databases.create(tenant).await.expect("create database");
        }

        let repository = Arc::new(MemoryTenantRepository::new());
        for tenant in &tenants {
            repository.insert(tenant.clone());
        }
        let tenancy = Tenancy::builder()
            .repository(repository)
            .databases(databases.clone())
            .build()
            .expect("tenancy");

        let migrator = Migrator::new(std::path::Path::new("migrations"))
            .await
            .expect("migrator");
        migrate_all(&tenancy, &migrator).await.expect("migrate all");

        for tenant in &tenants {
            let pool = sqlx::PgPool::connect(
                &url_template(&url)
                    .replace("{database}", &databases.config().database_name(tenant)),
            )
            .await
            .expect("connect tenant database");
            sqlx::query_scalar::<_, i64>("select count(*) from _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("migration table");
            // tutup pool verifikasi dulu, PostgreSQL menolak drop saat masih ada sesi
            pool.close().await;
            databases
                .drop_database(tenant)
                .await
                .expect("drop database");
        }
    }

    #[tokio::test]
    async fn invalid_database_name_is_rejected_before_sql() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        };

        let mut config = scratch_config(&url);
        config.database_prefix = "Tenant-".to_string();
        let database = PostgresTenantDatabase::new(config);
        let mut tenant = tenant(1, "acme");
        tenant.database = Some("Bad-Name".to_string());

        assert!(matches!(
            database.create(&tenant).await,
            Err(TenancyError::Config(_))
        ));
    }
    fn seed_row<'a>(
        tenant: &'a Tenant,
        pool: &'a PgPool,
    ) -> Pin<Box<dyn Future<Output = Result<(), TenancyError>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::raw_sql("create table if not exists seeded (tenant_slug text not null)")
                .execute(pool)
                .await?;
            sqlx::query("insert into seeded (tenant_slug) values ($1)")
                .bind(&tenant.slug)
                .execute(pool)
                .await?;
            Ok(())
        })
    }

    #[tokio::test]
    async fn seed_all_writes_to_each_tenant_database() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            return;
        };

        let databases = Arc::new(PostgresTenantDatabase::new(scratch_config(&url)));
        let tenants = vec![
            tenant(1, &format!("seed-a-{}", unique_suffix())),
            tenant(2, &format!("seed-b-{}", unique_suffix())),
        ];
        for tenant in &tenants {
            databases.create(tenant).await.expect("create database");
        }

        let repository = Arc::new(MemoryTenantRepository::new());
        for tenant in &tenants {
            repository.insert(tenant.clone());
        }
        let tenancy = Tenancy::builder()
            .repository(repository)
            .databases(databases.clone())
            .build()
            .expect("tenancy");

        let seed: &SeedFn = &seed_row;
        seed_all(&tenancy, seed).await.expect("seed all");

        for tenant in &tenants {
            let pool = sqlx::PgPool::connect(
                &url_template(&url)
                    .replace("{database}", &databases.config().database_name(tenant)),
            )
            .await
            .expect("connect tenant database");
            let slugs: Vec<String> = sqlx::query_scalar("select tenant_slug from seeded")
                .fetch_all(&pool)
                .await
                .expect("seeded rows");
            // tiap database tenant hanya memuat baris miliknya sendiri
            assert_eq!(slugs.len(), 1);
            assert_eq!(slugs[0], tenant.slug);
            pool.close().await;
            databases
                .drop_database(tenant)
                .await
                .expect("drop database");
        }
    }
}
