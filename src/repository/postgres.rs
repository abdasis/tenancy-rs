//! PostgreSQL tenant repository (feature `postgres`).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};

use crate::error::TenancyError;
use crate::repository::TenantRepository;
use crate::resolver::TenantKey;
use crate::tenant::{normalize_host, ConfigValue, Domain, Tenant, TenantConfig, TenantId};

const TENANT_COLUMNS: &str =
    "id, name, slug, database, maintenance_mode, data, created_at, updated_at";

/// [`TenantRepository`] backed by the schema in [`crate::TENANCY_MIGRATION`].
pub struct PgTenantRepository {
    pool: PgPool,
}

impl PgTenantRepository {
    /// Creates the repository from a pool connected to the central database.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct TenantRow {
    id: i64,
    name: String,
    slug: String,
    database: Option<String>,
    maintenance_mode: bool,
    data: Json<serde_json::Map<String, serde_json::Value>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DomainRow {
    host: String,
    is_primary: bool,
}

#[derive(sqlx::FromRow)]
struct ConfigRow {
    key: String,
    value: String,
    encrypted: bool,
}

async fn hydrate(pool: &PgPool, row: TenantRow) -> Result<Tenant, TenancyError> {
    // shortcut: dua query tambahan per tenant, upgrade bila daftar tenant besar
    let domains = sqlx::query_as::<_, DomainRow>(
        "select host, is_primary from tenant_domains where tenant_id = $1 order by is_primary desc, host",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?;
    let configs = sqlx::query_as::<_, ConfigRow>(
        "select \"key\", value, encrypted from tenant_configs where tenant_id = $1",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?;

    let mut config = TenantConfig::default();
    for entry in configs {
        if entry.encrypted {
            config.set_encrypted(entry.key, entry.value);
        } else {
            config.set(entry.key, entry.value);
        }
    }

    Ok(Tenant {
        id: TenantId(row.id),
        name: row.name,
        slug: row.slug,
        domains: domains
            .into_iter()
            .map(|domain| Domain {
                host: domain.host,
                is_primary: domain.is_primary,
            })
            .collect(),
        database: row.database,
        maintenance: row.maintenance_mode,
        config,
        data: row.data.0,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn write_relations(
    connection: &mut PgConnection,
    id: i64,
    tenant: &Tenant,
) -> Result<(), sqlx::Error> {
    sqlx::query("delete from tenant_domains where tenant_id = $1")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    for domain in &tenant.domains {
        sqlx::query("insert into tenant_domains (tenant_id, host, is_primary) values ($1, $2, $3)")
            .bind(id)
            .bind(&domain.host)
            .bind(domain.is_primary)
            .execute(&mut *connection)
            .await?;
    }

    sqlx::query("delete from tenant_configs where tenant_id = $1")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    for (key, value) in tenant.config.iter() {
        sqlx::query(
            "insert into tenant_configs (tenant_id, \"key\", value, encrypted) values ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(key)
        .bind(&value.value)
        .bind(value.encrypted)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

#[async_trait]
impl TenantRepository for PgTenantRepository {
    async fn all(&self) -> Result<Vec<Tenant>, TenancyError> {
        let sql = format!("select {TENANT_COLUMNS} from tenants order by id");
        let rows = sqlx::query_as::<_, TenantRow>(&sql)
            .fetch_all(&self.pool)
            .await?;
        let mut tenants = Vec::with_capacity(rows.len());
        for row in rows {
            tenants.push(hydrate(&self.pool, row).await?);
        }
        Ok(tenants)
    }

    async fn find(&self, id: TenantId) -> Result<Option<Tenant>, TenancyError> {
        let sql = format!("select {TENANT_COLUMNS} from tenants where id = $1");
        let row = sqlx::query_as::<_, TenantRow>(&sql)
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => Ok(Some(hydrate(&self.pool, row).await?)),
            None => Ok(None),
        }
    }

    async fn find_by_key(&self, key: &TenantKey) -> Result<Option<Tenant>, TenancyError> {
        let row: Option<TenantRow> = match key {
            TenantKey::Id(id) => return self.find(*id).await,
            TenantKey::Slug(slug) => {
                let sql = format!("select {TENANT_COLUMNS} from tenants where slug = $1");
                sqlx::query_as::<_, TenantRow>(&sql)
                    .bind(slug)
                    .fetch_optional(&self.pool)
                    .await?
            }
            TenantKey::Domain(host) => {
                let sql = "select t.id, t.name, t.slug, t.database, t.maintenance_mode, t.data, t.created_at, t.updated_at \
                     from tenants t join tenant_domains d on d.tenant_id = t.id where d.host = $1 limit 1";
                sqlx::query_as::<_, TenantRow>(sql)
                    .bind(normalize_host(host))
                    .fetch_optional(&self.pool)
                    .await?
            }
        };
        match row {
            Some(row) => Ok(Some(hydrate(&self.pool, row).await?)),
            None => Ok(None),
        }
    }

    async fn create(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "insert into tenants (name, slug, database, maintenance_mode, data, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7) returning id",
        )
        .bind(&tenant.name)
        .bind(&tenant.slug)
        .bind(&tenant.database)
        .bind(tenant.maintenance)
        .bind(Json(&tenant.data))
        .bind(tenant.created_at)
        .bind(tenant.updated_at)
        .fetch_one(&mut *tx)
        .await?;
        write_relations(&mut tx, id, tenant).await?;
        tx.commit().await?;

        let mut created = tenant.clone();
        created.id = TenantId(id);
        Ok(created)
    }

    async fn update(&self, tenant: &Tenant) -> Result<Tenant, TenancyError> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "update tenants set name = $2, slug = $3, database = $4, maintenance_mode = $5, \
             data = $6, updated_at = $7 where id = $1",
        )
        .bind(tenant.id.0)
        .bind(&tenant.name)
        .bind(&tenant.slug)
        .bind(&tenant.database)
        .bind(tenant.maintenance)
        .bind(Json(&tenant.data))
        .bind(tenant.updated_at)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(TenancyError::NotFound(tenant.id.to_string()));
        }
        write_relations(&mut tx, tenant.id.0, tenant).await?;
        tx.commit().await?;
        Ok(tenant.clone())
    }

    async fn delete(&self, id: TenantId) -> Result<bool, TenancyError> {
        let result = sqlx::query("delete from tenants where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn save_config(
        &self,
        id: TenantId,
        key: &str,
        value: &ConfigValue,
    ) -> Result<(), TenancyError> {
        sqlx::query(
            "insert into tenant_configs (tenant_id, \"key\", value, encrypted) values ($1, $2, $3, $4) \
             on conflict (tenant_id, \"key\") do update set value = excluded.value, encrypted = excluded.encrypted",
        )
        .bind(id.0)
        .bind(key)
        .bind(&value.value)
        .bind(value.encrypted)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
