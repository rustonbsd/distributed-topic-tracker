//! Main topic handle combining bootstrap, publishing, and merging.

use std::sync::{Arc, Weak};

use crate::{
    BubbleMergeConfig, Config, GossipSender, MessageOverlapMergeConfig,
    config::PublisherConfig,
    gossip::{
        merge::{BubbleMerge, MessageOverlapMerge},
        topic::{bootstrap::Bootstrap, publisher::Publisher},
    },
};
use actor_helper::{Handle, act, act_ok};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

/// Handle to a joined gossip topic with auto-discovery.
///
/// Manages bootstrap, publishing, bubble detection, and split-brain recovery.
/// Can be split into sender and receiver for message exchange.
#[derive(Debug, Clone)]
pub struct Topic {
    api: Arc<Handle<TopicActor, anyhow::Error>>,
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

        let api = Arc::new(
            Handle::spawn(TopicActor {
                bootstrap: bootstrap.clone(),
                record_publisher: record_publisher.clone(),
                publisher: None,
                bubble_merge: None,
                message_overlap_merge: None,
                cancel_token: cancel_token.clone(),
            })
            .0,
        );

        let bootstrap_done = bootstrap.bootstrap().await?;
        let config = record_publisher.config().clone();
        tokio::spawn({
            let api = Arc::downgrade(&api);
            let config = config.clone();
            let cancel_token = cancel_token.clone();
            async move {
                if let Err(err) = wait_for_bootstrap(bootstrap_done, cancel_token.clone()).await {
                    tracing::warn!("bootstrap failed: {}", err);
                    return;
                }

                if async_bootstrap {
                    tracing::debug!("Bootstrap completed, now spawning workers");
                    if let Err(err) = spawn_workers(api, config, cancel_token.clone()).await {
                        cancel_token.cancel();
                        tracing::warn!("failed to spawn workers: {}", err);
                    }
                }
            }
        });

