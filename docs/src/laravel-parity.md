# Padanan Laravel

## Tujuan

Memetakan konsep `tenancyforlaravel` v3 ke crate ini, termasuk yang sengaja tidak diporting.

## Kapan dipakai

Saat memigrasikan aplikasi Laravel atau membandingkan perilaku.

## Tabel padanan

| Laravel / tenancyforlaravel | Rust | Catatan |
| --- | --- | --- |
| `InitializeTenancyByDomain` | `DomainResolver` | host persis, tanpa wildcard |
| `InitializeTenancyBySubdomain` | `SubdomainResolver` | satu label, `www` ditolak |
| `InitializeTenancyByPath` | `PathResolver` | prefix `/t/` |
| `InitializeTenancyByRequestData` | `HeaderResolver`, `CookieResolver`, `ClosureResolver` | header lowercase |
| `InitializeTenancy` + cached lookup | `CachedTenantRepository` | dekorator repository |
| `TenancyServiceProvider` events | `TenancyEvent`, `TenancyListener` | async listener |
| `Bootstrappers` | `Bootstrapper` | maju/mundur eksplisit |
| `tenant()` / `tenancy()` helper | `current_context()` | task-local, bukan global |
| Global scope `tenant_id` | `scope_query`, `scope_query_for` | opt-in eksplisit |
| `tenant_unique`, `tenant_exists` | fungsi dengan nama sama | unique = nilai bebas |
| RLS Postgres | `begin_tenant_tx` | `postgres-rls` |
| `DatabaseManager` per tenant | `PostgresTenantDatabase`, `TenantDatabase` | registry pool |
| `tenants:migrate`, `tenants:seed` | `migrate_all`, `seed_all` | seed menerima pool tenant |
| `tenants:run` | `run_for_tenant`, `run_for_all_tenants` | berurutan |
| Maintenance mode | `put_in_maintenance`, `end_maintenance`, layer 503 | `Retry-After` dari config |
| Impersonation | `issue_token`, `verify_token`, helper session | HMAC-SHA256 |
| Storage/Cache/Session/Queue bootstrappers | bootstrapper sesuai nama | namespace `tenant:{slug}` |
| `withoutTenancy()` | tidak memanggil helper | tidak ada scope implisit |
| `tenants:list` / CLI | diserahkan host | tanpa dependency clap |

## Tidak diporting

Nova, Livewire, Orchid, Telescope, Passport, Sanctum, Sail, Vite, Spatie packages, billing/cashier, dan auto-discovery skema. Integrasi fasade real-time diganti handle `Tenancy` yang di-inject.

## Perbedaan perilaku penting

- Route tenant tanpa tenant = 404, bukan 403.
- Tidak ada global scope otomatis; query baru wajib memanggil helper.
- Purge resource hanya lewat flag `Purge` eksplisit.
- Event tenant dipancarkan manager, bukan repository, agar repository bisa diganti.
