//! Tenancy handle, lifecycle and task-local context management.

use std::future::Future;
use std::sync::Arc;

use crate::bootstrapper::Bootstrapper;
use crate::config::TenancyConfig;
use crate::context::{current_context, scope_context, TenantContext};
use crate::error::TenancyError;
use crate::event::{TenancyEvent, TenancyListener};
use crate::ports::{
    CacheStore, Encryptor, JobQueue, ScopedCache, ScopedSession, ScopedStorage, SessionStore,
    Storage,
};
use crate::repository::TenantRepository;
use crate::resolver::{RequestInfo, TenantKey, TenantResolver};
use crate::tenant::{ConfigValue, Tenant};

#[cfg(feature = "postgres")]
use crate::multi_db::TenantDatabase;

/// Builder for [`Tenancy`].
#[derive(Default)]
pub struct TenancyBuilder {
    config: Option<TenancyConfig>,
    repository: Option<Arc<dyn TenantRepository>>,
    resolver: Option<Arc<dyn TenantResolver>>,
    bootstrappers: Vec<Arc<dyn Bootstrapper>>,
    listeners: Vec<Arc<dyn TenancyListener>>,
    cache: Option<Arc<dyn CacheStore>>,
    storage: Option<Arc<dyn Storage>>,
    sessions: Option<Arc<dyn SessionStore>>,
    queue: Option<Arc<dyn JobQueue>>,
    encryptor: Option<Arc<dyn Encryptor>>,
    #[cfg(feature = "postgres")]
    databases: Option<Arc<dyn TenantDatabase>>,
}

