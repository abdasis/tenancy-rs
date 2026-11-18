//! Filesystem storage rooted at a tenant directory.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::error::TenancyError;
use crate::ports::Storage;

/// [`Storage`] implementation writing under a root directory.
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    /// Creates storage rooted at `root`, which must be an absolute path.
    pub fn new(root: PathBuf) -> Result<Self, TenancyError> {
        if !root.is_absolute() {
            return Err(TenancyError::Config(
                "storage root must be an absolute path".to_string(),
            ));
        }
        Ok(Self { root })
    }

    // shortcut: symlinks are not resolved, upgrade when untrusted writers can create them
    fn resolve(&self, path: &str) -> Result<PathBuf, TenancyError> {
        if path.is_empty() {
            return Err(TenancyError::Config("storage path is empty".to_string()));
        }
        let mut resolved = self.root.clone();
        for component in Path::new(path).components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(TenancyError::PathTraversal);
                }
            }
        }
        Ok(resolved)
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<(), TenancyError> {
        let resolved = self.resolve(path)?;
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await.map_err(io_error)?;
        }
        fs::write(&resolved, data).await.map_err(io_error)
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, TenancyError> {
        let resolved = self.resolve(path)?;
        fs::read(&resolved).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                TenancyError::NotFound(path.to_string())
            } else {
                io_error(error)
            }
        })
    }

    async fn exists(&self, path: &str) -> Result<bool, TenancyError> {
        let resolved = self.resolve(path)?;
        fs::try_exists(&resolved).await.map_err(io_error)
    }

    async fn delete(&self, path: &str) -> Result<(), TenancyError> {
        let resolved = self.resolve(path)?;
        match fs::remove_file(&resolved).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        let resolved = self.resolve(prefix)?;
        tokio::task::spawn_blocking(move || remove_tree(&resolved))
            .await
            .map_err(|error| TenancyError::Other(error.to_string()))?
    }
}

fn io_error(error: std::io::Error) -> TenancyError {
    TenancyError::Other(format!("storage io error: {error}"))
}

fn remove_tree(path: &Path) -> Result<(), TenancyError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };

    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            remove_tree(&entry.path())?;
        }
        std::fs::remove_dir(path).map_err(io_error)
    } else {
        std::fs::remove_file(path).map_err(io_error)
    }
}
