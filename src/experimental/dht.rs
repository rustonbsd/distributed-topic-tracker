use anyhow::{Context, Result};
use ed25519_dalek::VerifyingKey;

use futures_lite::StreamExt;
use mainline_exp::{Dht as MainlineDht, Id, SigningKey, async_dht::AsyncDht};
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct Dht {
    dht: Option<AsyncDht>,
    signing_key: SigningKey,
}

impl Dht {
    pub fn new(signing_key: &SigningKey) -> Self {
        Self {
            dht: None,
            signing_key: signing_key.clone(),
        }
    }

    pub async fn reset(&mut self) -> Result<()> {
        if self.dht.is_some() {
            return Ok(());
        }
        let dht = MainlineDht::builder()
                .extra_bootstrap(&["pkarr.rustonbsd.com:6881"])
                .build()?
                .as_async();
        if  !dht.bootstrapped().await {
            anyhow::bail!("DHT bootstrap failed");
        }
        self.dht = Some(dht);
            
        Ok(())
    }

    pub async fn get_peers(&mut self, topic_bytes: &Vec<u8>) -> Result<Vec<VerifyingKey>> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        let dht = self.dht.as_mut().context("DHT not initialized")?;
        let id = Id::from_bytes(topic_hash_20(topic_bytes))?;

        let topic_stream = dht.get_signed_peers(id).await.collect::<Vec<_>>();
        Ok(topic_stream
            .await
            .iter()
            .flatten()
            .filter_map(|item| VerifyingKey::from_bytes(item.key()).ok())
            .collect::<Vec<_>>())
    }

    pub async fn announce_self(&mut self, topic_bytes: &Vec<u8>) -> Result<()> {
        if self.dht.is_none() {
            self.reset().await?;
        }

        let dht = self.dht.as_mut().context("DHT not initialized")?;
        let id = Id::from_bytes(topic_hash_20(topic_bytes))?;

        tokio::time::timeout(std::time::Duration::from_secs(5), dht.announce_signed_peer(id, &self.signing_key))
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to announce signed peer: {e}"))
    }
}

fn topic_hash_20(topic_bytes: &Vec<u8>) -> [u8; 20] {
    let mut hasher = sha2::Sha512::new();
    hasher.update("/iroh/distributed-topic-tracker");
    hasher.update(topic_bytes);
    hasher.finalize()[..20].try_into().expect("hashing failed")
}
