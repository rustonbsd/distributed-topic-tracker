# distributed-topic-tracker

[![Crates.io](https://img.shields.io/crates/v/distributed-topic-tracker.svg)](https://crates.io/crates/distributed-topic-tracker)
[![Docs.rs](https://docs.rs/distributed-topic-tracker/badge.svg)](https://docs.rs/distributed-topic-tracker)

Decentralized auto-bootstrapping for [iroh-gossip](https://github.com/n0-computer/iroh-gossip) topics using the [mainline](https://github.com/pubky/mainline) BitTorrent DHT.

## Quick Start

Add dependencies to `Cargo.toml`:

```toml
[dependencies]
anyhow = "1"
tokio = "1"
rand = "0.9"
ed25519-dalek = "3.0.0-pre.6"
iroh = "0.98"
iroh-gossip = "^0.98"

distributed-topic-tracker = "0.3"
```

Basic iroh-gossip integration:

```rust,no_run
use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;
use ed25519_dalek::SigningKey;

// Imports from distributed-topic-tracker
use distributed_topic_tracker::{TopicId, AutoDiscoveryGossip, RecordPublisher, Config};

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

    // Initialize gossip
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    // Distributed Topic Tracker
    let topic_id = TopicId::new("my-iroh-gossip-topic".to_string());
    let initial_secret = b"my-initial-secret".to_vec();
    let record_publisher = RecordPublisher::new(
        topic_id.clone(),
        signing_key.clone(),
        None,
        initial_secret,
        Config::default(),
    );

    // Use new `subscribe_and_join_with_auto_discovery` on Gossip
    let topic = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
        .await?;

    println!("[joined topic]");

    // Work with the topic (GossipSender/Receiver are clonable)
    let (_gossip_sender, _gossip_receiver) = topic.split().await?;
    
    Ok(())
}
```

## Protocol

- Details (spec): [PROTOCOL.md](PROTOCOL.md)
- Architecture: [ARCHITECTURE.md](ARCHITECTURE.md)
- Feedback: [Issue #5](https://github.com/rustonbsd/distributed-topic-tracker-exp/issues/5)

## Features

- Decentralized bootstrap for iroh-gossip topics
- Ed25519 signing and HPKE shared-secret encryption
- DHT rate limiting (per-minute record caps)
- Resilient bootstrap with retries and jitter
- Background publisher with bubble detection and peer merging

## Testing

### Unit Tests

Run core component tests:

```bash
cargo test
```

### End-to-End Tests

Verify peer discovery across Docker containers:

```bash
# Requires Docker and Docker Compose
./test-e2e.sh
```

The E2E test confirms multiple nodes discover each other via DHT and join the same gossip topic.

## Upgrading from 0.2 to 0.3

**0.3** resolves several **stability issues** present in **0.2**. Circular and dangling references between actors caused resource leaks and **tasks could outlive topic and channel handles** (after `Topic`, `GossipSender` and `GossipReceiver` were dropped). Reduced unnecessary DHT writes and reads, adjusted timeouts, **reduced time to bootstrap**. All actor lifecycles are now token-gated, references now work as expected (if all dropped, all background tasks shut down gracefully), resolved bugs in merge workers, and many more improvements. If you find any issues, please report them.

**tldr: background tasks shutdown as expected, faster bootstrap time, better all around**

### Breaking changes

**`RecordTopic` removed, use `TopicId` instead**

`RecordTopic` has been removed. `TopicId` now serves as the unified topic identifier across the entire API and supports conversion from `&str`, `String`, `Vec<u8>`, and `FromStr`.

```rust,ignore
// 0.2
use distributed_topic_tracker::RecordTopic;
let topic = RecordTopic::from_str("my-topic")?;
let publisher = RecordPublisher::new(topic, signing_key, None, secret);

// 0.3
use distributed_topic_tracker::TopicId;
let topic = TopicId::new("my-topic".to_string());
// or: let topic: TopicId = "my-topic".into();
// or: let topic: TopicId = "my-topic".parse()?;
let publisher = RecordPublisher::new(topic, signing_key, None, secret, Config::default());
```

**`RecordPublisher::new()` now requires a `Config` parameter**

A 5th `Config` parameter was added. Use `Config::default()` for most use cases, tune as needed.

```rust,ignore
// 0.2
let publisher = RecordPublisher::new(topic, pub_key, signing_key, None, secret);

// 0.3
use distributed_topic_tracker::Config;
let publisher = RecordPublisher::new(topic, signing_key, None, secret, Config::default());

// or use the new builder:
let publisher = RecordPublisher::builder(topic_id, signing_key, initial_secret)
    .config(
        Config::builder()
            .max_join_peer_count(4)
            .build(),
    )
    .build();
```

**`MAX_BOOTSTRAP_RECORDS` constant removed**

The per-minute record cap is now configurable via `BootstrapConfig::max_bootstrap_records` (default: 5, was hardcoded 100).

```rust,ignore
// 0.2
use distributed_topic_tracker::MAX_BOOTSTRAP_RECORDS; // was 100

// 0.3 - configure via Config
let config = Config::builder()
    .bootstrap_config(
        BootstrapConfig::builder()
            .max_bootstrap_records(10)
            .build()
    )
    .build();
```

**`TopicId::raw()` removed**

`TopicId` no longer stores the original string. Only the 32-byte hash is retained.

```rust,ignore
// 0.2
let topic = TopicId::new("my-topic".to_string());
let original: &str = topic.raw(); // no longer available

// 0.3 - store the raw string yourself if needed
let raw = "my-topic".to_string();
let topic = TopicId::new(raw.clone());
```

### New features

**Full configuration system**

All timing, retry, and threshold parameters are now configurable:

```rust,no_run
use std::time::Duration;
use distributed_topic_tracker::{
    Config, DhtConfig, BootstrapConfig, PublisherConfig, MergeConfig, 
    BubbleMergeConfig, MessageOverlapMergeConfig, TimeoutConfig,
};

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
    .build();
```

**Disable merge strategies or publishing**

```rust,ignore
// Run without any merge strategies (bootstrap-only)
let config = Config::builder()
    .merge_config(
        MergeConfig::builder()
            .bubble_merge(BubbleMergeConfig::Disabled)
            .message_overlap_merge(MessageOverlapMergeConfig::Disabled)
            .build(),
    )
    .build();
```

**`RecordPublisher::builder()` for ergonomic construction**

```rust,ignore
let publisher = RecordPublisher::builder("my-topic", signing_key, b"secret")
    .secret_rotation(rotation_handle)
    .config(config)
    .build();
```

## Todo's

- [ ] Network degradation testing

## License

Licensed under Apache-2.0 or MIT you choose.

- [LICENSE-APACHE](LICENSE-APACHE.txt)
- [LICENSE-MIT](LICENSE-MIT.txt)

## Contributing

- Test and provide feedback: [Issue #5](https://github.com/rustonbsd/distributed-topic-tracker-exp/issues/5)
- PRs, issues, and reports welcome.

Contributions are dual-licensed as above unless stated otherwise.
