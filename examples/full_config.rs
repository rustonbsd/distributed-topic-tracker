use std::time::Duration;

use anyhow::Result;
use ed25519_dalek::SigningKey;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distributed-topic-tracker
use distributed_topic_tracker::{
    AutoDiscoveryGossip, BootstrapConfig, BubbleMergeConfig, Config, DefaultSecretRotation,
    DhtConfig, MergeConfig, MessageOverlapMergeConfig, PublisherConfig, RecordPublisher,
    RotationHandle, TimeoutConfig, TopicId,
};

// Default configuration, all in one spot as an overview
// same as `Config::default()`
fn config_builder() -> Config {
    Config::builder()
        .dht_config(
            DhtConfig::builder()
                .retries(3)
                .base_retry_interval(Duration::from_secs(5))
                .max_retry_jitter(Duration::from_secs(10))
                .get_timeout(Duration::from_secs(10))
                .put_timeout(Duration::from_secs(10))
                .build(),
        )
        .bootstrap_config(
            BootstrapConfig::builder()
                .max_bootstrap_records(5)
                .publish_record_on_startup(true)
                .check_older_records_first_on_startup(false)
                .discovery_poll_interval(Duration::from_millis(2000))
                .no_peers_retry_interval(Duration::from_millis(1500))
                .per_peer_join_settle_time(Duration::from_millis(100))
                .join_confirmation_wait_time(Duration::from_millis(500))
                .build(),
        )
        .max_join_peer_count(4)
        .publisher_config(
            PublisherConfig::builder()
                .initial_delay(Duration::from_secs(10))
                .base_interval(Duration::from_secs(10))
                .max_jitter(Duration::from_secs(50))
                .build(),
        )
        .merge_config(
            MergeConfig::builder()
                .bubble_merge(
                    BubbleMergeConfig::builder()
                        .min_neighbors(4)
                        .base_interval(Duration::from_secs(60))
                        .max_jitter(Duration::from_secs(120))
                        .fail_topic_creation_on_merge_startup_failure(true)
                        .build(),
                )
                .message_overlap_merge(
                    MessageOverlapMergeConfig::builder()
                        .base_interval(Duration::from_secs(60))
                        .max_jitter(Duration::from_secs(120))
                        .fail_topic_creation_on_merge_startup_failure(true)
                        .build(),
                )
                .build(),
        )
        .timeouts(
            TimeoutConfig::builder()
                .join_peer_timeout(Duration::from_secs(5))
                .broadcast_neighbors_timeout(Duration::from_secs(5))
                .broadcast_timeout(Duration::from_secs(5))
                .build(),
        )
        .build()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate();
    let signing_key = SigningKey::from_bytes(&secret_key.to_bytes());

    // Set up endpoint with discovery enabled
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key.clone())
        .bind()
        .await?;

    // Initialize gossip with auto-discovery
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let topic_id = TopicId::new("my-iroh-gossip-topic".to_string());
    let initial_secret = b"my-initial-secret".to_vec();

    let record_publisher =
        RecordPublisher::builder(topic_id.clone(), signing_key.clone(), initial_secret)
            .config(config_builder())
            .secret_rotation(RotationHandle::new(DefaultSecretRotation))
            .build();

    let topic = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
        .await?;

    println!("[joined topic]");

    // Do something with the gossip topic
    // (bonus: GossipSender and GossipReceiver are safely clonable)
    let (_gossip_sender, _gossip_receiver) = topic.split().await?;

    Ok(())
}
