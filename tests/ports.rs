use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tenancy::ports::local::LocalStorage;
use tenancy::ports::memory::{MemoryCache, MemoryJobQueue, MemorySessionStore};
use tenancy::{
    CacheStore, JobQueue, Loader, SessionStore, Storage, TenancyError, TenantId, TenantJob,
};

fn temp_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tenancy-test-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn counting_loader(calls: Arc<AtomicUsize>) -> Loader<'static> {
    Box::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(b"loaded".to_vec()) })
            as Pin<Box<dyn Future<Output = Result<Vec<u8>, TenancyError>> + Send>>
    })
}

#[tokio::test]
async fn remember_returns_cached_value_without_calling_loader() {
    let cache = MemoryCache::new();
    cache.set("k", b"cached".to_vec(), None).await.expect("set");

    let calls = Arc::new(AtomicUsize::new(0));
    let value = cache
        .remember("k", None, counting_loader(Arc::clone(&calls)))
        .await
        .expect("remember");

    assert_eq!(value, b"cached".to_vec());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn remember_loads_and_stores_on_miss() {
    let cache = MemoryCache::new();
    let calls = Arc::new(AtomicUsize::new(0));

    let value = cache
        .remember("k", None, counting_loader(Arc::clone(&calls)))
        .await
        .expect("remember");

    assert_eq!(value, b"loaded".to_vec());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(cache.get("k").await.expect("get"), Some(b"loaded".to_vec()));
}

#[tokio::test]
async fn memory_cache_stores_reads_and_deletes() {
    let cache = MemoryCache::new();
    cache
        .set("tenant:a:x", b"1".to_vec(), None)
        .await
        .expect("set");
    assert_eq!(
        cache.get("tenant:a:x").await.expect("get"),
        Some(b"1".to_vec())
    );

    cache.delete("tenant:a:x").await.expect("delete");
    assert_eq!(cache.get("tenant:a:x").await.expect("get"), None);
}

#[tokio::test(start_paused = true)]
async fn memory_cache_expires_after_ttl() {
    let cache = MemoryCache::new();
    cache
        .set("a", b"1".to_vec(), Some(Duration::from_secs(60)))
        .await
        .expect("set");

    assert_eq!(cache.get("a").await.expect("get"), Some(b"1".to_vec()));

    tokio::time::advance(Duration::from_secs(61)).await;
    assert_eq!(cache.get("a").await.expect("get"), None);
}

#[tokio::test]
async fn memory_cache_delete_prefix_only_removes_matching_keys() {
    let cache = MemoryCache::new();
    for key in ["tenant:a:x", "tenant:a:y", "tenant:b:z"] {
        cache.set(key, b"v".to_vec(), None).await.expect("set");
    }

    cache
        .delete_prefix("tenant:a:")
        .await
        .expect("delete prefix");

    assert_eq!(cache.get("tenant:a:x").await.expect("get"), None);
    assert_eq!(cache.get("tenant:a:y").await.expect("get"), None);
    assert_eq!(
        cache.get("tenant:b:z").await.expect("get"),
        Some(b"v".to_vec())
    );
}

#[tokio::test]
async fn memory_session_store_roundtrip() {
    let sessions = MemorySessionStore::new();
    sessions
        .set("tenant:a:sid", b"payload".to_vec())
        .await
        .expect("set");
    assert_eq!(
        sessions.get("tenant:a:sid").await.expect("get"),
        Some(b"payload".to_vec())
    );

    sessions
        .delete_prefix("tenant:a:")
        .await
        .expect("delete prefix");
    assert_eq!(sessions.get("tenant:a:sid").await.expect("get"), None);
    assert_eq!(sessions.get("tenant:a:other").await.expect("get"), None);
}

#[tokio::test]
async fn memory_job_queue_records_tenant() {
    let queue = MemoryJobQueue::new();
    queue
        .enqueue(TenantJob {
            tenant_id: TenantId(3),
            name: "send-report".to_string(),
            payload: serde_json::json!({ "report": 9 }),
        })
        .await
        .expect("enqueue");

    let enqueued = queue.enqueued();
    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].tenant_id, TenantId(3));
}

#[tokio::test]
async fn local_storage_writes_and_reads_relative_paths() {
    let root = temp_root("storage");
    let storage = LocalStorage::new(root.clone()).expect("storage");

    storage
        .put("a/b.txt", b"hello".to_vec())
        .await
        .expect("put");
    assert!(storage.exists("a/b.txt").await.expect("exists"));
    assert_eq!(
        storage.get("a/b.txt").await.expect("get"),
        b"hello".to_vec()
    );

    storage.delete("a/b.txt").await.expect("delete");
    assert!(!storage.exists("a/b.txt").await.expect("exists"));
}

#[tokio::test]
async fn local_storage_rejects_traversal_paths() {
    let root = temp_root("traversal");
    let storage = LocalStorage::new(root.clone()).expect("storage");

    for path in ["../escape.txt", "/etc/passwd", "a/../../escape.txt"] {
        let result = match path {
            "a/../../escape.txt" => storage.get(path).await.map(|_| ()),
            _ => storage.put(path, b"x".to_vec()).await,
        };
        assert!(
            matches!(result, Err(TenancyError::PathTraversal)),
            "path {path} should be rejected, got {result:?}"
        );
    }

    let escaped = root
        .parent()
        .expect("temp dir has parent")
        .join("escape.txt");
    assert!(!escaped.exists(), "traversal must not create files");
}

#[tokio::test]
async fn local_storage_delete_prefix_removes_directory_tree() {
    let root = temp_root("prefix");
    let storage = LocalStorage::new(root.clone()).expect("storage");

    storage.put("acme/a.txt", b"1".to_vec()).await.expect("put");
    storage
        .put("acme/nested/b.txt", b"2".to_vec())
        .await
        .expect("put");
    storage
        .put("other/c.txt", b"3".to_vec())
        .await
        .expect("put");

    storage.delete_prefix("acme").await.expect("delete prefix");

    assert!(!storage.exists("acme/a.txt").await.expect("exists"));
    assert!(!storage.exists("acme/nested/b.txt").await.expect("exists"));
    assert!(storage.exists("other/c.txt").await.expect("exists"));
    assert!(!root.join("acme").exists());
}

#[cfg(feature = "encryption")]
mod encryption {
    use tenancy::encryption::AesGcmEncryptor;
    use tenancy::{Encryptor, TenancyError};

    #[tokio::test]
    async fn encryptor_roundtrips_plaintext() {
        let encryptor = AesGcmEncryptor::from_key([7u8; 32]);
        let sealed = encryptor.seal(b"secret").await.expect("seal");
        assert_ne!(sealed, b"secret");
        assert_eq!(encryptor.open(&sealed).await.expect("open"), b"secret");
    }

    #[tokio::test]
    async fn encryptor_uses_fresh_nonce_per_seal() {
        let encryptor = AesGcmEncryptor::from_key([7u8; 32]);
        let first = encryptor.seal(b"secret").await.expect("seal");
        let second = encryptor.seal(b"secret").await.expect("seal");
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn encryptor_rejects_wrong_key() {
        let sealer = AesGcmEncryptor::from_key([7u8; 32]);
        let opener = AesGcmEncryptor::from_key([9u8; 32]);
        let sealed = sealer.seal(b"secret").await.expect("seal");

        assert!(matches!(
            opener.open(&sealed).await,
            Err(TenancyError::Crypto(_))
        ));
    }
}
