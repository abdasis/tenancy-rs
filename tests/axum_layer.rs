#![cfg(feature = "axum")]

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tenancy::axum::{
    maintenance_response, tenant_router, tenant_url, universal_router, CurrentTenant,
    OptionalTenant,
};
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::resolver::request_data::HeaderResolver;
use tenancy::{Domain, Tenancy, TenancyConfig, Tenant, TenantId};
use tower::ServiceExt;

const MAINTENANCE_RETRY_AFTER: Duration = Duration::from_secs(60);

fn seed() -> Arc<MemoryTenantRepository> {
    let repository = Arc::new(MemoryTenantRepository::new());

    let mut acme = Tenant::new("Acme", "acme").expect("valid tenant");
    acme.id = TenantId(1);
    acme.domains.push(Domain::primary("acme.example.com"));
    repository.insert(acme);

    let mut paused = Tenant::new("Paused", "paused").expect("valid tenant");
    paused.id = TenantId(2);
    paused.maintenance = true;
    paused.domains.push(Domain::primary("paused.example.com"));
    repository.insert(paused);

    repository
}

fn tenancy_with_config(config: TenancyConfig) -> Arc<Tenancy> {
    Arc::new(
        Tenancy::builder()
            .repository(seed())
            .resolver(Arc::new(HeaderResolver::new("x-tenant")))
            .config(config)
            .build()
            .expect("tenancy"),
    )
}

fn tenancy(bypass_platform_admin: bool) -> Arc<Tenancy> {
    let mut config = TenancyConfig::default();
    if bypass_platform_admin {
        config.maintenance_bypass = Some(Arc::new(|info: &tenancy::RequestInfo| {
            info.header("x-platform-admin") == Some("1")
        }));
    }
    tenancy_with_config(config)
}

async fn whoami(CurrentTenant(tenant): CurrentTenant) -> String {
    tenant.slug
}

async fn optional_whoami(OptionalTenant(tenant): OptionalTenant) -> String {
    tenant
        .map(|tenant| tenant.slug)
        .unwrap_or_else(|| "none".to_string())
}

async fn send(app: Router, uri: &str, tenant_header: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::builder().uri(uri);
    if let Some(slug) = tenant_header {
        request = request.header("x-tenant", slug);
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&body).to_string())
}

fn app(tenancy: Arc<Tenancy>) -> Router {
    tenant_router(tenancy, Router::new().route("/whoami", get(whoami)))
}

#[tokio::test]
async fn request_with_tenant_reaches_handler() {
    let (status, body) = send(app(tenancy(false)), "/whoami", Some("acme")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "acme");
}

#[tokio::test]
async fn request_without_tenant_is_404() {
    let (status, _) = send(app(tenancy(false)), "/whoami", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_tenant_is_404() {
    let (status, _) = send(app(tenancy(false)), "/whoami", Some("missing")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_in_maintenance_returns_503_with_retry_after() {
    let request = Request::builder()
        .uri("/whoami")
        .header("x-tenant", "paused")
        .body(Body::empty())
        .expect("request");
    let response = app(tenancy(false))
        .oneshot(request)
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("60")
    );
}

#[tokio::test]
async fn maintenance_bypass_reaches_handler() {
    let request = Request::builder()
        .uri("/whoami")
        .header("x-tenant", "paused")
        .header("x-platform-admin", "1")
        .body(Body::empty())
        .expect("request");
    let response = app(tenancy(true)).oneshot(request).await.expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn universal_route_accepts_requests_with_and_without_tenant() {
    let app = universal_router(
        tenancy(false),
        Router::new().route("/whoami", get(optional_whoami)),
    );

    let (status, body) = send(app.clone(), "/whoami", Some("acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "acme");

    let (status, body) = send(app, "/whoami", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "none");
}

#[tokio::test]
async fn extractor_without_middleware_is_404() {
    let app = Router::new().route("/whoami", get(whoami));

    let (status, _) = send(app, "/whoami", Some("acme")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn maintenance_response_sets_retry_after() {
    let response = maintenance_response(MAINTENANCE_RETRY_AFTER);
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("60")
    );
}

#[test]
fn tenant_url_uses_primary_domain() {
    let tenancy = tenancy(false);
    let mut tenant = Tenant::new("Acme", "acme").expect("valid tenant");
    tenant.domains.push(Domain::primary("acme.example.com"));

    assert_eq!(
        tenant_url(&tenancy, &tenant, "/dashboard").expect("url"),
        "https://acme.example.com/dashboard"
    );
    assert_eq!(
        tenant_url(&tenancy, &tenant, "inbox").expect("url"),
        "https://acme.example.com/inbox"
    );
}

#[test]
fn tenant_url_requires_a_domain() {
    let tenancy = tenancy(false);
    let tenant = Tenant::new("Acme", "acme").expect("valid tenant");
    assert!(matches!(
        tenant_url(&tenancy, &tenant, "/"),
        Err(tenancy::TenancyError::Config(_))
    ));
}

#[tokio::test]
async fn maintenance_retry_after_uses_config() {
    let config = TenancyConfig {
        maintenance_retry_after: Duration::from_secs(120),
        ..TenancyConfig::default()
    };
    let request = Request::builder()
        .uri("/whoami")
        .header("x-tenant", "paused")
        .body(Body::empty())
        .expect("request");
    let response = app(tenancy_with_config(config))
        .oneshot(request)
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("120")
    );
}

#[test]
fn tenant_url_uses_configured_scheme() {
    let config = TenancyConfig {
        public_scheme: "http".to_string(),
        ..TenancyConfig::default()
    };
    let tenancy = tenancy_with_config(config);
    let mut tenant = Tenant::new("Acme", "acme").expect("valid tenant");
    tenant.domains.push(Domain::primary("acme.example.com"));

    assert_eq!(
        tenant_url(&tenancy, &tenant, "/dashboard").expect("url"),
        "http://acme.example.com/dashboard"
    );
}
