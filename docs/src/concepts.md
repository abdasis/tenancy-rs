# Konsep

## Tujuan

Memahami model tenant, lifecycle, dan aturan konkurensi sebelum menulis handler atau worker.

## Kapan dipakai

Saat mendesain alur request, pekerjaan latar, atau event listener.

## Tenant dan TenantId

`TenantId` adalah newtype `Copy` atas `i64`. `Tenant` adalah struct konkret:

```rust
use tenancy::{Domain, Tenant, TenantId};

let mut tenant = Tenant::new("Acme", "acme")?;
assert_eq!(tenant.id, TenantId(0)); // belum tersimpan
tenant.domains.push(Domain::primary("acme.example.com"));
tenant.data.insert("plan".into(), serde_json::json!("pro"));
assert_eq!(tenant.primary_domain(), Some("acme.example.com"));
# Ok::<_, tenancy::TenancyError>(())
```

Atribut khusus proyek masuk ke `data`, bukan field baru di crate.

## Lifecycle otomatis

`run` menjalankan urutan lengkap dan mengembalikan nilai future:

```rust,no_run
use tenancy::current_context;

# async fn example(tenancy: &tenancy::Tenancy, tenant: tenancy::Tenant) -> Result<(), tenancy::TenancyError> {
let slug = tenancy
    .run(tenant, || async {
        current_context().map(|context| context.tenant().slug.clone())
    })
    .await?;
assert!(slug.is_some());
# Ok(())
# }
```

Urutan: listener `Bootstrapping`, bootstrapper maju sesuai registrasi, listener `Bootstrapped`, future handler, listener `Ending`, bootstrapper mundur, listener `Ended`.

## Lifecycle manual

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy, tenant: tenancy::Tenant) -> Result<(), tenancy::TenancyError> {
let handle = tenancy.initialize(tenant).await?;
// handle.context() berisi tenant dan namespace
tenancy.end(handle).await?;
# Ok(())
# }
```

`end` wajib dipanggil berpasangan dengan `initialize`. Handle yang di-drop tanpa `end` mencatat warning `tracing` dan melewati revert async. Pakai `TenancyHandle::run` bila butuh context terpasang di antara keduanya.

## Task-local dan konkurensi

Current tenant hidup di task-local, jadi dua request dengan tenant berbeda tidak saling melihat. Karena task-local tidak menyeberang `tokio::spawn`, selalu gunakan:

```rust,no_run
# async fn example(tenancy: std::sync::Arc<tenancy::Tenancy>, tenant: tenancy::Tenant) {
let handle = tenancy.spawn(tenant, || async { "done" });
# let _ = handle;
# }
```

atau `tenancy.run(...)` di dalam task baru.

## Event

`TenancyEvent` mencakup siklus hidup (`Bootstrapping`..`Ended`) dan perubahan tenant (`TenantCreated`, `TenantUpdated`, `TenantDeleted`, `MaintenanceEnabled`, `MaintenanceDisabled`). Listener dipanggil berurutan.

Kebijakan error:

- Error pada `Bootstrapping`/`Ending` membatalkan lifecycle dan dikembalikan ke pemanggil.
- Error pada `Bootstrapped`/`Ended` hanya dicatat (`tracing::warn`).
- Bootstrapper yang gagal di tengah menyebabkan bootstrapper sebelumnya di-revert terbalik sebelum error dikembalikan.

## Mengapa scoping eksplisit

Eloquent di Laravel memasang global scope otomatis. Di Rust, helper eksplisit dipilih karena:

- Query admin pusat tidak diam-diam terfilter.
- Pemanggil yang tahu posisi `WHERE`/`AND` dan alias join.
- Tidak ada kejutan saat query ditulis manual atau lewat query builder lain.

"Tanpa tenancy" sama dengan tidak memanggil helper scoping. Detail di [Single Database](single-database.md).

## Pitfall

- Jangan menyimpan tenant di variabel global atau `static`; itu bocor antar request konkuren.
- Jangan menganggap revert berjalan saat future dibatalkan; bootstrapper hanya boleh menulis ke context, bukan state global.
- Event tenant (`TenantCreated`, dst.) dipancarkan manager, bukan repository; jangan memancarkannya ulang di adapter.
