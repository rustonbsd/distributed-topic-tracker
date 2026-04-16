//! Main topic handle combining bootstrap, publishing, and merging.

use std::{str::FromStr, sync::Arc};

use crate::{
    BubbleMergeConfig, GossipSender, MessageOverlapMergeConfig,
    config::PublisherConfig,
    gossip::{
        merge::{BubbleMerge, MessageOverlapMerge},
        topic::{bootstrap::Bootstrap, publisher::Publisher},
    },
};
use actor_helper::{Handle, act, act_ok};
use anyhow::Result;
use sha2::Digest;
use tokio_util::sync::CancellationToken;

/// Topic identifier derived from a string via SHA512 hashing.
///
/// Used as the stable identifier for gossip subscriptions and DHT records.
///
/// # Example
///
/// ```ignore
/// let topic_id = TopicId::new("chat-room-1".to_string());
/// ```
#[derive(Debug, Clone)]
pub struct TopicId([u8; 32]);

impl FromStr for TopicId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self::new(s.to_string()))
    }
}

impl TopicId {
    /// Create a new topic ID from a string.
    ///
    /// String is hashed with SHA512; the first 32 bytes produce the identifier.
    pub fn new(raw: String) -> Self {
        let mut raw_hash = sha2::Sha512::new();
        raw_hash.update(raw.as_bytes());

        Self(
            raw_hash.finalize()[..32]
                .try_into()
                .expect("hashing 'raw' failed"),
        )
    }

    /// Create from a pre-computed 32-byte hash.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    /// Get the hash bytes.
    pub fn hash(&self) -> [u8; 32] {
        self.0
    }
}

/// Handle to a joined gossip topic with auto-discovery.
///
/// Manages bootstrap, publishing, bubble detection, and split-brain recovery.
/// Can be split into sender and receiver for message exchange.
#[derive(Debug, Clone)]
pub struct Topic {
    api: Handle<TopicActor, anyhow::Error>,
    cancel_token: CancellationToken,
}

#[derive(Debug)]
struct TopicActor {
    bootstrap: Bootstrap,
    publisher: Option<Publisher>,
    bubble_merge: Option<BubbleMerge>,
    message_overlap_merge: Option<MessageOverlapMerge>,
    record_publisher: crate::crypto::RecordPublisher,
    cancel_token: CancellationToken,
}

impl Drop for TopicActor {
    fn drop(&mut self) {
        self.cancel_token.cancel();
    }
}

impl Topic {
    /// Create and initialize a new topic with auto-discovery.
    ///
    /// # Arguments
    ///
    /// * `record_publisher` - Record publisher for DHT operations
    /// * `gossip` - Gossip instance for topic subscription
    /// * `async_bootstrap` - If false, awaits until bootstrap completes
    pub async fn new(
        record_publisher: crate::crypto::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
        async_bootstrap: bool,
    ) -> Result<Self> {
        tracing::debug!(
            "Topic: creating new topic (async_bootstrap={})",
            async_bootstrap
        );

        let cancel_token = CancellationToken::new();
        let bootstrap = Bootstrap::new(
            record_publisher.clone(),
            gossip.clone(),
            cancel_token.clone(),
            record_publisher.config().timeouts().clone(),
            record_publisher.config().bootstrap_config().clone(),
        )
        .await?;
        tracing::debug!("Topic: bootstrap instance created");

        let api = Handle::spawn(TopicActor {
            bootstrap: bootstrap.clone(),
            record_publisher: record_publisher.clone(),
            publisher: None,
            bubble_merge: None,
            message_overlap_merge: None,
            cancel_token: cancel_token.clone(),
        })
        .0;

        let bootstrap_done = bootstrap.bootstrap().await?;
        if !async_bootstrap {
            tracing::debug!("Topic: waiting for bootstrap to complete");
            bootstrap_done.await??;
            tracing::debug!("Topic: bootstrap completed");
        } else {
            tracing::debug!("Topic: bootstrap started asynchronously");
        }

        if matches!(
            record_publisher.config().publisher_config(),
            PublisherConfig::Enabled { .. }
        ) {
            tracing::debug!("Topic: starting publisher");
            api.call(act!(actor => actor.start_publishing())).await?;
        }

        if matches!(
            record_publisher.config().merge_config().bubble_merge(),
            BubbleMergeConfig::Enabled { .. }
        ) {
            tracing::debug!("Topic: starting bubble merge");
            api.call(act!(actor => actor.start_bubble_merge())).await?;
        }

        if matches!(
            record_publisher
                .config()
                .merge_config()
                .message_overlap_merge(),
            MessageOverlapMergeConfig::Enabled { .. }
        ) {
            tracing::debug!("Topic: starting message overlap merge");
            api.call(act!(actor => actor.start_message_overlap_merge())).await?;
        }

        tracing::debug!("Topic: fully initialized");
        Ok(Self { api, cancel_token })
    }

