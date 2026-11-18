-- Skema pusat tenancy. Jalankan pada database pusat saja.
create table if not exists tenants (
    id bigserial primary key,
    name text not null,
    slug text not null unique,
    database text,
    maintenance_mode boolean not null default false,
    data jsonb not null default '{}'::jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table if not exists tenant_domains (
    id bigserial primary key,
    tenant_id bigint not null references tenants(id) on delete cascade,
    host text not null unique,
    is_primary boolean not null default false,
    created_at timestamptz not null default now()
);

create index if not exists tenant_domains_host_idx on tenant_domains (host);

create table if not exists tenant_configs (
    tenant_id bigint not null references tenants(id) on delete cascade,
    "key" text not null,
    value text not null,
    encrypted boolean not null default false,
    primary key (tenant_id, "key")
);

create index if not exists tenant_configs_tenant_idx on tenant_configs (tenant_id);
