#![cfg(feature = "postgres")]

use sqlx::postgres::Postgres;
use sqlx::{PgPool, QueryBuilder};
use tenancy::single_db::{
    scope_query, scope_query_for, tenant_exists, tenant_unique, TenantScoped,
};
use tenancy::{Tenant, TenantId};

fn tenant(id: i64) -> Tenant {
    let mut tenant = Tenant::new("Acme", "acme").expect("valid tenant");
    tenant.id = TenantId(id);
    tenant
}

#[test]
fn scope_query_without_alias_targets_tenant_column() {
    let mut query =
        QueryBuilder::<Postgres>::new("select * from posts where published = true and ");
    scope_query(&mut query, &tenant(7), None);
    assert!(query.sql().contains("\"tenant_id\" = $1"));
}

#[test]
fn scope_query_with_alias_qualifies_column() {
    let mut query = QueryBuilder::<Postgres>::new("select * from posts and ");
    scope_query(&mut query, &tenant(7), Some("posts"));
    assert!(query.sql().contains("\"posts\".\"tenant_id\" = $1"));
}

struct School;

impl TenantScoped for School {
    const TENANT_COLUMN: &'static str = "school_id";
}

#[test]
fn tenant_scoped_model_uses_custom_column() {
    let mut query = QueryBuilder::<Postgres>::new("select * from posts and ");
    scope_query_for::<School>(&mut query, &tenant(7), Some("posts"));
    assert!(query.sql().contains("\"posts\".\"school_id\" = $1"));
}

async fn pool() -> Option<PgPool> {
    match std::env::var("TEST_DATABASE_URL") {
        Ok(url) => Some(
            sqlx::PgPool::connect(&url)
                .await
                .expect("connect TEST_DATABASE_URL"),
        ),
        Err(_) => {
            eprintln!("skipping postgres test: TEST_DATABASE_URL is not set");
            None
        }
    }
}

#[tokio::test]
async fn scoped_query_binds_tenant_id_in_real_query() {
    let Some(pool) = pool().await else { return };

    let mut query = QueryBuilder::<Postgres>::new(
        "select count(*) from (values (7::bigint), (8::bigint)) as posts(tenant_id) where ",
    );
    scope_query(&mut query, &tenant(7), None);
    let count: i64 = query
        .build_query_scalar()
        .fetch_one(&pool)
        .await
        .expect("scoped query");

    assert_eq!(count, 1);
}

#[tokio::test]
async fn tenant_unique_and_exists_scope_by_tenant_column() {
    let Some(pool) = pool().await else { return };

    sqlx::raw_sql(
        "create table if not exists tenancy_scoped_items (tenant_id bigint not null, name text not null)",
    )
    .execute(&pool)
    .await
    .expect("create table");
    sqlx::raw_sql("delete from tenancy_scoped_items")
        .execute(&pool)
        .await
        .expect("truncate");
    sqlx::raw_sql("insert into tenancy_scoped_items (tenant_id, name) values (7, 'a'), (8, 'a')")
        .execute(&pool)
        .await
        .expect("seed");

    // tenant_unique: true berarti nilai masih bebas dipakai
    assert!(
        !tenant_unique(&pool, &tenant(7), "tenancy_scoped_items", "name", "a")
            .await
            .expect("unique hit")
    );
    assert!(
        tenant_unique(&pool, &tenant(7), "tenancy_scoped_items", "name", "b")
            .await
            .expect("unique miss")
    );
    // tenant_exists: true berarti nilai sudah ada
    assert!(
        tenant_exists(&pool, &tenant(8), "tenancy_scoped_items", "name", "a")
            .await
            .expect("exists hit")
    );
    assert!(
        !tenant_exists(&pool, &tenant(7), "tenancy_scoped_items", "name", "b")
            .await
            .expect("exists miss")
    );

    sqlx::raw_sql("drop table tenancy_scoped_items")
        .execute(&pool)
        .await
        .expect("drop table");
}

#[cfg(feature = "postgres-rls")]
mod rls {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use tenancy::single_db::rls::begin_tenant_tx;

    #[tokio::test]
    async fn tenant_transaction_sets_local_setting_and_resets_after_rollback() {
        let Some(_) = pool().await else { return };
        let url = std::env::var("TEST_DATABASE_URL").expect("checked above");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");

        let mut tx = begin_tenant_tx(&pool, &tenant(7)).await.expect("begin");
        let current: Option<String> =
            sqlx::query_scalar("select current_setting('app.tenant_id', true)")
                .fetch_one(tx.connection())
                .await
                .expect("read setting");
        assert_eq!(current.as_deref(), Some("7"));

        drop(tx);

        let after: Option<String> =
            sqlx::query_scalar("select current_setting('app.tenant_id', true)")
                .fetch_one(&pool)
                .await
                .expect("read setting after rollback");
        assert!(
            after.as_deref().is_none_or(str::is_empty),
            "setting must reset after rollback, got {after:?}"
        );
    }
}
