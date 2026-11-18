#![cfg(feature = "testing")]

use std::sync::{Arc, Mutex};

use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::testing::{fake_tenancy, tenant, FakeBootstrapper, RecordingListener};
use tenancy::{current_context, Tenancy, TenancyEvent, TenantId};

#[tokio::test]
async fn fake_tenancy_runs_with_context() {
    let tenancy = fake_tenancy();

    let observed = tenancy
        .run(tenant("acme"), || async {
            current_context().map(|context| context.tenant().slug.clone())
        })
        .await
        .expect("run");

    assert_eq!(observed, Some("acme".to_string()));
}

#[tokio::test]
async fn fake_bootstrapper_records_forward_and_reverse_calls() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let tenancy = Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .bootstrapper(Arc::new(FakeBootstrapper::new("db", log.clone())))
        .build()
        .expect("tenancy");

    tenancy.run(tenant("acme"), || async {}).await.expect("run");

    let entries = log
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(entries, ["bootstrap:db", "revert:db"]);
}

#[tokio::test]
async fn recording_listener_captures_lifecycle_events() {
    let listener = Arc::new(RecordingListener::new());
    let tenancy = Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .listener(listener.clone())
        .build()
        .expect("tenancy");

    tenancy.run(tenant("acme"), || async {}).await.expect("run");

    assert_eq!(
        listener.events(),
        [
            TenancyEvent::Bootstrapping(TenantId(0)),
            TenancyEvent::Bootstrapped(TenantId(0)),
            TenancyEvent::Ending(TenantId(0)),
            TenancyEvent::Ended(TenantId(0))
        ]
    );
}
