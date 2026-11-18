//! Minimal axum service resolving tenants from the `X-Tenant` header.

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tenancy::axum::{tenant_router, CurrentTenant};
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::resolver::request_data::HeaderResolver;
use tenancy::{Tenancy, Tenant, TenantId};

async fn whoami(CurrentTenant(tenant): CurrentTenant) -> String {
    format!("{} ({})", tenant.name, tenant.slug)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repository = Arc::new(MemoryTenantRepository::new());
    let mut tenant = Tenant::new("Acme", "acme")?;
    tenant.id = TenantId(1);
    repository.insert(tenant);

    let tenancy = Arc::new(
        Tenancy::builder()
            .repository(repository)
            .resolver(Arc::new(HeaderResolver::new("x-tenant")))
            .build()?,
    );

    let app = tenant_router(tenancy, Router::new().route("/whoami", get(whoami)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("listening on http://127.0.0.1:3000 with header x-tenant: acme");
    axum::serve(listener, app).await?;
    Ok(())
}