impl TenancyBuilder {
    /// Overrides the configuration.
    pub fn config(mut self, config: TenancyConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the required tenant repository.
    pub fn repository(mut self, repository: Arc<dyn TenantRepository>) -> Self {
        self.repository = Some(repository);
        self
    }

    /// Sets the tenant resolver used by `resolve`.
    pub fn resolver(mut self, resolver: Arc<dyn TenantResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Appends a bootstrapper, run in registration order.
    pub fn bootstrapper(mut self, bootstrapper: Arc<dyn Bootstrapper>) -> Self {
        self.bootstrappers.push(bootstrapper);
        self
    }

    /// Appends a listener, called in registration order.
    pub fn listener(mut self, listener: Arc<dyn TenancyListener>) -> Self {
        self.listeners.push(listener);
        self
    }

    /// Sets the cache store.
    pub fn cache(mut self, cache: Arc<dyn CacheStore>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Sets the storage backend.
    pub fn storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Sets the session store.
    pub fn sessions(mut self, sessions: Arc<dyn SessionStore>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// Sets the job queue.
    pub fn queue(mut self, queue: Arc<dyn JobQueue>) -> Self {
        self.queue = Some(queue);
        self
    }

    /// Sets the encryptor used for tenant configuration.
    pub fn encryptor(mut self, encryptor: Arc<dyn Encryptor>) -> Self {
        self.encryptor = Some(encryptor);
        self
    }

    #[cfg(feature = "postgres")]
    /// Sets the multi-database manager (feature `postgres`).
    pub fn databases(mut self, databases: Arc<dyn TenantDatabase>) -> Self {
        self.databases = Some(databases);
        self
    }

    /// Builds the handle; a repository is required, everything else is optional.
    pub fn build(self) -> Result<Tenancy, TenancyError> {
        let repository = self
            .repository
            .ok_or_else(|| TenancyError::Config("tenant repository is required".to_string()))?;
        Ok(Tenancy {
            inner: Arc::new(Inner {
                config: self.config.unwrap_or_default(),
                repository,
                resolver: self.resolver,
                bootstrappers: self.bootstrappers,
                listeners: self.listeners,
                cache: self.cache,
                storage: self.storage,
                sessions: self.sessions,
                queue: self.queue,
                encryptor: self.encryptor,
                #[cfg(feature = "postgres")]
                databases: self.databases,
            }),
        })
    }
}

struct Inner {
    config: TenancyConfig,
    repository: Arc<dyn TenantRepository>,
    resolver: Option<Arc<dyn TenantResolver>>,
    bootstrappers: Vec<Arc<dyn Bootstrapper>>,
    listeners: Vec<Arc<dyn TenancyListener>>,
    cache: Option<Arc<dyn CacheStore>>,
    storage: Option<Arc<dyn Storage>>,
    sessions: Option<Arc<dyn SessionStore>>,
    queue: Option<Arc<dyn JobQueue>>,
    encryptor: Option<Arc<dyn Encryptor>>,
    #[cfg(feature = "postgres")]
    databases: Option<Arc<dyn TenantDatabase>>,
}

/// Cloneable handle wiring configuration, ports and lifecycle.
///
/// ```
/// use std::sync::Arc;
///
/// use tenancy::repository::memory::MemoryTenantRepository;
/// use tenancy::{Tenancy, Tenant};
///
/// # async fn example() -> Result<(), tenancy::TenancyError> {
/// let tenancy = Tenancy::builder()
///     .repository(Arc::new(MemoryTenantRepository::new()))
///     .build()?;
///
/// let tenant = Tenant::new("Acme", "acme")?;
/// let slug = tenancy.run(tenant, || async { "acme" }).await?;
/// assert_eq!(slug, "acme");
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Tenancy {
    inner: Arc<Inner>,
}

impl Tenancy {
    /// Starts a builder.
    pub fn builder() -> TenancyBuilder {
        TenancyBuilder::default()
    }

    /// Configuration of this instance.
    pub fn config(&self) -> &TenancyConfig {
        &self.inner.config
    }

    /// Tenant repository backing this instance.
    pub fn repository(&self) -> Arc<dyn TenantRepository> {
        Arc::clone(&self.inner.repository)
    }

    /// Cache scoped to the current tenant; `NotResolved` outside a lifecycle.
    pub fn cache(&self) -> Result<ScopedCache, TenancyError> {
        self.inner
            .cache
            .clone()
            .map(ScopedCache::new)
            .ok_or_else(|| TenancyError::Config("cache store is not configured".to_string()))
    }

    /// Storage scoped to the current tenant; `NotResolved` outside a lifecycle.
    pub fn storage(&self) -> Result<ScopedStorage, TenancyError> {
        self.inner
            .storage
            .clone()
            .map(ScopedStorage::new)
            .ok_or_else(|| TenancyError::Config("storage is not configured".to_string()))
    }

    /// Sessions scoped to the current tenant; `NotResolved` outside a lifecycle.
    pub fn sessions(&self) -> Result<ScopedSession, TenancyError> {
        self.inner
            .sessions
            .clone()
            .map(ScopedSession::new)
            .ok_or_else(|| TenancyError::Config("session store is not configured".to_string()))
    }

    /// Queue port, when configured.
    pub fn queue(&self) -> Option<Arc<dyn JobQueue>> {
        self.inner.queue.clone()
    }

    /// Encryptor port, when configured.
    pub fn encryptor(&self) -> Option<Arc<dyn Encryptor>> {
        self.inner.encryptor.clone()
    }

    #[cfg(feature = "postgres")]
    /// Multi-database manager, when configured (feature `postgres`).
    pub fn databases(&self) -> Option<Arc<dyn TenantDatabase>> {
        self.inner.databases.clone()
    }

    /// Resolves the request into a tenant; `NotResolved` when no resolver matches.
    pub async fn resolve(&self, request: &RequestInfo) -> Result<Tenant, TenancyError> {
        let resolver =
            self.inner.resolver.as_ref().ok_or_else(|| {
                TenancyError::Config("no tenant resolver is configured".to_string())
            })?;
        let key = resolver
            .resolve(request)
            .await?
            .ok_or(TenancyError::NotResolved)?;
        self.inner
            .repository
            .find_by_key(&key)
            .await?
            .ok_or_else(|| TenancyError::NotFound(describe_key(&key)))
    }

    /// Runs the forward lifecycle; pair with [`Tenancy::end`] in manual mode.
    pub async fn initialize(&self, tenant: Tenant) -> Result<TenancyHandle, TenancyError> {
        let context = TenantContext::new(tenant);
        let slug = context.tenant().slug.clone();
        let namespace = format!("tenant:{slug}");
        context.set_cache_namespace(namespace.clone());
        context.set_session_namespace(namespace.clone());
        context.set_queue_namespace(namespace);
        context.set_storage_root(self.inner.config.storage_root.join(&slug));

        self.emit(&TenancyEvent::Bootstrapping(context.tenant_id()))
            .await?;

        for (started, bootstrapper) in self.inner.bootstrappers.iter().enumerate() {
            if let Err(error) = bootstrapper.bootstrap(&context).await {
                for previous in self.inner.bootstrappers[..started].iter().rev() {
                    if let Err(revert_error) = previous.revert(&context).await {
                        tracing::warn!(tenant = %slug, %revert_error, "bootstrapper revert failed");
                    }
                }
                return Err(error);
            }
        }

        if let Err(error) = self
            .emit(&TenancyEvent::Bootstrapped(context.tenant_id()))
            .await
        {
            tracing::warn!(tenant = %slug, %error, "listener failed on Bootstrapped");
        }

        Ok(TenancyHandle {
            tenancy: self.clone(),
            context,
            ended: false,
        })
    }

    /// Runs the reverse lifecycle for a handle from [`Tenancy::initialize`].
    pub async fn end(&self, mut handle: TenancyHandle) -> Result<(), TenancyError> {
        handle.ended = true;
        self.teardown(&handle.context).await
    }

    /// Initializes the tenant, runs `f` inside the tenant context, then tears down.
    pub async fn run<T, F, Fut>(&self, tenant: Tenant, f: F) -> Result<T, TenancyError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let handle = self.initialize(tenant).await?;
        handle.run(f).await
    }

    /// Runs `f` in a new task with the tenant context installed.
    pub fn spawn<T, F, Fut>(
        self: &Arc<Self>,
        tenant: Tenant,
        f: F,
    ) -> tokio::task::JoinHandle<Result<T, TenancyError>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let tenancy = Arc::clone(self);
        tokio::spawn(async move { tenancy.run(tenant, f).await })
    }

    /// Tenant of the current task, when one is active.
    pub fn current(&self) -> Option<Tenant> {
        current_context().map(|context| context.tenant().clone())
    }

    /// Persists a new tenant and emits [`TenancyEvent::TenantCreated`].
    pub async fn create_tenant(&self, tenant: Tenant) -> Result<Tenant, TenancyError> {
        let created = self.inner.repository.create(&tenant).await?;
        self.log_listener_error(TenancyEvent::TenantCreated(created.id))
            .await;
        Ok(created)
    }

    /// Persists an existing tenant and emits [`TenancyEvent::TenantUpdated`].
    pub async fn update_tenant(&self, tenant: Tenant) -> Result<Tenant, TenancyError> {
        let updated = self.inner.repository.update(&tenant).await?;
        self.log_listener_error(TenancyEvent::TenantUpdated(updated.id))
            .await;
        Ok(updated)
    }

    /// Persists one configuration entry; the cache decorator invalidates lookups.
    pub async fn set_config(
        &self,
        tenant: &Tenant,
        key: &str,
        value: &str,
        encrypted: bool,
    ) -> Result<(), TenancyError> {
        let value = ConfigValue {
            value: value.to_string(),
            encrypted,
        };
        self.inner
            .repository
            .save_config(tenant.id, key, &value)
            .await
    }

    async fn log_listener_error(&self, event: TenancyEvent) {
        if let Err(error) = self.emit(&event).await {
            tracing::warn!(%error, "listener failed on tenant event");
        }
    }

    pub(crate) fn raw_cache(&self) -> Option<Arc<dyn CacheStore>> {
        self.inner.cache.clone()
    }

    pub(crate) fn raw_storage(&self) -> Option<Arc<dyn Storage>> {
        self.inner.storage.clone()
    }

    pub(crate) fn raw_sessions(&self) -> Option<Arc<dyn SessionStore>> {
        self.inner.sessions.clone()
    }

    pub(crate) async fn emit(&self, event: &TenancyEvent) -> Result<(), TenancyError> {
        for listener in &self.inner.listeners {
            listener.handle(event).await?;
        }
        Ok(())
    }

    async fn teardown(&self, context: &TenantContext) -> Result<(), TenancyError> {
        let mut first_error = self
            .emit(&TenancyEvent::Ending(context.tenant_id()))
            .await
            .err();

        for bootstrapper in self.inner.bootstrappers.iter().rev() {
            if let Err(error) = bootstrapper.revert(context).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        if let Err(error) = self.emit(&TenancyEvent::Ended(context.tenant_id())).await {
            tracing::warn!(tenant = %context.tenant().slug, %error, "listener failed on Ended");
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Guard returned by [`Tenancy::initialize`].
///
/// Dropping it without [`Tenancy::end`] skips the async revert; the task-local
/// context is still restored by Tokio.
pub struct TenancyHandle {
    tenancy: Tenancy,
    context: TenantContext,
    ended: bool,
}

impl TenancyHandle {
    /// Context installed by this handle.
    pub fn context(&self) -> &TenantContext {
        &self.context
    }

    /// Installs the context, runs `f`, tears down and returns its value.
    pub async fn run<T, F, Fut>(mut self, f: F) -> Result<T, TenancyError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let value = scope_context(self.context.clone(), f()).await;
        self.ended = true;
        self.tenancy.teardown(&self.context).await?;
        Ok(value)
    }
}

impl Drop for TenancyHandle {
    fn drop(&mut self) {
        if !self.ended {
            tracing::warn!(
                tenant = %self.context.tenant().slug,
                "tenancy handle dropped without end; bootstrappers were not reverted"
            );
        }
    }
}

fn describe_key(key: &TenantKey) -> String {
    match key {
        TenantKey::Id(id) => format!("id {id}"),
        TenantKey::Slug(slug) => format!("slug {slug}"),
        TenantKey::Domain(host) => format!("domain {host}"),
    }
}
