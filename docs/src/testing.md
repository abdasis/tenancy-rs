# Testing

## Tujuan

Menulis test cepat dengan port in-memory, dan test integrasi Postgres yang deterministik serta bersih-bersih.

## Kapan dipakai

Saat menambah handler, bootstrapper, port, atau adapter baru.

## Helper feature testing

Aktifkan `features = ["testing"]` pada dev-dependency:

```toml
[dev-dependencies]
tenancy = { path = "..", features = ["testing"] }
```

- `fake_tenancy()` — repository, cache, session, queue in-memory, storage di temp dir.
- `tenant(slug)` — factory tenant belum tersimpan.
- `FakeBootstrapper::new(name, log)` — mencatat `bootstrap:{name}` dan `revert:{name}`.
- `RecordingListener::new()` — mencatat semua `TenancyEvent`.

```rust,no_run
use tenancy::testing::{fake_tenancy, tenant};
use tenancy::current_context;

# async fn example() -> Result<(), tenancy::TenancyError> {
let tenancy = fake_tenancy();
let slug = tenancy
    .run(tenant("acme"), || async {
        current_context().map(|context| context.tenant().slug.clone())
    })
    .await?;
assert_eq!(slug.as_deref(), Some("acme"));
# Ok(())
# }
```

## Test integrasi Postgres

Test DB memakai `TEST_DATABASE_URL`; tanpa env, test di-skip dengan pesan:

```
skipping postgres test: TEST_DATABASE_URL is not set
```

Contoh menjalankan dengan database scratch unik:

```bash
createdb tenancy_scratch_$$
export TEST_DATABASE_URL="postgres:///tenancy_scratch_$$?host=/var/run/postgresql"
cargo test --all-features
dropdb tenancy_scratch_$$
```

Test crate membuat dan menghapus database tenant sendiri (prefix `tenancy_test_`), tetapi tetap bersihkan sisa bila test gagal di tengah.

## Test lapisan axum

Gunakan `tower::ServiceExt::oneshot` tanpa membuka port:

```rust,no_run
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tenancy::axum::{tenant_router, CurrentTenant};
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::resolver::request_data::HeaderResolver;
use tenancy::Tenancy;
use tower::ServiceExt;

async fn whoami(CurrentTenant(tenant): CurrentTenant) -> String {
    tenant.slug
}

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let tenancy = Arc::new(
    Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .resolver(Arc::new(HeaderResolver::new("x-tenant")))
        .build()?,
);
let app = tenant_router(tenancy, Router::new().route("/whoami", get(whoami)));

let response = app
    .oneshot(
        Request::builder()
            .uri("/whoami")
            .header("x-tenant", "acme")
            .body(Body::empty())?,
    )
    .await?;
assert_eq!(response.status(), StatusCode::NOT_FOUND); // tenant acme belum diseed
# Ok(())
# }
```

## Langkah menulis test

1. Mulai dari `fake_tenancy()` untuk logika core.
2. Tambahkan `RecordingListener` bila urutan event penting.
3. Untuk SQL, buat database scratch unik lalu set `TEST_DATABASE_URL`.
4. Tutup pool sebelum `drop_database`; PostgreSQL menolak drop saat ada sesi aktif.
5. Assert efek samping (baris, file, cache) bukan hanya status.

## Pitfall

- Test DB dengan nama tetap akan bertabrakan saat paralel; sertakan PID dan timestamp.
- Jangan berbagi pool antar test yang menjalankan migrasi berbeda.
- RLS butuh `max_connections(1)` bila test memeriksa reset `set_config` pada koneksi yang sama.
- `fake_tenancy` memakai temp dir `tenancy-test`; jangan mengandalkan isi kosong antar test.
- Test yang butuh feature harus diberi `#[cfg(feature = "...")]` atau ditaruh di modul terpisah.
