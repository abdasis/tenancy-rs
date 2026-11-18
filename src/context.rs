//! Task-local tenant context and its typed slot store.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::tenant::{Tenant, TenantId};

tokio::task_local! {
    static CURRENT: TenantContext;
}

/// Current tenant context for the running task, when one is active.
pub fn current_context() -> Option<TenantContext> {
    CURRENT.try_with(Clone::clone).ok()
}

/// Context installed while a tenant lifecycle is active.
///
/// Holds the tenant, the namespaces derived from its slug and a typed slot
/// store used by bootstrappers (active pool, queue context, ...). Clones share
/// the same mutable scope, so bootstrappers can adjust it through `&self`.
#[derive(Clone)]
pub struct TenantContext {
    tenant: Tenant,
    scope: Arc<RwLock<Scope>>,
    slots: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
}

struct Scope {
    cache_namespace: String,
    session_namespace: String,
    queue_namespace: String,
    storage_root: PathBuf,
}

impl TenantContext {
    /// Builds a context for `tenant` with the default slug-based namespaces.
    pub fn new(tenant: Tenant) -> Self {
        let namespace = format!("tenant:{}", tenant.slug);
        Self {
            tenant,
            scope: Arc::new(RwLock::new(Scope {
                cache_namespace: namespace.clone(),
                session_namespace: namespace.clone(),
                queue_namespace: namespace,
                storage_root: PathBuf::new(),
            })),
            slots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Tenant owning this context.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    /// Tenant identifier owning this context.
    pub fn tenant_id(&self) -> TenantId {
        self.tenant.id
    }

    /// Cache key namespace, `tenant:{slug}` by default.
    pub fn cache_namespace(&self) -> String {
        self.scope().cache_namespace.clone()
    }

    /// Session key namespace, `tenant:{slug}` by default.
    pub fn session_namespace(&self) -> String {
        self.scope().session_namespace.clone()
    }

    /// Queue namespace, `tenant:{slug}` by default.
    pub fn queue_namespace(&self) -> String {
        self.scope().queue_namespace.clone()
    }

    /// Storage root for the tenant.
    pub fn storage_root(&self) -> PathBuf {
        self.scope().storage_root.clone()
    }

    /// Overrides the cache namespace.
    pub fn set_cache_namespace(&self, namespace: impl Into<String>) {
        self.scope_mut().cache_namespace = namespace.into();
    }

    /// Overrides the session namespace.
    pub fn set_session_namespace(&self, namespace: impl Into<String>) {
        self.scope_mut().session_namespace = namespace.into();
    }

    /// Overrides the queue namespace.
    pub fn set_queue_namespace(&self, namespace: impl Into<String>) {
        self.scope_mut().queue_namespace = namespace.into();
    }

    /// Overrides the storage root.
    pub fn set_storage_root(&self, root: impl Into<PathBuf>) {
        self.scope_mut().storage_root = root.into();
    }

    /// Stores `value` in the slot keyed by its type, replacing any previous value.
    pub fn insert<T: Any + Send + Sync>(&self, value: T) {
        let mut slots = self
            .slots
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slots.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Returns the slot of type `T`.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        let slots = self
            .slots
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slots
            .get(&TypeId::of::<T>())
            .and_then(|slot| Arc::clone(slot).downcast::<T>().ok())
    }

    /// Returns a clone of the slot of type `T`.
    pub fn get_cloned<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.get::<T>().map(|slot| (*slot).clone())
    }

    fn scope(&self) -> std::sync::RwLockReadGuard<'_, Scope> {
        self.scope
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn scope_mut(&self) -> std::sync::RwLockWriteGuard<'_, Scope> {
        self.scope
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Installs `context` for the duration of `future` (used by the lifecycle manager).
#[allow(dead_code)] // dipakai manager lifecycle pada Task 6
pub(crate) async fn scope_context<F: std::future::Future>(
    context: TenantContext,
    future: F,
) -> F::Output {
    CURRENT.scope(context, future).await
}

impl std::fmt::Debug for TenantContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scope = self.scope();
        f.debug_struct("TenantContext")
            .field("tenant", &self.tenant.slug)
            .field("cache_namespace", &scope.cache_namespace)
            .field("session_namespace", &scope.session_namespace)
            .field("queue_namespace", &scope.queue_namespace)
            .field("storage_root", &scope.storage_root)
            .finish()
    }
}
