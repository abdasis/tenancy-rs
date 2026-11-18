use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tenancy::resolver::chain::ResolverChain;
use tenancy::resolver::domain::DomainResolver;
use tenancy::resolver::path::PathResolver;
use tenancy::resolver::request_data::{ClosureResolver, CookieResolver, HeaderResolver};
use tenancy::resolver::subdomain::SubdomainResolver;
use tenancy::{RequestInfo, TenantKey, TenantResolver};

fn request(host: &str, path: &str) -> RequestInfo {
    RequestInfo::new("GET", path, host)
}

#[tokio::test]
async fn domain_resolver_returns_normalized_host() {
    let resolver = DomainResolver::new();
    let key = resolver
        .resolve(&request("Springfield.Example.COM:8443", "/"))
        .await
        .expect("resolve");
    assert_eq!(
        key,
        Some(TenantKey::Domain("springfield.example.com".to_string()))
    );

    assert_eq!(
        resolver.resolve(&request("", "/")).await.expect("resolve"),
        None
    );
}

#[tokio::test]
async fn subdomain_resolver_accepts_single_label_below_central_domain() {
    let resolver = SubdomainResolver::new(vec!["example.com".to_string()]);

    assert_eq!(
        resolver
            .resolve(&request("springfield.example.com", "/"))
            .await
            .expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );

    for host in [
        "example.com",
        "www.example.com",
        "a.b.example.com",
        "other.net",
    ] {
        assert_eq!(
            resolver
                .resolve(&request(host, "/"))
                .await
                .expect("resolve"),
            None,
            "host {host} must not resolve"
        );
    }
}

#[tokio::test]
async fn path_resolver_reads_first_segment_after_prefix() {
    let resolver = PathResolver::new();

    assert_eq!(
        resolver
            .resolve(&request("example.com", "/t/springfield/api"))
            .await
            .expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );
    assert_eq!(
        resolver
            .resolve(&request("example.com", "/t/springfield"))
            .await
            .expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );
    assert_eq!(
        resolver
            .resolve(&request("example.com", "/api/springfield"))
            .await
            .expect("resolve"),
        None
    );
    assert_eq!(
        resolver
            .resolve(&request("example.com", "/t/"))
            .await
            .expect("resolve"),
        None
    );
}

#[tokio::test]
async fn header_and_cookie_resolvers_are_case_tolerant_for_headers() {
    let header = HeaderResolver::new("X-Tenant");
    let with_header = request("example.com", "/").with_header("x-tenant", "springfield");
    assert_eq!(
        header.resolve(&with_header).await.expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );

    let cookie = CookieResolver::new("tenant");
    let mut with_cookie = request("example.com", "/");
    with_cookie
        .cookies
        .insert("tenant".to_string(), "springfield".to_string());
    assert_eq!(
        cookie.resolve(&with_cookie).await.expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );

    let empty = request("", "/");
    assert_eq!(header.resolve(&empty).await.expect("resolve"), None);
    assert_eq!(cookie.resolve(&empty).await.expect("resolve"), None);
}

#[tokio::test]
async fn chain_stops_at_first_match() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = Arc::clone(&hits);
    let fallback = ClosureResolver::new(move |_request: &RequestInfo| {
        hits_clone.fetch_add(1, Ordering::SeqCst);
        Some(TenantKey::Slug("fallback".to_string()))
    });

    let chain = ResolverChain::new(vec![
        Arc::new(HeaderResolver::new("x-tenant")),
        Arc::new(fallback),
    ]);

    let matched = request("example.com", "/").with_header("X-Tenant", "springfield");
    assert_eq!(
        chain.resolve(&matched).await.expect("resolve"),
        Some(TenantKey::Slug("springfield".to_string()))
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "chain must stop at first match"
    );

    let unmatched = request("example.com", "/");
    assert_eq!(
        chain.resolve(&unmatched).await.expect("resolve"),
        Some(TenantKey::Slug("fallback".to_string()))
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chain_can_be_built_incrementally() {
    let mut chain = ResolverChain::new(Vec::new());
    chain.push(Arc::new(PathResolver::new()) as Arc<dyn TenantResolver>);
    assert_eq!(
        chain
            .resolve(&request("example.com", "/t/acme/x"))
            .await
            .expect("resolve"),
        Some(TenantKey::Slug("acme".to_string()))
    );
}
