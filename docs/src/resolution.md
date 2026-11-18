# Resolusi Tenant

## Tujuan

Menentukan tenant dari request secara deterministik, dengan urutan resolver yang jelas dan lookup ter-cache.

## Kapan dipakai

Saat memilih sumber identitas tenant: host, subdomain, path, header, atau cookie.

## RequestInfo

`RequestInfo` memuat `method`, `path`, `host` (dinormalkan: lowercase, port dan trailing dot dibuang), `headers` (lowercase), `cookies`, dan `query`.

```rust
use tenancy::RequestInfo;

let request = RequestInfo::new("GET", "/t/acme/api", "Springfield.Example.COM:443")
    .with_header("X-Tenant", "acme");
assert_eq!(request.host, "springfield.example.com");
assert_eq!(request.header("x-tenant"), Some("acme"));
```

## Resolver bawaan

| Resolver | Sumber | Catatan |
| --- | --- | --- |
| `DomainResolver` | host persis | tidak ada wildcard; daftarkan host eksplisit |
| `SubdomainResolver` | satu label di bawah central domain | `www` dan subdomain bertingkat ditolak |
| `PathResolver` | segmen pertama setelah `/t/` | tanpa prefix mengembalikan `None` |
| `HeaderResolver` | nama header | nama dinormalkan lowercase |
| `CookieResolver` | nama cookie | case-sensitive |
| `ClosureResolver` | fungsi host | fleksibel, tetap `Send + Sync` |

Setiap resolver mengembalikan `Ok(None)` saat tidak cocok; error keras hanya untuk kegagalan nyata.

## Rantai first-match

```rust,no_run
use std::sync::Arc;

use tenancy::resolver::chain::ResolverChain;
use tenancy::resolver::request_data::HeaderResolver;
use tenancy::resolver::subdomain::SubdomainResolver;
use tenancy::{RequestInfo, TenantResolver};

let chain = ResolverChain::new(vec![
    Arc::new(SubdomainResolver::new(vec!["example.com".to_string()])),
    Arc::new(HeaderResolver::new("x-tenant")),
]);

# async fn example(chain: ResolverChain) -> Result<(), tenancy::TenancyError> {
let key = chain.resolve(&RequestInfo::new("GET", "/", "acme.example.com")).await?;
assert!(key.is_some());
# Ok(())
# }
```

Resolver pertama yang menghasilkan nilai menang; sisanya tidak dipanggil.

## Cached lookup

Cache ditempatkan sebagai dekorator repository, sehingga semua jalur lookup (`Id`, `Slug`, `Domain`) ikut ter-cache dan invalidasi terpusat:

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use tenancy::ports::memory::MemoryCache;
use tenancy::repository::cached::CachedTenantRepository;
use tenancy::repository::memory::MemoryTenantRepository;

# fn example() -> Result<tenancy::Tenancy, tenancy::TenancyError> {
let repository = CachedTenantRepository::new(
    Arc::new(MemoryTenantRepository::new()),
    Arc::new(MemoryCache::new()),
    Duration::from_secs(300),
);
tenancy::Tenancy::builder().repository(Arc::new(repository)).build()
# }
```

`create`, `update`, `delete`, dan `save_config` menghapus key id, slug, serta domain lama dan baru.

## Resolusi di lapisan axum

`tenant_router` memanggil `Tenancy::resolve`. Bila tidak ada tenant, respons 404 (bukan 403) agar keberadaan tenant tidak bocor. Error resolusi selain "tidak ditemukan" menjadi 500.

## Langkah operasional

1. Tentukan sumber identitas utama dan cadangan.
2. Normalisasi host di depan proxy (Host header), lalu percayakan hanya pada proxy internal.
3. Isi cache TTL sesuai toleransi perubahan domain/slug.
4. Pantau rasio 404 pada route tenant; lonjakan biasanya berarti resolver atau data domain salah.

## Pitfall

- Tidak ada wildcard domain otomatis; simpan setiap host di `tenant_domains`.
- Header `Host` bisa dipalsukan klien; batasi lewat reverse proxy bila domain menjadi sumber utama.
- Jangan menaruh cookie resolver sebelum domain resolver bila cookie bisa diisi pengguna.
- Cache dengan TTL panjang memperlambat perubahan domain; invalidasi hanya terjadi lewat jalur tulis crate.
