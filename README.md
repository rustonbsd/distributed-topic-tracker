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
iroh = "*"
iroh-gossip = "*"

distributed-topic-tracker = "0.2"
```

Basic iroh-gossip integration:

```rust,no_run
use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distributed-topic-tracker
use distributed_topic_tracker::{TopicId, AutoDiscoveryGossip, RecordPublisher};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(&mut rand::rng());

    // Set up endpoint with discovery enabled
    let endpoint = Endpoint::builder()
        .secret_key(secret_key.clone())
        .discovery_n0()
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
        endpoint.node_id().public(),
        secret_key.secret().clone(),
        None,
        initial_secret,
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

## Roadmap

- [x] Finalize crate name and publish to crates.io
- [x] Tests and CI
- [x] Add more examples
- [x] Optimize configuration
- [x] Major refactor
- [x] Make `iroh-gossip` integration a feature (repurposed for rustpatcher)
- [ ] API docs

## License

Licensed under Apache-2.0 or MIT you choose.

- [LICENSE-APACHE](LICENSE-APACHE.txt)
- [LICENSE-MIT](LICENSE-MIT.txt)

## Contributing

- Test and provide feedback: [Issue #5](https://github.com/rustonbsd/distributed-topic-tracker-exp/issues/5)
- PRs, issues, and reports welcome.

Contributions are dual-licensed as above unless stated otherwise.