        if !async_bootstrap {
            tracing::debug!("Topic: waiting for bootstrap to complete");
            bootstrap.gossip_receiver().await?.joined().await?;
            if let Err(err) =
                spawn_workers(Arc::downgrade(&api), config, cancel_token.clone()).await
            {
                tracing::warn!("failed to spawn workers: {}", err);
                cancel_token.cancel();
                return Err(anyhow::anyhow!("failed to spawn workers: {}", err));
            }
            tracing::debug!("Topic: bootstrap completed");
        } else {
            tracing::debug!("Topic: bootstrap started asynchronously");
        }

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

async fn wait_for_bootstrap(
    bootstrap_done: tokio::sync::oneshot::Receiver<Result<()>>,
    cancel_token: CancellationToken,
) -> Result<()> {
    if let Ok(Ok(_)) = bootstrap_done.await {
        Ok(())
    } else {
        tracing::error!("Topic: bootstrap failed or cancelled, shutting down topic");
        cancel_token.cancel();
        Err(anyhow::anyhow!("bootstrap failed or cancelled"))
    }
}

async fn spawn_workers(
    api: Weak<Handle<TopicActor, anyhow::Error>>,
    config: Config,
    cancel_token: CancellationToken,
) -> Result<()> {
    if !cancel_token.is_cancelled() {
        if matches!(config.publisher_config(), PublisherConfig::Enabled(_)) {
            tracing::debug!("Topic: starting publisher");
            match api.upgrade() {
                Some(api) => {
                    if let Err(err) = api.call(act!(actor => actor.start_publishing())).await {
                        return Err(anyhow::anyhow!("failed to start publisher: {err}"));
                    }
                }
                None => {
                    return Err(anyhow::anyhow!(
                        "failed to start publisher, topic actor dropped"
                    ));
                }
            }
        }

        if matches!(
            config.merge_config().bubble_merge(),
            BubbleMergeConfig::Enabled(_)
        ) {
            tracing::debug!("Topic: starting bubble merge");
            match api.upgrade() {
                Some(api) => {
                    if let Err(err) = api.call(act!(actor => actor.start_bubble_merge())).await {
                        return Err(anyhow::anyhow!("failed to start bubble merge: {err}"));
                    }
                }
                None => {
                    return Err(anyhow::anyhow!(
                        "failed to start bubble merge, topic actor dropped"
                    ));
                }
            }
        }

        if matches!(
            config.merge_config().message_overlap_merge(),
            MessageOverlapMergeConfig::Enabled(_)
        ) {
            tracing::debug!("Topic: starting message overlap merge");
            match api.upgrade() {
                Some(api) => {
                    if let Err(err) = api
                        .call(act!(actor => actor.start_message_overlap_merge()))
                        .await
                    {
                        return Err(anyhow::anyhow!(
                            "failed to start message overlap merge: {err}"
                        ));
                    }
                }
                None => {
                    return Err(anyhow::anyhow!(
                        "failed to start message overlap merge, topic actor dropped"
                    ));
                }
            }
        }
        tracing::debug!("Topic: spawn_worker finished");
        Ok(())
    } else {
        tracing::warn!("Topic: cancelled before workers could be spawned");
        Err(anyhow::anyhow!("cancelled before workers could be spawned"))
    }
}

impl TopicActor {
    pub async fn start_publishing(&mut self) -> Result<()> {
        if let PublisherConfig::Enabled(config) = self.record_publisher.config().publisher_config()
        {
            tracing::debug!("TopicActor: initializing publisher");
            let publisher = async {
                Publisher::new(
                    self.record_publisher.clone(),
                    self.bootstrap.gossip_receiver().await?,
                    self.cancel_token.clone(),
                    config.initial_delay(),
                    config.base_interval(),
                    config.max_jitter(),
                )
            };

            match publisher.await {
                Ok(publisher) => {
                    self.publisher = Some(publisher);
                    tracing::debug!("TopicActor: publisher started");
                }
                Err(err) => {
                    if config.fail_topic_creation_on_publishing_startup_failure() {
                        return Err(anyhow::anyhow!("failed to start publisher: {}", err));
                    } else {
                        tracing::warn!(
                            "TopicActor: failed to start publisher: {}, but continuing because Publisher.fail_topic_creation_on_publishing_startup_failure is false",
                            err
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn start_bubble_merge(&mut self) -> Result<()> {
        if let BubbleMergeConfig::Enabled(config) =
            self.record_publisher.config().merge_config().bubble_merge()
        {
            tracing::debug!("TopicActor: initializing bubble merge");
            let bubble_merge = async {
                BubbleMerge::new(
                    self.record_publisher.clone(),
                    self.bootstrap.gossip_sender().await?,
                    self.bootstrap.gossip_receiver().await?,
                    self.cancel_token.clone(),
                    self.record_publisher.config().max_join_peer_count(),
                    config.base_interval(),
                    config.max_jitter(),
                    config.min_neighbors(),
                )
            };

            match bubble_merge.await {
                Ok(bubble_merge) => {
                    self.bubble_merge = Some(bubble_merge);
                    tracing::debug!("TopicActor: bubble merge started");
                }
                Err(err) => {
                    if config.fail_topic_creation_on_merge_startup_failure() {
                        return Err(anyhow::anyhow!("failed to start bubble merge: {}", err));
                    } else {
                        tracing::warn!(
                            "TopicActor: failed to start bubble merge: {}, but continuing because BubbleMerge.fail_topic_creation_on_merge_startup_failure is false",
                            err
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn start_message_overlap_merge(&mut self) -> Result<()> {
        if let MessageOverlapMergeConfig::Enabled(config) = self
            .record_publisher
            .config()
            .merge_config()
            .message_overlap_merge()
        {
            tracing::debug!("TopicActor: initializing message overlap merge");
            let message_overlap_merge = async {
                MessageOverlapMerge::new(
                    self.record_publisher.clone(),
                    self.bootstrap.gossip_sender().await?,
                    self.bootstrap.gossip_receiver().await?,
                    self.cancel_token.clone(),
                    self.record_publisher.config().max_join_peer_count(),
                    config.base_interval(),
                    config.max_jitter(),
                )
            };

            match message_overlap_merge.await {
                Ok(message_overlap_merge) => {
                    self.message_overlap_merge = Some(message_overlap_merge);
                    tracing::debug!("TopicActor: message overlap merge started");
                }
                Err(err) => {
                    if config.fail_topic_creation_on_merge_startup_failure() {
                        return Err(anyhow::anyhow!(
                            "failed to start message overlap merge: {}",
                            err
                        ));
                    } else {
                        tracing::warn!(
                            "TopicActor: failed to start message overlap merge: {}, but continuing because MessageOverlapMerge.fail_topic_creation_on_merge_startup_failure is false",
                            err
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_receiver_returns_none_after_shutdown() {
        let secret_key = iroh::SecretKey::generate();
        let signing_key = mainline::SigningKey::from_bytes(&secret_key.to_bytes());
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret_key.clone())
            .bind()
            .await
            .expect("failed to bind endpoint");
        let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());

        let topic_id = crate::TopicId::new("shutdown-receiver-test".to_string());
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
            .expect("failed to create topic");

        let cancel_token = topic.cancel_token();
        let (_sender, receiver) = topic.split().await.expect("failed to split topic");

        // Clone the receiver before shutdown, this survivor must not hang
        let mut survivor = receiver.clone();

        cancel_token.cancel();

        // next() on a receiver that was alive before shutdown must return ChannelError,
        // not hang. If the broadcast channel didn't close, this would block forever
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), survivor.next())
            .await
            .expect("next() hung after shutdown - broadcast channel didn't close");
        assert!(result.is_err(), "expected Err from next() after shutdown");

        // joined() must also return Err after shutdown
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), survivor.joined())
            .await
            .expect("joined() hung after shutdown - broadcast channel didn't close");
        assert!(result.is_err(), "expected Err from joined() after shutdown");

        // A clone made after shutdown must also return Err immediately
        // (WeakSender::upgrade fails -> gets an already closed channel)
        let mut late_clone = survivor.clone();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), late_clone.next())
            .await
            .expect("next() hung on post shutdown clone, WeakSender upgrade should fail");
        assert!(
            result.is_err(),
            "expected Err from next() on post shutdown clone"
        );

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), late_clone.joined())
            .await
            .expect("joined() hung on post shutdown clone, WeakSender upgrade should fail");
        assert!(
            result.is_err(),
            "expected Err from joined() on post shutdown clone"
        );
    }

    #[tokio::test]
    async fn test_topic_full_shutdown_on_drop() {
        let secret_key = iroh::SecretKey::generate();
        let signing_key = mainline::SigningKey::from_bytes(&secret_key.to_bytes());
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret_key.clone())
            .bind()
            .await
            .expect("failed to bind endpoint");
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
            .expect("failed to create Topic");

        let cancel_token = topic.cancel_token();

        let (sender, receiver) = topic.split().await.expect("failed to split topic");

        assert!(!cancel_token.is_cancelled());

        drop(sender);
        drop(receiver);
        drop(topic);

        tokio::time::timeout(std::time::Duration::from_secs(5), cancel_token.cancelled())
            .await
            .expect("cancel token timed out");

        assert!(cancel_token.is_cancelled());
    }
}
