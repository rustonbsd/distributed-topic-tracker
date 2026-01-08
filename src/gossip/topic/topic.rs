//! Main topic handle combining bootstrap, publishing, and merging.

use crate::{
    GossipSender,
    crypto::RecordTopic,
    gossip::{
        merge::{BubbleMerge, MessageOverlapMerge},
        topic::{bootstrap::Bootstrap, publisher::Publisher},
    },
};
use actor_helper::{Action, Actor, Handle, Receiver, act, act_ok};
use anyhow::Result;
use sha2::Digest;

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
pub struct TopicId {
    _raw: String,
    hash: [u8; 32],
}

impl From<TopicId> for RecordTopic {
    fn from(val: TopicId) -> Self {
        RecordTopic::from_bytes(&val.hash)
    }
}

impl TopicId {
    /// Create a new topic ID from a string.
    ///
    /// String is hashed with SHA512; the first 32 bytes produce the identifier.
    pub fn new(raw: String) -> Self {
        let mut raw_hash = sha2::Sha512::new();
        raw_hash.update(raw.as_bytes());

        Self {
            _raw: raw,
            hash: raw_hash.finalize()[..32]
                .try_into()
                .expect("hashing 'raw' failed"),
        }
    }

    /// Get the hash bytes.
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Get the original string.
    #[allow(dead_code)]
    pub fn raw(&self) -> &str {
        &self._raw
    }
}

/// Handle to a joined gossip topic with auto-discovery.
///
/// Manages bootstrap, publishing, bubble detection, and split-brain recovery.
/// Can be split into sender and receiver for message exchange.
#[derive(Debug, Clone)]
pub struct Topic {
    api: Handle<TopicActor, anyhow::Error>,
}

#[derive(Debug)]
struct TopicActor {
    rx: Receiver<Action<Self>>,
    bootstrap: Bootstrap,
    publisher: Option<Publisher>,
    bubble_merge: Option<BubbleMerge>,
    message_overlap_merge: Option<MessageOverlapMerge>,
    record_publisher: crate::crypto::RecordPublisher,
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

        let (api, rx) = Handle::channel();

        let bootstrap = Bootstrap::new(record_publisher.clone(), gossip.clone()).await?;
        tracing::debug!("Topic: bootstrap instance created");

        tokio::spawn({
            let bootstrap = bootstrap.clone();
            async move {
                tracing::debug!("Topic: starting topic actor");
                let mut actor = TopicActor {
                    rx,
                    bootstrap: bootstrap.clone(),
                    record_publisher,
                    publisher: None,
                    bubble_merge: None,
                    message_overlap_merge: None,
                };
                let _ = actor.run().await;
            }
        });

        let bootstrap_done = bootstrap.bootstrap().await?;
        if !async_bootstrap {
            tracing::debug!("Topic: waiting for bootstrap to complete");
            bootstrap_done.await?;
            tracing::debug!("Topic: bootstrap completed");
        } else {
            tracing::debug!("Topic: bootstrap started asynchronously");
        }

        tracing::debug!("Topic: starting publisher");
        let _ = api.call(act!(actor => actor.start_publishing())).await;

        tracing::debug!("Topic: starting bubble merge");
        let _ = api.call(act!(actor => actor.start_bubble_merge())).await;

        tracing::debug!("Topic: starting message overlap merge");
        let _ = api
            .call(act!(actor => actor.start_message_overlap_merge()))
            .await;

        tracing::debug!("Topic: fully initialized");
        Ok(Self { api })
    }

    /// Split into sender and receiver for message exchange.
    pub async fn split(&self) -> Result<(GossipSender, crate::core::receiver::GossipReceiver)> {
        Ok((self.gossip_sender().await?, self.gossip_receiver().await?))
    }

    /// Get the gossip sender for this topic.
    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api
            .call(act!(actor => actor.bootstrap.gossip_sender()))
            .await
    }

    /// Get the gossip receiver for this topic.
    pub async fn gossip_receiver(&self) -> Result<crate::core::receiver::GossipReceiver> {
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
}

impl Actor<anyhow::Error> for TopicActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    let _ = action(self).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl TopicActor {
    pub async fn start_publishing(&mut self) -> Result<()> {
        tracing::debug!("TopicActor: initializing publisher");
        let publisher = Publisher::new(
            self.record_publisher.clone(),
            self.bootstrap.gossip_receiver().await?,
        )?;
        self.publisher = Some(publisher);
        tracing::debug!("TopicActor: publisher started");
        Ok(())
    }

    pub async fn start_bubble_merge(&mut self) -> Result<()> {
        tracing::debug!("TopicActor: initializing bubble merge");
        let bubble_merge = BubbleMerge::new(
            self.record_publisher.clone(),
            self.bootstrap.gossip_sender().await?,
            self.bootstrap.gossip_receiver().await?,
        )?;
        self.bubble_merge = Some(bubble_merge);
        tracing::debug!("TopicActor: bubble merge started");
        Ok(())
    }

    pub async fn start_message_overlap_merge(&mut self) -> Result<()> {
        tracing::debug!("TopicActor: initializing message overlap merge");
        let message_overlap_merge = MessageOverlapMerge::new(
            self.record_publisher.clone(),
            self.bootstrap.gossip_sender().await?,
            self.bootstrap.gossip_receiver().await?,
        )?;
        self.message_overlap_merge = Some(message_overlap_merge);
        tracing::debug!("TopicActor: message overlap merge started");
        Ok(())
    }
}
