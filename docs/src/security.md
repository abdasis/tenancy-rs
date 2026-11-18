# Keamanan

## Tujuan

Menutup jalur serangan yang paling sering muncul pada aplikasi multi-tenant: path traversal, SQL injection lewat identifier, kebocoran lintas tenant, dan token impersonation.

## Kapan dibaca

Sebelum rilis pertama dan setiap kali menambah endpoint yang menerima path, nama kolom, atau token.

## Path traversal

`LocalStorage` menolak komponen `..`, path absolut, dan prefix. File tidak dibuat di luar root tenant.

```rust,no_run
# async fn example(storage: &dyn tenancy::Storage) -> Result<(), tenancy::TenancyError> {
assert!(matches!(
    storage.put("../escape.txt", b"x".to_vec()).await,
    Err(tenancy::TenancyError::PathTraversal)
));
# Ok(())
# }
```

Symlink belum di-resolve (ada komentar `shortcut` di kode). Bila direktori tenant bisa ditulis pihak tak tepercaya, tambahkan validasi symlink atau jalankan storage di filesystem terisolasi.

## Identifier SQL

Helper scoping hanya menerima nama tabel/kolom `&'static str` dan alias `Option<&'static str>`. Tidak ada jalur untuk memasukkan identifier dari request. Jangan menambahkan API yang menerima `String` dari input pengguna.

## Isolasi tenant

- Scoping single-DB eksplisit; query tanpa helper akan membaca semua tenant.
- RLS opt-in sebagai jaring pengaman: role non-superuser, `FORCE ROW LEVEL SECURITY`, policy dengan `current_setting('app.tenant_id', true)`.
- RLS pada koneksi superuser tidak berefek; uji dengan role aplikasi sebenarnya.

## Purge opt-in

`Purge` default semua `false`:

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy) -> Result<(), tenancy::TenancyError> {
use tenancy::commands::{delete_tenant, Purge};

// hanya menghapus baris tenant
delete_tenant(tenancy, tenancy::TenantId(1), Purge::default()).await?;

// menghapus database, file, cache, dan session
delete_tenant(
    tenancy,
    tenancy::TenantId(1),
    Purge { database: true, storage: true, cache: true, sessions: true },
).await?;
# Ok(())
# }
```

Urutan: event `TenantDeleted` dipancarkan lebih dulu, baris dihapus, lalu resource dibersihkan. Bila purge gagal di tengah:

- event sudah terkirim dan baris sudah hilang;
- resource yang belum sempat dibersihkan tetap ada (mis. cache, session, database);
- pemanggilan ulang `delete_tenant` akan gagal `NotFound` karena baris sudah tidak ada, jadi bersihkan sisa resource secara manual atau lewat job kompensasi.

Selalu backup database tenant sebelum `Purge { database: true }`.

## Impersonation

Token: `hex(payload) + "." + hex(hmac_sha256(secret, payload))`, payload memuat `tenant_id`, `user_id`, `expires_at` (epoch detik). Verifikasi memakai perbandingan waktu konstan dan menolak token kedaluwarsa.

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy, tenant: tenancy::Tenant) -> Result<(), tenancy::TenancyError> {
use std::time::Duration;

use tenancy::impersonation::{issue_token, start_impersonation, stop_impersonation};

let secret = std::env::var("TENANCY_IMPERSONATION_SECRET")?;
let token = issue_token(secret.as_bytes(), tenant.id, 42, Duration::from_secs(900))?;

tenancy.run(tenant, || async {
    let sessions = tenancy.sessions()?;
    start_impersonation(&sessions, &token).await?; // key: impersonation
    // ... pekerjaan sebagai user 42 ...
    stop_impersonation(&sessions).await
}).await?;
# Ok(())
# }
```

Helper session mengembalikan `NotResolved` di luar lifecycle tenant.

## Maintenance

Layer axum membalas 503 + `Retry-After` (default 60 detik, atur lewat `TenancyConfig.maintenance_retry_after`). Bypass hanya lewat `maintenance_bypass` milik host; jangan mengandalkan header klien tanpa memeriksa identitas admin.

## Host header

Resolusi domain memakai `Host`/authority. Klien bisa memalsukannya bila aplikasi terbuka langsung. Tempatkan di belakang reverse proxy yang menetapkan host tepercaya, dan hindari memakai domain resolver sendirian untuk otorisasi.

## Rahasia

- Secret impersonation dan key enkripsi dari environment atau secret manager; jangan di source control.
- Kunci AES-256-GCM 32 byte; rotasi kunci berarti seal ulang nilai konfigurasi lama.
- `tenant_configs` yang terenkripsi hanya sekuat kunci dan kontrol akses database.

## Checklist singkat

1. Storage root bukan direktori bersama yang bisa ditulis klien.
2. Semua query tenant memakai helper; query pusat memakai fungsi terpisah.
3. Purge dipanggil dari endpoint admin terautentikasi, dengan backup.
4. Token impersonation pendek umurnya dan diaudit.
5. Maintenance bypass tidak membuka seluruh route sensitif.
