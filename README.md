# tenancy-rs

Primitif multi-tenant untuk layanan Rust: resolusi tenant, lifecycle task-local, bootstrapper, scoping single-DB, database per tenant, integrasi axum, dan helper testing.

## Feature

| Feature | Isi | Default |
| --- | --- | --- |
| `axum` | middleware, extractor, router helper | tidak |
| `postgres` | sqlx pool, repository, multi-DB, migrasi | tidak |
| `postgres-rls` | transaksi RLS | tidak |
| `redis` | cache Redis | tidak |
| `encryption` | AES-256-GCM | tidak |
| `impersonation` | token HMAC | tidak |
| `testing` | fake dan helper test | tidak |

Default kosong: core bebas axum dan sqlx.

## Instalasi

```toml
[dependencies]
tenancy = { git = "https://github.com/example/tenancy-rs" }
```

## Contoh

```rust,no_run
use std::sync::Arc;

use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::{Tenancy, Tenant};

# async fn run() -> Result<(), tenancy::TenancyError> {
let tenancy = Tenancy::builder()
    .repository(Arc::new(MemoryTenantRepository::new()))
    .build()?;

let tenant = Tenant::new("Acme", "acme")?;
let slug = tenancy.run(tenant, || async { "acme" }).await?;
assert_eq!(slug, "acme");
# Ok(())
# }
```

## Dokumentasi

Buku (mdBook) ada di `docs/src/`, bangun dengan `mdbook build`:

- [Memulai](docs/src/getting-started.md)
- [Arsitektur](docs/src/architecture.md)
- [Konsep](docs/src/concepts.md)
- [Single Database](docs/src/single-database.md)
- [Multi Database](docs/src/multi-database.md)
- [Resolusi Tenant](docs/src/resolution.md)
- [Bootstrapper dan Port](docs/src/bootstrappers-and-ports.md)
- [Queue dan Worker](docs/src/queues-and-workers.md)
- [Keamanan](docs/src/security.md)
- [Operasional](docs/src/operations.md)
- [Testing](docs/src/testing.md)
- [Padanan Laravel](docs/src/laravel-parity.md)

API: `cargo doc --no-deps --open`.

Lisensi: MIT OR Apache-2.0.
