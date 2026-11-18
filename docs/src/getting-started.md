# Memulai

## Tujuan

Memasang crate `tenancy`, memilih feature yang dipakai, lalu menjalankan request pertama dengan tenant yang ter-resolve dari header, memakai repository Postgres dan lapisan axum.

## Kapan dipakai

Pakai panduan ini saat menyiapkan layanan baru. Untuk menambah tenancy ke layanan yang sudah berjalan, mulai dari [Konsep](concepts.md) dan [Single Database](single-database.md), lalu kembali ke sini untuk wiring.

## Instalasi

Sebagai git dependency:

```toml
[dependencies]
tenancy = { git = "https://github.com/example/tenancy-rs", features = ["axum", "postgres"] }
```

Sebagai path dependency saat pengembangan lokal:

```toml
[dependencies]
tenancy = { path = "../tenancy-rs", features = ["axum", "postgres"] }
```

Feature default kosong. Pilih hanya adapter yang dipakai:

| Feature | Isi |
| --- | --- |
| `axum` | middleware, extractor, router helper |
| `postgres` | repository Postgres, multi-DB, migrasi |
| `postgres-rls` | transaksi RLS (memaksa `postgres`) |
| `redis` | cache Redis |
| `encryption` | AES-256-GCM |
| `impersonation` | token HMAC dan helper session |
| `testing` | fake dan helper test |

## Wiring minimal axum + Postgres

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use axum::{routing::get, Router};
use tenancy::axum::{tenant_router, CurrentTenant};
use tenancy::bootstrapper::cache::CacheBootstrapper;
use tenancy::bootstrapper::database::DatabaseBootstrapper;
use tenancy::bootstrapper::storage::StorageBootstrapper;
use tenancy::multi_db::{DatabaseConfig, PostgresTenantDatabase};
use tenancy::repository::postgres::PgTenantRepository;
use tenancy::resolver::chain::ResolverChain;
use tenancy::resolver::request_data::HeaderResolver;
use tenancy::{Tenancy, Tenant};

async fn whoami(CurrentTenant(tenant): CurrentTenant) -> String {
    tenant.slug
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
    let databases = Arc::new(PostgresTenantDatabase::new(DatabaseConfig {
        url_template: std::env::var("TENANT_DATABASE_URL_TEMPLATE")?,
        admin_url: std::env::var("DATABASE_ADMIN_URL")?,
        database_prefix: "tenant_".to_string(),
        pool_max_connections: 10,
        idle_timeout: Duration::from_secs(300),
    }));

    let tenancy = Arc::new(
        Tenancy::builder()
            .repository(Arc::new(PgTenantRepository::new(pool.clone())))
            .resolver(Arc::new(ResolverChain::new(vec![Arc::new(
                HeaderResolver::new("x-tenant"),
            )])))
            .bootstrapper(Arc::new(CacheBootstrapper::new()))
            .bootstrapper(Arc::new(StorageBootstrapper::new("storage".into())))
            .bootstrapper(Arc::new(DatabaseBootstrapper::new(pool, databases)))
            .build()?,
    );

    let app = tenant_router(tenancy, Router::new().route("/whoami", get(whoami)));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

`Tenant` tidak dipakai di contoh ini selain lewat extractor; `whoami` menunjukkan request tanpa tenant berakhir 404 sebelum handler dipanggil.

## Migrasi skema pusat

Skema tenant, domain, dan konfigurasi tersedia sebagai konstanta:

```rust,no_run
# async fn migrate(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
sqlx::raw_sql(tenancy::TENANCY_MIGRATION).execute(pool).await?;
# Ok(())
# }
```

Untuk database yang sudah punya tabel `tenants` dengan bentuk berbeda, jangan jalankan migrasi ini; tulis implementasi `TenantRepository` sendiri. Lihat [Operasional](operations.md).

## Langkah operasional

1. Siapkan database pusat dan jalankan `TENANCY_MIGRATION`.
2. Set `DATABASE_URL`, `DATABASE_ADMIN_URL`, dan template URL tenant.
3. Pilih resolver sesuai topologi (header, domain, subdomain, path).
4. Daftarkan bootstrapper yang menulis namespace dan pool.
5. Pasang `tenant_router` pada router tenant; router pusat tidak memakai layer.
6. Jalankan `migrate_all`/`seed_all` untuk tenant yang memakai database terpisah.

## Pitfall

- Default feature kosong: tanpa `axum` modul `tenancy::axum` tidak ada, tanpa `postgres` tipe sqlx tidak tersedia.
- `Tenancy::resolve` membutuhkan resolver dan repository; keduanya wajib dikonfigurasi sebelum request pertama.
- Slug hanya `[a-z0-9-]`; nama domain dinormalkan (lowercase, port dibuang) saat `Domain` dibuat.
- Jangan memanggil `tenancy.run` di dalam `tokio::spawn` tanpa membawa `Tenancy` dan tenant; lihat [Queue dan Worker](queues-and-workers.md).
