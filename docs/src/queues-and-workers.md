# Queue dan Worker

## Tujuan

Menjalankan pekerjaan latar dengan tenant yang benar, tanpa bergantung pada state global.

## Kapan dipakai

Saat request memicu pekerjaan asinkron (kirim laporan, sinkronisasi, impor) yang harus dieksekusi per tenant.

## TenantJob

```rust,no_run
use tenancy::{JobQueue, TenantJob};

# async fn example(queue: &dyn JobQueue) -> Result<(), tenancy::TenancyError> {
queue
    .enqueue(TenantJob {
        tenant_id: tenancy::TenantId(1),
        name: "send-report".to_string(),
        payload: serde_json::json!({ "month": "2026-11" }),
    })
    .await?;
# Ok(())
# }
```

Queue wajib menyimpan `tenant_id`; tanpa itu worker tidak bisa memasang context sebelum eksekusi.

## Worker loop

```rust,no_run
use std::sync::Arc;

use tenancy::{current_context, Tenancy, TenantJob};

async fn handle(job: TenantJob) {
    // eksekusi job; current_context() berisi tenant yang sama dengan job.tenant_id
    let _slug = current_context().map(|context| context.tenant().slug.clone());
}

async fn worker(tenancy: &Tenancy, job: TenantJob) -> Result<(), tenancy::TenancyError> {
    let tenant = tenancy
        .repository()
        .find(job.tenant_id)
        .await?
        .ok_or_else(|| tenancy::TenancyError::NotFound(job.tenant_id.to_string()))?;
    tenancy.run(tenant, || handle(job)).await
}
# let _ = worker;
```

Untuk worker yang menerima `Arc<Tenancy>`, `tenancy.spawn(tenant, || async { ... })` mengembalikan `JoinHandle<Result<T, TenancyError>>` dan memasang context di task baru.

## Command helper

`run_for_tenant` dan `run_for_all_tenants` melakukan lookup repository lalu memanggil `run`:

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy) -> Result<(), tenancy::TenancyError> {
use tenancy::commands::{run_for_all_tenants, run_for_tenant};

run_for_tenant(tenancy, tenancy::TenantId(1), || async {
    // pekerjaan tenant 1
}).await?;

let results = run_for_all_tenants(tenancy, || async {
    // dijalankan berurutan untuk semua tenant
    1
}).await?;
# let _ = results;
# Ok(())
# }
```

## Larangan tokio::spawn polos

Task-local tidak menyeberang `tokio::spawn` biasa:

```rust,ignore
// SALAH: context tenant hilang di task baru
tokio::spawn(async move { current_context() });

// BENAR
tenancy.spawn(tenant, || async { current_context() });
```

## Langkah operasional

1. Pastikan payload job cukup untuk eksekusi tanpa request asli.
2. Worker melakukan lookup tenant segar sebelum eksekusi (status maintenance bisa berubah).
3. Terapkan retry dengan batas dan idempotensi; job yang gagal karena tenant terhapus sebaiknya dibuang.
4. Pantau job yang tenant-nya tidak ditemukan; biasanya berarti urutan delete/purge salah.

## Pitfall

- Jangan menyimpan `TenantContext` di payload job; simpan `tenant_id`.
- Jangan memakai `tokio::spawn` polos lalu berharap tenant ikut.
- `run_for_all_tenants` berjalan berurutan; untuk paralel, batasi konkurensi di lapisan host.
- Job yang berjalan saat tenant dihapus harus menangani `NotFound`.
