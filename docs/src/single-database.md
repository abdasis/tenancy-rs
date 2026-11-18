# Single Database

## Tujuan

Menjalankan banyak tenant pada satu database dengan kolom `tenant_id`, secara eksplisit dan aman.

## Kapan dipakai

Saat isolasi logis sudah cukup dan operasi lintas tenant (laporan pusat) sering dibutuhkan.

## Skema

```sql
create table posts (
    id bigserial primary key,
    tenant_id bigint not null references tenants(id) on delete cascade,
    title text not null,
    created_at timestamptz not null default now()
);

-- index untuk setiap query ter-scope
create index posts_tenant_idx on posts (tenant_id);
-- keunikan per tenant, bukan global
create unique index posts_tenant_title_idx on posts (tenant_id, title);
```

Kolom tenant pada tabel anak mengikuti induknya; jangan bergantung pada join untuk menyaring bila query bisa dijalankan langsung dengan `tenant_id`.

## Scoping query

```rust,ignore
use sqlx::postgres::Postgres;
use sqlx::QueryBuilder;
use tenancy::single_db::scope_query;

let mut query = QueryBuilder::<Postgres>::new(
    "select id, title from posts where published = true and ",
);
scope_query(&mut query, &tenant, None);
// SQL: ... and "tenant_id" = $1, bind tenant.id
```

Aturan penempatan: helper hanya menambahkan predikat `"tenant_id" = $n`. Pemanggil yang menyisipkan `where` atau `and` sebelumnya. Jangan mengandalkan helper untuk menebak posisi.

Model dengan kolom berbeda memakai `TenantScoped`:

```rust,ignore
use tenancy::single_db::{scope_query_for, TenantScoped};

struct Post;
impl TenantScoped for Post {
    const TENANT_COLUMN: &'static str = "school_id";
}

scope_query_for::<Post>(&mut query, &tenant, Some("posts"));
// SQL: "posts"."school_id" = $1
```

Join dan alias:

```rust,ignore
let mut query = QueryBuilder::<Postgres>::new(
    "select p.* from posts p join comments c on c.post_id = p.id where ",
);
scope_query(&mut query, &tenant, Some("p"));
```

Selalu beri alias pada setiap tabel yang ikut query; tanpa alias, nama kolom bisa ambigu.

## tenant_unique vs tenant_exists

| Fungsi | `true` berarti | Padanan Laravel |
| --- | --- | --- |
| `tenant_unique` | nilai masih bebas dipakai | rule `unique` |
| `tenant_exists` | nilai sudah ada | rule `exists` |

```rust,ignore
let free = tenancy::single_db::tenant_unique(&pool, &tenant, "posts", "title", &title).await?;
if !free {
    return Err("judul sudah dipakai".into());
}
```

Nama tabel dan kolom wajib `&'static str` (konstanta kode). Jangan pernah mengisi dari input pengguna.

## RLS opt-in

RLS memberi jaring pengaman di database, tetapi bukan pengganti scoping eksplisit. Aktifkan feature `postgres-rls`.

```sql
-- contoh policy
alter table posts enable row level security;
alter table posts force row level security;

create policy posts_tenant on posts
    using (tenant_id = current_setting('app.tenant_id', true)::bigint)
    with check (tenant_id = current_setting('app.tenant_id', true)::bigint);
```

Syarat: role koneksi bukan superuser dan bukan pemilik tabel dengan `BYPASSRLS`; tanpa `FORCE ROW LEVEL SECURITY`, pemilik tabel tetap melihat semua baris.

```rust,ignore
let mut tx = tenancy::single_db::rls::begin_tenant_tx(&pool, &tenant).await?;
let rows = sqlx::query("select * from posts")
    .fetch_all(tx.connection())
    .await?;
tx.rollback().await?;
```

`set_config('app.tenant_id', ..., true)` bersifat transaksi-lokal: commit/rollback mengembalikan setting ke kosong walau koneksi kembali ke pool.

## Langkah operasional

1. Tambahkan `tenant_id` dan index-nya pada migrasi tabel.
2. Ganti unique global dengan unique komposit `(tenant_id, ...)`.
3. Pasang helper scoping pada semua query tenant; query pusat dibiarkan.
4. Bila memakai RLS: buat role aplikasi non-superuser, aktifkan `FORCE ROW LEVEL SECURITY`, lalu uji dengan dua tenant.
5. Ukur query plan setelah index ditambah.

## Pitfall

- `tenant_unique` mengembalikan `true` saat nilai belum dipakai; jangan tertukar dengan `tenant_exists`.
- Scoping tidak otomatis: query baru tanpa helper akan membaca semua tenant.
- RLS pada koneksi superuser tidak berefek.
- Alias dinamis dari input pengguna membuka SQL injection; tipe `&'static str` mencegahnya di level kompilasi.
