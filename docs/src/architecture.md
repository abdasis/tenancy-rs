# Arsitektur

## Tujuan

Menjelaskan pembagian modul, arah dependensi, dan alasan keputusan desain supaya perubahan berikutnya tidak merusak batas core/adapter.

## Kapan dibaca

Sebelum menambah port, adapter, atau fitur baru; saat meninjau PR yang menyentuh core.

## Peta modul

| Modul | Isi | Feature |
| --- | --- | --- |
| `tenant` | `Tenant`, `TenantId`, `Domain`, `TenantConfig` | core |
| `manager` | `Tenancy`, builder, lifecycle, task-local | core |
| `context` | `TenantContext`, slot bertipe, `current_context` | core |
| `event` | `TenancyEvent`, `TenancyListener` | core |
| `config` | `TenancyConfig`, `DatabaseMode` | core |
| `resolver` | `RequestInfo`, `TenantKey`, resolver dan chain | core |
| `repository` | trait repository, memory, cached decorator, Postgres | core + `postgres` |
| `ports` | cache, storage, session, queue, encryptor, scoped wrapper | core, adapter per feature |
| `bootstrapper` | hook lifecycle dan bootstrapper bawaan | core + `postgres` |
| `single_db` | scoping, validasi, RLS | `postgres`, `postgres-rls` |
| `multi_db` | `DatabaseConfig`, registry pool | core config + `postgres` |
| `commands` | perintah admin dan worker | core + `postgres` |
| `maintenance` | flag maintenance | core |
| `impersonation` | token HMAC dan helper session | `impersonation` |
| `axum` | layer, extractor, router, respons | `axum` |
| `testing` | fake dan helper assertion | `testing` |

Core tidak mengimpor axum maupun sqlx. Semua tipe sqlx berada di balik `feature = "postgres"`.

## Alur data request

```
Request -> RequestInfo -> resolver chain -> TenantKey
        -> repository.find_by_key -> Tenant
        -> maintenance check -> TenantContext (task-local)
        -> handler -> teardown bootstrapper terbalik
```

## Model konkurensi

- Current tenant disimpan di task-local Tokio; tidak ada state global.
- `run` memasang context dengan `TaskLocalFuture`. Future yang dibatalkan otomatis memulihkan context luar, tetapi revert bootstrapper di-skip.
- Karena task-local tidak menyeberang `tokio::spawn`, pekerjaan latar memakai `tenancy.spawn` atau `tenancy.run` eksplisit.

## Keputusan

1. Satu crate feature-gated, bukan workspace, sampai adapter tumbuh besar.
2. `Tenant` struct konkret dengan peta `data`, bukan trait generik, agar adapter tidak menularkan parameter tipe.
3. Scoping single-DB eksplisit (helper), bukan global scope implisit, agar query admin pusat tidak diam-diam terfilter.
4. Cached lookup sebagai dekorator repository, sehingga semua jalur lookup ikut ter-cache dan invalidasi terpusat.
5. Bootstrapper hanya menulis ke context; `Drop` tidak menjalankan async revert.
6. Purge resource opt-in lewat `Purge`; event `TenantDeleted` dipancarkan sebelum pembersihan.

## Pitfall

- Jangan menambah dependency framework ke core demi kenyamanan adapter.
- Nama tabel/kolom dinamis hanya boleh identifier statis.
- Menambah slot context baru berarti memilih tipe kunci (`TypeId`); gunakan newtype seperti `TenantPool`, bukan tipe umum seperti `PgPool`, agar tidak bentrok.
