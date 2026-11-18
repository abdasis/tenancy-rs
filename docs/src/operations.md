# Operasional

## Tujuan

Checklist produksi: migrasi, konfigurasi environment, ukuran pool, maintenance, purge, dan observability.

## Kapan dipakai

Saat menyiapkan deployment pertama, rotasi konfigurasi, atau menyusun runbook insiden.

## Migrasi

1. Backup database pusat.
2. Jalankan `TENANCY_MIGRATION` pada database pusat (bukan database tenant).
3. Untuk tenant baru multi-DB: `create` lalu `migrate_all`/`seed_all`.
4. Bila database yang sama sudah punya tabel `tenants` versi lain, jangan jalankan migrasi crate; sediakan `TenantRepository` yang sesuai.

```rust,no_run
# async fn migrate(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
sqlx::raw_sql(tenancy::TENANCY_MIGRATION).execute(pool).await?;
# Ok(())
# }
```

## Environment

| Variabel | Isi |
| --- | --- |
| `DATABASE_URL` | koneksi database pusat |
| `DATABASE_ADMIN_URL` | koneksi admin untuk `CREATE/DROP DATABASE` |
| `TENANT_DATABASE_URL_TEMPLATE` | template dengan `{database}` |
| `TENANCY_IMPERSONATION_SECRET` | secret HMAC token impersonation |
| `TEST_DATABASE_URL` | database scratch untuk test integrasi |

Baca rahasia dari environment; jangan menaruhnya di config yang di-commit.

## Konfigurasi produksi

```rust
use std::time::Duration;

use tenancy::TenancyConfig;

let config = TenancyConfig {
    cache_ttl: Duration::from_secs(300),
    storage_root: "/var/lib/app/tenants".into(),
    maintenance_retry_after: Duration::from_secs(120),
    public_scheme: "https".to_string(),
    ..TenancyConfig::default()
};
assert_eq!(config.public_scheme, "https");
```

`public_scheme` dipakai `tenant_url`; `maintenance_retry_after` dipakai respons 503.

## Pool dan batas

- `pool_max_connections` per tenant menentukan total koneksi; kalikan dengan perkiraan tenant aktif dan bandingkan `max_connections` server.
- `idle_timeout` menutup koneksi idle, bukan pool. Registry pool tidak punya eviction; pantau jumlah pool bila tenant fluktuatif.
- Repository cached menambah satu lookup cache per miss; siapkan cache dengan latensi rendah.

## Maintenance mode

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy) -> Result<(), tenancy::TenancyError> {
use tenancy::maintenance::{end_maintenance, put_in_maintenance};

put_in_maintenance(tenancy, tenancy::TenantId(1)).await?; // request tenant -> 503 + Retry-After
end_maintenance(tenancy, tenancy::TenantId(1)).await?;
# Ok(())
# }
```

Bypass untuk platform admin lewat `maintenance_bypass`; verifikasi identitas sebelum mengembalikan `true`.

## Purge dan penghapusan

- `delete_tenant` memancarkan `TenantDeleted` sebelum membersihkan.
- `Purge::default()` hanya menghapus baris tenant.
- Purge yang gagal sebagian meninggalkan resource; tangani lewat job kompensasi dan log.
- Backup database tenant sebelum purge database.

## Observability

- Daftarkan `TenancyListener` untuk log event lifecycle dan admin.
- Tangani `TenancyError::NotResolved` sebagai 404, bukan 500.
- Pantau rasio 404 route tenant, waktu bootstrap, dan jumlah pool tenant.

## Runbook insiden singkat

1. Request 500 pada route tenant: cek log `tenant resolution failed` dan `tenancy lifecycle failed`.
2. Semua request tenant 404: cek resolver, data domain/slug, dan cache TTL setelah perubahan.
3. Database tenant tidak bisa di-drop: tutup pool dan koneksi lain, lalu ulangi purge.
4. Token impersonation ditolak: cek jam server, umur token, dan secret yang dipakai.
