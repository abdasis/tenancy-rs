//! Axum integration (feature `axum`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ::axum::extract::{FromRequestParts, Request, State};
use ::axum::http::request::Parts;
use ::axum::http::{header, StatusCode};
use ::axum::middleware::{self, Next};
use ::axum::response::{IntoResponse, Response};
use ::axum::Router;

use crate::config::TenancyConfig;
use crate::error::TenancyError;
use crate::manager::Tenancy;
use crate::resolver::RequestInfo;
use crate::tenant::{normalize_host, Tenant};

/// Extractor for the tenant installed by the middleware.
#[derive(Debug, Clone)]
pub struct CurrentTenant(pub Tenant);

impl<S> FromRequestParts<S> for CurrentTenant
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Tenant>()
            .cloned()
            .map(CurrentTenant)
            .ok_or(StatusCode::NOT_FOUND)
    }
}

/// Extractor for universal routes; `None` when no tenant matched.
#[derive(Debug, Clone)]
pub struct OptionalTenant(pub Option<Tenant>);

impl<S> FromRequestParts<S> for OptionalTenant
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalTenant(parts.extensions.get::<Tenant>().cloned()))
    }
}

/// 503 response with a `Retry-After` header.
pub fn maintenance_response(retry_after: Duration) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, retry_after.as_secs().to_string())],
    )
        .into_response()
}

/// Middleware resolving the tenant, checking maintenance and running the lifecycle.
pub async fn tenancy_middleware(
    State(tenancy): State<Arc<Tenancy>>,
    mut request: Request,
    next: Next,
) -> Response {
    let info = request_info(&request);
    let tenant = match tenancy.resolve(&info).await {
        Ok(tenant) => tenant,
        Err(TenancyError::NotResolved) | Err(TenancyError::NotFound(_)) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => {
            tracing::warn!(%error, "tenant resolution failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if tenant.maintenance && !bypassed(tenancy.config(), &info) {
        return maintenance_response(tenancy.config().maintenance_retry_after);
    }

    request.extensions_mut().insert(tenant.clone());
    match tenancy.run(tenant, || next.run(request)).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "tenancy lifecycle failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Universal counterpart of [`tenancy_middleware`]: requests without a tenant pass through.
pub async fn universal_middleware(
    State(tenancy): State<Arc<Tenancy>>,
    mut request: Request,
    next: Next,
) -> Response {
    let info = request_info(&request);
    match tenancy.resolve(&info).await {
        Ok(tenant) => {
            if tenant.maintenance && !bypassed(tenancy.config(), &info) {
                return maintenance_response(tenancy.config().maintenance_retry_after);
            }
            request.extensions_mut().insert(tenant.clone());
            match tenancy.run(tenant, || next.run(request)).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(%error, "tenancy lifecycle failed");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(TenancyError::NotResolved) | Err(TenancyError::NotFound(_)) => next.run(request).await,
        Err(error) => {
            tracing::warn!(%error, "tenant resolution failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Installs [`tenancy_middleware`] on a tenant router.
///
/// ```no_run
/// use std::sync::Arc;
///
/// use axum::{routing::get, Router};
/// use tenancy::axum::tenant_router;
/// use tenancy::repository::memory::MemoryTenantRepository;
/// use tenancy::Tenancy;
///
/// let tenancy = Arc::new(
///     Tenancy::builder()
///         .repository(Arc::new(MemoryTenantRepository::new()))
///         .build()?,
/// );
/// let app = tenant_router(tenancy, Router::new().route("/", get(|| async {})));
/// # Ok::<_, tenancy::TenancyError>(())
/// ```
pub fn tenant_router(tenancy: Arc<Tenancy>, router: Router) -> Router {
    router.layer(middleware::from_fn_with_state(tenancy, tenancy_middleware))
}

/// Installs [`universal_middleware`] on a router shared by tenant and central routes.
pub fn universal_router(tenancy: Arc<Tenancy>, router: Router) -> Router {
    router.layer(middleware::from_fn_with_state(
        tenancy,
        universal_middleware,
    ))
}

/// Absolute URL for `path` on the tenant primary domain.
pub fn tenant_url(tenancy: &Tenancy, tenant: &Tenant, path: &str) -> Result<String, TenancyError> {
    let domain = tenant
        .primary_domain()
        .ok_or_else(|| TenancyError::Config("tenant has no primary domain".to_string()))?;
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(format!(
        "{}://{domain}{path}",
        tenancy.config().public_scheme
    ))
}

fn bypassed(config: &TenancyConfig, info: &RequestInfo) -> bool {
    config
        .maintenance_bypass
        .as_ref()
        .is_some_and(|bypass| bypass(info))
}

fn request_info(request: &Request) -> RequestInfo {
    let headers: HashMap<String, String> = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect();

    let host = headers
        .get("host")
        .cloned()
        .or_else(|| request.uri().host().map(str::to_string))
        .unwrap_or_default();

    // shortcut: query/cookie values are not percent-decoded, decode when hosts need it
    let query = request
        .uri()
        .query()
        .map(|query| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let cookies = headers
        .get("cookie")
        .map(|cookies| {
            cookies
                .split(';')
                .filter_map(|pair| pair.trim().split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect()
        })
        .unwrap_or_default();

    RequestInfo {
        method: request.method().as_str().to_string(),
        path: request.uri().path().to_string(),
        host: normalize_host(&host),
        headers,
        cookies,
        query,
    }
}
