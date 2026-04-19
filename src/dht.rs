//! Mainline BitTorrent DHT client for mutable record operations.
//!
//! Provides async interface for DHT get/put operations with automatic
//! retry logic and connection management.

use std::time::Duration;

use actor_helper::{Handle, act};
use anyhow::{Context, Result, bail};
use ed25519_dalek::VerifyingKey;
use futures_lite::StreamExt;
use mainline::{MutableItem, SigningKey};

use crate::config::DhtConfig;

/// DHT client wrapper with actor-based concurrency.
///
/// Manages connections to the mainline DHT and handles
/// mutable record get/put operations with automatic retries.
#[derive(Debug, Clone)]
pub struct Dht {
    api: Handle<DhtActor, anyhow::Error>,
}

#[derive(Debug, Default)]
struct DhtActor {
    dht: Option<mainline::async_dht::AsyncDht>,
    config: crate::config::DhtConfig,
}

impl Dht {
    /// Create a new DHT client.
    ///
    /// Spawns a background actor for handling DHT operations.
    pub fn new(dht_config: &DhtConfig) -> Self {
        Self {
            api: Handle::spawn(DhtActor {
                dht: None,
                config: dht_config.clone(),
            })
            .0,
        }
    }

    /// Retrieve mutable records from the DHT.
    ///
    /// # Arguments
    ///
    /// * `pub_key` - Ed25519 public key for the record
    /// * `salt` - Optional salt for record lookup
    /// * `more_recent_than` - Sequence number filter (get records newer than this)
    pub async fn get(
        &self,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        more_recent_than: Option<i64>,
    ) -> Result<Vec<MutableItem>> {
        self.api
            .call(act!(actor => actor.get(pub_key, salt, more_recent_than)))
            .await
    }

    /// Publish a mutable record to the DHT.
    ///
    /// # Arguments
    ///
    /// * `signing_key` - Ed25519 secret key for signing
    /// * `salt` - Optional salt for record slot
    /// * `data` - Record value to publish
    /// * `next_unix_minute_seq` - Sequence number for the record (per unix_minute)
    pub async fn put_mutable(
        &self,
        signing_key: SigningKey,
        salt: Option<Vec<u8>>,
        data: Vec<u8>,
        next_unix_minute_seq: i64,
    ) -> Result<()> {
        self.api
            .call(act!(actor => actor.put_mutable(signing_key, salt, data, next_unix_minute_seq)))
            .await
    }
}

impl DhtActor {
    pub async fn get(
        &mut self,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        more_recent_than: Option<i64>,
    ) -> Result<Vec<MutableItem>> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        let dht = self.dht.as_mut().context("DHT not initialized")?;
        match tokio::time::timeout(
            self.config.get_timeout(),
            dht.get_mutable(pub_key.as_bytes(), salt.as_deref(), more_recent_than)
                .collect::<Vec<_>>(),
        )
        .await
        {
            Ok(items) => Ok(items),
            Err(_) => {
                tracing::warn!("DHT get operation timed out");
                bail!("DHT get operation timed out")
            }
        }
    }

    pub async fn put_mutable(
        &mut self,
        signing_key: SigningKey,
        salt: Option<Vec<u8>>,
        data: Vec<u8>,
        next_unix_minute_seq: i64,
    ) -> Result<()> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        for i in 0..1 + self.config.retries() {
            let dht = self.dht.as_mut().context("DHT not initialized")?;

            let item = MutableItem::new(
                signing_key.clone(),
                &data,
                next_unix_minute_seq,
                salt.as_deref(),
            );

            let put_result = match tokio::time::timeout(
                self.config.put_timeout(),
                dht.put_mutable(item.clone(), Some(item.seq())),
            )
            .await
            {
                Ok(result) => match result {
                    Ok(id) => Some(id),
                    Err(err) => {
                        tracing::warn!("DHT put_mutable operation failed: {err:?}");
                        None
                    }
                },
                Err(_) => None,
            };

            if put_result.is_some() {
                break;
            } else if i == self.config.retries() {
                bail!("failed to publish record")
            }

            self.reset().await?;

            let jitter = if self.config.max_retry_jitter() > Duration::ZERO {
                Duration::from_nanos(rand::random::<u64>() % self.config.max_retry_jitter().as_nanos() as u64)
            } else {
                Duration::ZERO
            };
            let retry_interval = self.config.base_retry_interval() + jitter;

            tracing::debug!(
                "DHTActor: put_mutable attempt {}/{} failed, retrying in {}ms",
                i + 1,
                1 + self.config.retries(),
                retry_interval.as_millis()
            );
            tokio::time::sleep(retry_interval).await;
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        self.dht = Some(mainline::Dht::builder().build()?.as_async());
        Ok(())
    }
}
