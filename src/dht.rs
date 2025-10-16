use std::time::Duration;

use actor_helper::{Action, Actor, Handle, Receiver, act};
use anyhow::{Context, Result, bail};
use ed25519_dalek::VerifyingKey;
use futures_lite::StreamExt;
use mainline::{MutableItem, SigningKey};

const RETRY_DEFAULT: usize = 3;

#[derive(Debug, Clone)]
pub struct Dht {
    api: Handle<DhtActor, anyhow::Error>,
}

#[derive(Debug)]
struct DhtActor {
    rx: Receiver<Action<Self>>,
    dht: Option<mainline::async_dht::AsyncDht>,
}

impl Dht {
    pub fn new() -> Self {
        let (api, rx) = Handle::channel();

        tokio::spawn(async move {
            let mut actor = DhtActor { rx, dht: None };
            let _ = actor.run().await;
        });

        Self { api }
    }

    pub async fn get(
        &self,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        more_recent_then: Option<i64>,
        timeout: Duration,
    ) -> Result<Vec<MutableItem>> {
        self.api
            .call(act!(actor => actor.get(pub_key, salt, more_recent_then, timeout)))
            .await
    }

    pub async fn put_mutable(
        &self,
        signing_key: SigningKey,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        data: Vec<u8>,
        retry_count: Option<usize>,
        timeout: Duration,
    ) -> Result<()> {
        self.api.call(act!(actor => actor.put_mutable(signing_key, pub_key, salt, data, retry_count, timeout))).await
    }
}

impl Actor<anyhow::Error> for DhtActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    action(self).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Err(anyhow::anyhow!("actor stopped"))
    }
}

impl DhtActor {
    pub async fn get(
        &mut self,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        more_recent_then: Option<i64>,
        timeout: Duration,
    ) -> Result<Vec<MutableItem>> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        let dht = self.dht.as_mut().context("DHT not initialized")?;
        Ok(tokio::time::timeout(
            timeout,
            dht.get_mutable(pub_key.as_bytes(), salt.as_deref(), more_recent_then)
                .collect::<Vec<_>>(),
        )
        .await?)
    }

    pub async fn put_mutable(
        &mut self,
        signing_key: SigningKey,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        data: Vec<u8>,
        retry_count: Option<usize>,
        timeout: Duration,
    ) -> Result<()> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        for i in 0..retry_count.unwrap_or(RETRY_DEFAULT) {
            let dht = self.dht.as_mut().context("DHT not initialized")?;

            let most_recent_result = tokio::time::timeout(
                timeout,
                dht.get_mutable_most_recent(pub_key.as_bytes(), salt.as_deref()),
            )
            .await?;

            let item = if let Some(mut_item) = most_recent_result {
                MutableItem::new(
                    signing_key.clone(),
                    &data,
                    mut_item.seq() + 1,
                    salt.as_deref(),
                )
            } else {
                MutableItem::new(signing_key.clone(), &data, 0, salt.as_deref())
            };

            let put_result = match tokio::time::timeout(
                Duration::from_secs(10),
                dht.put_mutable(item.clone(), Some(item.seq())),
            )
            .await
            {
                Ok(result) => result.ok(),
                Err(_) => None,
            };

            if put_result.is_some() {
                break;
            } else if i == retry_count.unwrap_or(RETRY_DEFAULT) - 1 {
                bail!("failed to publish record")
            }

            self.reset().await?;

            tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 2000)).await;
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        self.dht = Some(mainline::Dht::builder().build()?.as_async());
        Ok(())
    }
}
