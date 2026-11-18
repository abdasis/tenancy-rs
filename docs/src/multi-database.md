# Multi Database

## Tujuan

Memberi setiap tenant database sendiri, lengkap dengan pembuatan, migrasi, seed, dan penghapusan.

## Kapan dipakai

Saat isolasi fisik, ukuran data, atau kebijakan backup per tenant menuntut database terpisah.

## DatabaseConfig

```rust,no_run
use std::time::Duration;

use tenancy::multi_db::{DatabaseConfig, PostgresTenantDatabase};

let databases = PostgresTenantDatabase::new(DatabaseConfig {
    url_template: "postgres://user:pass@db:5432/{database}".to_string(),
    admin_url: "postgres://user:pass@db:5432/postgres".to_string(),
    database_prefix: "tenant_".to_string(),
    pool_max_connections: 10,
    idle_timeout: Duration::from_secs(300),
});
# let _ = databases;
```

- `url_template` wajib memuat `{database}`; `tenant_url` mengembalikan `TenancyError::Config` bila tidak ada.
- `admin_url` dipakai `CREATE`/`DROP DATABASE` karena operasi itu tidak boleh berjalan pada koneksi ke database tenant.
- `database_name` memakai `Tenant.database` bila diisi, selain itu `database_prefix + slug` dengan `-` menjadi `_`.

```rust
use std::time::Duration;
use tenancy::{DatabaseConfig, Tenant};

let config = DatabaseConfig {
    url_template: "postgres://localhost/{database}".into(),
    admin_url: "postgres://localhost/postgres".into(),
    database_prefix: "tenant_".into(),
    pool_max_connections: 5,
    idle_timeout: Duration::from_secs(60),
};
assert_eq!(config.database_name(&Tenant::new("Acme", "springfield-high")?), "tenant_springfield_high");
# Ok::<_, tenancy::TenancyError>(())
```

## Operasi database per tenant

```rust,no_run
# async fn example(
#     databases: &tenancy::multi_db::PostgresTenantDatabase,
#     tenant: &tenancy::Tenant,
#     migrator: &sqlx::migrate::Migrator,
# ) -> Result<(), tenancy::TenancyError> {
use tenancy::TenantDatabase;

databases.create(tenant).await?;          // CREATE DATABASE via admin_url
databases.migrate(tenant, migrator).await?; // migrator.run pada pool tenant
let pool = databases.connect(tenant).await?;
databases.drop_database(tenant).await?;   // tutup pool lalu DROP
# let _ = pool;
# Ok(())
# }
```

Seed menerima tenant dan pool tenant sehingga closure bisa menjalankan SQL:

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy) -> Result<(), tenancy::TenancyError> {
use std::future::Future;
use std::pin::Pin;

use tenancy::{SeedFn, Tenant, TenancyError};

fn seed_row<'a>(
    tenant: &'a Tenant,
    pool: &'a sqlx::PgPool,
) -> Pin<Box<dyn Future<Output = Result<(), TenancyError>> + Send + 'a>> {
    Box::pin(async move {
        sqlx::query("insert into settings (key, value) values ('plan', $1)")
            .bind(&tenant.slug)
            .execute(pool)
            .await?;
        Ok(())
    })
}

let seed: &SeedFn = &seed_row;
tenancy::commands::seed_all(tenancy, seed).await?;
# Ok(())
# }
```

`migrate_all` dan `seed_all` mengiterasi semua tenant secara berurutan.

## Registry pool

`PostgresTenantDatabase::connect` menyimpan satu pool per tenant. Panggilan kedua mengembalikan pool yang sama. `drop_database` menutup pool sebelum menghapus database.

Batasan saat ini (lihat `shortcut` pada kode): `idle_timeout` hanya menutup koneksi idle, bukan pool. Pool yang sudah tidak dipakai tetap tersimpan sampai `drop_database` dipanggil. Bila jumlah tenant besar dan fluktuatif, tambahkan eviction/reaper di lapisan host.

## Langkah operasional

1. Pastikan role aplikasi boleh `CREATEDB` (atau sediakan role admin terpisah).
2. Siapkan `admin_url` ke database pemeliharaan, bukan database tenant.
3. Jalankan `create` lalu `migrate_all` untuk tenant baru.
4. Simpan `Tenant.database` hanya bila nama database menyimpang dari derivasi slug.
5. Sebelum `delete_tenant` dengan `Purge { database: true }`, pastikan backup selesai.

## Pitfall

- `CREATE/DROP DATABASE` tidak bisa berjalan di dalam transaksi.
- `DROP DATABASE` gagal bila masih ada sesi aktif; tutup pool tenant dan koneksi lain lebih dulu.
- Nama database divalidasi `[a-z0-9_]`; nama dengan huruf besar atau tanda hubung ditolak sebelum SQL dijalankan.
- Migrasi crate tenancy (`TENANCY_MIGRATION`) adalah skema pusat; jangan dijalankan ke database tenant.