    /// Split into sender and receiver for message exchange.
    pub async fn split(&self) -> Result<(GossipSender, crate::gossip::receiver::GossipReceiver)> {
        let topic_ref = Arc::new(self.clone());
        let mut sender = self.gossip_sender().await?;
        let mut receiver = self.gossip_receiver().await?;
        sender._topic_keep_alive = Some(topic_ref.clone());
        receiver._topic_keep_alive = Some(topic_ref);
        Ok((sender, receiver))
    }

    /// Get the gossip sender for this topic.
    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api
            .call(act!(actor => actor.bootstrap.gossip_sender()))
            .await
    }

    /// Get the gossip receiver for this topic.
    pub async fn gossip_receiver(&self) -> Result<crate::gossip::receiver::GossipReceiver> {
        self.api
            .call(act!(actor => actor.bootstrap.gossip_receiver()))
            .await
    }

    /// Get the record publisher for this topic.
    pub async fn record_creator(&self) -> Result<crate::crypto::RecordPublisher> {
        self.api
            .call(act_ok!(actor => async move { actor.record_publisher.clone() }))
            .await
    }

    #[allow(dead_code)]
    pub(crate) fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }
}

impl TopicActor {
    pub async fn start_publishing(&mut self) -> Result<()> {
        if let PublisherConfig::Enabled {
            initial_delay,
            base_interval,
            max_jitter,
        } = self.record_publisher.config().publisher_config()
        {
            tracing::debug!("TopicActor: initializing publisher");
            let publisher = Publisher::new(
                self.record_publisher.clone(),
                self.bootstrap.gossip_receiver().await?,
                self.cancel_token.clone(),
                *initial_delay,
                *base_interval,
                *max_jitter,
            )?;
            self.publisher = Some(publisher);
            tracing::debug!("TopicActor: publisher started");
        }
        Ok(())
    }

    pub async fn start_bubble_merge(&mut self) -> Result<()> {
        if let BubbleMergeConfig::Enabled {
            base_interval,
            max_jitter,
            min_neighbors,
        } = self.record_publisher.config().merge_config().bubble_merge()
        {
            tracing::debug!("TopicActor: initializing bubble merge");
            let bubble_merge = BubbleMerge::new(
                self.record_publisher.clone(),
                self.bootstrap.gossip_sender().await?,
                self.bootstrap.gossip_receiver().await?,
                self.cancel_token.clone(),
                self.record_publisher.config().max_join_peer_count().max(1),
                *base_interval,
                *max_jitter,
                *min_neighbors,
            )?;
            self.bubble_merge = Some(bubble_merge);
            tracing::debug!("TopicActor: bubble merge started");
        }
        Ok(())
    }

    pub async fn start_message_overlap_merge(&mut self) -> Result<()> {
        if let MessageOverlapMergeConfig::Enabled {
            base_interval,
            max_jitter,
        } = self
            .record_publisher
            .config()
            .merge_config()
            .message_overlap_merge()
        {
            tracing::debug!("TopicActor: initializing message overlap merge");
            let message_overlap_merge = MessageOverlapMerge::new(
                self.record_publisher.clone(),
                self.bootstrap.gossip_sender().await?,
                self.bootstrap.gossip_receiver().await?,
                self.cancel_token.clone(),
                self.record_publisher.config().max_join_peer_count().max(1),
                *base_interval,
                *max_jitter,
            )?;
            self.message_overlap_merge = Some(message_overlap_merge);
            tracing::debug!("TopicActor: message overlap merge started");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_topic_full_shutdown_on_drop() {
        let secret_key = iroh::SecretKey::generate(&mut rand::rng());
        let signing_key = mainline::SigningKey::from_bytes(&secret_key.to_bytes());
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret_key.clone())
            .bind()
            .await
            .unwrap();
        let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());

        let topic_id = crate::TopicId::new("my-iroh-gossip-topic".to_string());
        let initial_secret = b"my-initial-secret".to_vec();

        let record_publisher = crate::RecordPublisher::new(
            topic_id.clone(),
            signing_key.clone(),
            None,
            initial_secret,
            crate::config::Config::default(),
        );

        let topic = crate::Topic::new(record_publisher, gossip.clone(), true)
            .await
            .unwrap();

        let cancel_token = topic.cancel_token();

        let (sender, receiver) = topic.split().await.unwrap();

        assert!(!cancel_token.is_cancelled());

        drop(sender);
        drop(receiver);
        drop(topic);

        tokio::time::timeout(std::time::Duration::from_secs(5), cancel_token.cancelled())
            .await
            .unwrap();

        assert!(cancel_token.is_cancelled());
    }
}
