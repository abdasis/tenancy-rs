use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tenancy::ports::memory::MemoryCache;
use tenancy::repository::memory::MemoryTenantRepository;
use tenancy::{
    current_context, Bootstrapper, CacheStore, Tenancy, TenancyError, TenancyEvent,
    TenancyListener, Tenant, TenantContext,
};

fn tenant(slug: &str) -> Tenant {
    Tenant::new(format!("Tenant {slug}"), slug).expect("valid tenant")
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn push(&self, value: impl Into<String>) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(value.into());
    }

    fn events(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct RecordingListener {
    recorder: Recorder,
}

#[async_trait]
impl TenancyListener for RecordingListener {
    async fn handle(&self, event: &TenancyEvent) -> Result<(), TenancyError> {
        let label = match event {
            TenancyEvent::Bootstrapping(_) => "bootstrapping",
            TenancyEvent::Bootstrapped(_) => "bootstrapped",
            TenancyEvent::Ending(_) => "ending",
            TenancyEvent::Ended(_) => "ended",
            _ => "tenant-event",
        };
        self.recorder.push(label);
        Ok(())
    }
}

struct RecordingBootstrapper {
    name: &'static str,
    recorder: Recorder,
    fail: bool,
}

impl RecordingBootstrapper {
    fn new(name: &'static str, recorder: Recorder) -> Self {
        Self {
            name,
            recorder,
            fail: false,
        }
    }

    fn failing(self) -> Self {
        Self { fail: true, ..self }
    }
}

#[async_trait]
impl Bootstrapper for RecordingBootstrapper {
    async fn bootstrap(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        self.recorder.push(format!("bootstrap:{}", self.name));
        if self.fail {
            return Err(TenancyError::Bootstrapper(format!("{} failed", self.name)));
        }
        Ok(())
    }

    async fn revert(&self, _context: &TenantContext) -> Result<(), TenancyError> {
        self.recorder.push(format!("revert:{}", self.name));
        Ok(())
    }
}

fn tenancy_with(recorder: &Recorder, bootstrappers: Vec<Arc<dyn Bootstrapper>>) -> Tenancy {
    let mut builder = Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .listener(Arc::new(RecordingListener {
            recorder: recorder.clone(),
        }));
    for bootstrapper in bootstrappers {
        builder = builder.bootstrapper(bootstrapper);
    }
    builder.build().expect("tenancy")
}

#[tokio::test]
async fn run_bootstraps_forward_and_reverts_in_reverse() {
    let recorder = Recorder::default();
    let tenancy = tenancy_with(
        &recorder,
        vec![
            Arc::new(RecordingBootstrapper::new("1", recorder.clone())),
            Arc::new(RecordingBootstrapper::new("2", recorder.clone())),
        ],
    );

    let value = tenancy
        .run(tenant("a"), || async { 42 })
        .await
        .expect("run");

    assert_eq!(value, 42);
    assert_eq!(
        recorder.events(),
        [
            "bootstrapping",
            "bootstrap:1",
            "bootstrap:2",
            "bootstrapped",
            "ending",
            "revert:2",
            "revert:1",
            "ended"
        ]
    );
}

#[tokio::test]
async fn current_context_is_scoped_to_run() {
    let tenancy = tenancy_with(&Recorder::default(), Vec::new());

    let seen = tenancy
        .run(tenant("a"), || async {
            current_context().map(|context| context.tenant().slug.clone())
        })
        .await
        .expect("run");

    assert_eq!(seen, Some("a".to_string()));
    assert!(current_context().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_tasks_see_their_own_tenant() {
    let tenancy = Arc::new(tenancy_with(&Recorder::default(), Vec::new()));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let mut handles = Vec::new();
    for expected in ["a", "b"] {
        let tenancy = Arc::clone(&tenancy);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            tenancy
                .run(tenant(expected), || async {
                    barrier.wait().await;
                    let before = current_context().expect("context").tenant().slug.clone();
                    tokio::task::yield_now().await;
                    let after = current_context().expect("context").tenant().slug.clone();
                    assert_eq!(before, expected);
                    assert_eq!(after, expected);
                })
                .await
                .expect("run");
        }));
    }

    for handle in handles {
        handle.await.expect("join");
    }
}

#[tokio::test]
async fn spawn_carries_tenant_into_new_task() {
    let tenancy = Arc::new(tenancy_with(&Recorder::default(), Vec::new()));

    let observed = tenancy
        .spawn(tenant("b"), || async {
            current_context().map(|context| context.tenant().slug.clone())
        })
        .await
        .expect("join")
        .expect("run");

    assert_eq!(observed, Some("b".to_string()));
}

#[tokio::test]
async fn failing_bootstrapper_reverts_started_ones() {
    let recorder = Recorder::default();
    let tenancy = tenancy_with(
        &recorder,
        vec![
            Arc::new(RecordingBootstrapper::new("1", recorder.clone())),
            Arc::new(RecordingBootstrapper::new("2", recorder.clone()).failing()),
        ],
    );

    let result = tenancy.run(tenant("a"), || async {}).await;

    assert!(matches!(result, Err(TenancyError::Bootstrapper(_))));
    assert_eq!(
        recorder.events(),
        ["bootstrapping", "bootstrap:1", "bootstrap:2", "revert:1"]
    );
}

#[tokio::test]
async fn nested_run_restores_outer_context_when_inner_is_dropped() {
    let tenancy = tenancy_with(&Recorder::default(), Vec::new());

    let outer = tenancy
        .run(tenant("a"), || async {
            let inner = tenancy.run(tenant("b"), || async {
                tokio::time::sleep(Duration::from_secs(5)).await;
            });
            drop(inner);
            tokio::task::yield_now().await;
            current_context().map(|context| context.tenant().slug.clone())
        })
        .await
        .expect("outer run");

    assert_eq!(outer, Some("a".to_string()));
}

#[tokio::test]
async fn scoped_cache_uses_tenant_namespace_inside_run() {
    let cache = Arc::new(MemoryCache::new());
    let tenancy = Tenancy::builder()
        .repository(Arc::new(MemoryTenantRepository::new()))
        .cache(cache.clone())
        .build()
        .expect("tenancy");

    tenancy
        .run(tenant("acme"), || async {
            tenancy
                .cache()
                .expect("cache")
                .set("x", b"y".to_vec(), None)
                .await
                .expect("set");
        })
        .await
        .expect("run");

    assert_eq!(
        cache.get("tenant:acme:x").await.expect("raw get"),
        Some(b"y".to_vec())
    );
    assert!(matches!(
        tenancy.cache().expect("cache").get("x").await,
        Err(TenancyError::NotResolved)
    ));
}

#[tokio::test]
async fn manual_initialize_and_end_emit_lifecycle_events() {
    let recorder = Recorder::default();
    let tenancy = tenancy_with(&recorder, Vec::new());

    let handle = tenancy.initialize(tenant("a")).await.expect("initialize");
    assert_eq!(handle.context().tenant().slug, "a");
    tenancy.end(handle).await.expect("end");

    assert_eq!(
        recorder.events(),
        ["bootstrapping", "bootstrapped", "ending", "ended"]
    );
    assert!(current_context().is_none());
}
