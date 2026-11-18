//! Redis-backed cache adapter (feature `redis`).

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::MultiplexedConnection;

use crate::error::TenancyError;
use crate::ports::CacheStore;

/// [`CacheStore`] backed by Redis.
pub struct RedisCache {
    client: redis::Client,
}

impl RedisCache {
    /// Wraps a Redis client.
    pub fn new(client: redis::Client) -> Result<Self, TenancyError> {
        Ok(Self { client })
    }

    async fn connection(&self) -> Result<MultiplexedConnection, TenancyError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)
    }
}

#[async_trait]
impl CacheStore for RedisCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TenancyError> {
        let mut connection = self.connection().await?;
        let value: Option<Vec<u8>> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut connection)
            .await
            .map_err(redis_error)?;
        Ok(value)
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), TenancyError> {
        let mut connection = self.connection().await?;
        let command = match ttl {
            Some(ttl) => {
                let mut command = redis::cmd("PSETEX");
                command.arg(key).arg(ttl.as_millis() as u64).arg(value);
                command
            }
            None => {
                let mut command = redis::cmd("SET");
                command.arg(key).arg(value);
                command
            }
        };
        command
            .query_async::<()>(&mut connection)
            .await
            .map_err(redis_error)
    }

    async fn delete(&self, key: &str) -> Result<(), TenancyError> {
        let mut connection = self.connection().await?;
        redis::cmd("DEL")
            .arg(key)
            .query_async::<i64>(&mut connection)
            .await
            .map_err(redis_error)?;
        Ok(())
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<(), TenancyError> {
        let mut connection = self.connection().await?;
        let pattern = format!("{prefix}*");
        let mut cursor: u64 = 0;
        loop {
            let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            if !keys.is_empty() {
                redis::cmd("DEL")
                    .arg(&keys)
                    .query_async::<i64>(&mut connection)
                    .await
                    .map_err(redis_error)?;
            }
            cursor = next;
            if cursor == 0 {
                return Ok(());
            }
        }
    }
}

fn redis_error(error: redis::RedisError) -> TenancyError {
    TenancyError::Other(format!("redis error: {error}"))
}
