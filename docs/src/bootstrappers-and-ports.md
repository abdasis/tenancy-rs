# Bootstrapper dan Port

## Tujuan

Menjelaskan hook lifecycle dan kontrak port yang harus diisi host, supaya state tenant konsisten dan tidak bocor.

## Kapan dipakai

Saat menambahkan koneksi database per tenant, namespace cache/session, root storage, atau context queue.

## Urutan bootstrap dan revert

`initialize` menjalankan:

1. listener `Bootstrapping` (error membatalkan lifecycle),
2. bootstrapper maju sesuai urutan registrasi,
3. listener `Bootstrapped` (error hanya dicatat).

Teardown menjalankan:

1. listener `Ending` (error dikembalikan setelah revert),
2. bootstrapper mundur (urutan terbalik),
3. listener `Ended` (error hanya dicatat).

Bila bootstrapper ke-`n` gagal, bootstrapper `n-1..0` di-revert terbalik sebelum error dikembalikan.

## Bootstrapper bawaan

| Bootstrapper | Menulis | Feature |
| --- | --- | --- |
| `CacheBootstrapper` | `cache_namespace = tenant:{slug}` | core |
| `SessionBootstrapper` | `session_namespace = tenant:{slug}` | core |
| `QueueBootstrapper` | `queue_namespace = tenant:{slug}` | core |
| `StorageBootstrapper::new(root)` | `storage_root = root/{slug}` | core |
| `DatabaseBootstrapper::new(central, databases)` | slot `TenantPool` pusat atau pool tenant | `postgres` |

`StorageBootstrapper` tidak membuat direktori; direktori dibuat saat penulisan pertama.

## Port yang wajib diisi host

`CacheStore`, `Storage`, `SessionStore`, `JobQueue`, `Encryptor`, `TenantRepository`, `TenantResolver`, dan (bila multi-DB) `TenantDatabase`. Crate menyediakan implementasi memory/local, Redis, dan AES-GCM.

## Contoh port kustom

Membungkus cache dengan metrik tetap mengikuti kontrak yang sama:

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tenancy::{CacheStore, TenancyError};

struct MetricsCache {
    inner: Arc<dyn CacheStore>,
    hits: std::sync::atomic::AtomicU64,
}

#[async_trait]
impl CacheStore for MetricsCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        let value = self.inner.get(key).await?;
        if value.is_some() {
            self.hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(value)
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError> {
        self.inner.set(key, value, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        self.inner.delete(key).await
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        self.inner.delete_prefix(prefix).await
    }
}
```

`remember` punya default implementation (get, lalu loader, lalu set), jadi port kustom tidak wajib mengisinya.

## Menggunakan state tenant

```rust,no_run
# async fn example(tenancy: &tenancy::Tenancy, tenant: tenancy::Tenant) -> Result<(), tenancy::TenancyError> {
tenancy
    .run(tenant, || async {
        tenancy.cache()?.set("greeting", b"hai".to_vec(), None).await?;
        // key nyata: tenant:{slug}:greeting
        Ok(())
    })
    .await?;
# Ok(())
# }
```

`ScopedCache`, `ScopedSession`, dan `ScopedStorage` mengembalikan `NotResolved` di luar lifecycle.

## Langkah operasional

1. Daftarkan bootstrapper dari yang paling umum ke paling spesifik.
2. Pastikan setiap port yang dipakai handler sudah dikonfigurasi; getter mengembalikan `Config` bila kosong.
3. Tambahkan listener untuk observability (log event, metrik).
4. Uji urutan dengan `FakeBootstrapper` dari feature `testing`.

## Pitfall

- Bootstrapper tidak boleh menulis state global; hanya context task-local.
- Revert tidak dijalankan saat future dibatalkan; jangan menaruh resource yang butuh pembersihan wajib di bootstrap.
- Kunci context bertipe `TypeId`; dua port dengan tipe sama akan saling menimpa slot. Gunakan newtype.
- Error `Bootstrapping`/`Ending` menggagalkan request; jangan melempar error untuk kondisi yang bisa dilanjutkan.
