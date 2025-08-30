# distributed-topic-tracker


[![Crates.io](https://img.shields.io/crates/v/distributed-topic-tracker.svg)](https://crates.io/crates/distributed-topic-tracker)
[![Docs.rs](https://docs.rs/distributed-topic-tracker/badge.svg)](https://docs.rs/distributed-topic-tracker)


Decentralized auto Bootstraping for [iroh-gossip](https://github.com/n0-computer/iroh-gossip) topic's via the [mainline](https://github.com/pubky/mainline) Bittorrent DHT.

### Protocol Info
- Protocol details (spec): [PROTOCOL.md](/PROTOCOL.md)
- Architecture (illustrative): [ARCHITECTURE.md](/ARCHITECTURE.md)
- Feedback issue: https://github.com/rustonbsd/distributed-topic-tracker-exp/issues/5


## Features

- Fully decentralized bootstrap for iroh-gossip
- Ed25519-based signing and hpke shared-secret-based encryption
- DHT rate limiting (caps per-minute records)
- Resilient bootstrap with retries and jitter
- Background publisher with bubble detection and peer merging

## Quick start

Add dependencies (names subject to final crate publish):

```toml
[dependencies]
anyhow = "1"
tokio = "1"
iroh = "*"
iroh-gossip = "*"

distributed-topic-tracker = "0.1.4"
```

Simple iroh-gossip integration example:

```rust
use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{
    TopicId, AutoDiscoveryGossip, RecordPublisher, Dht,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(rand::rngs::OsRng);
    let dht = Dht::new();

    // Set up endpoint with discovery enabled
    let endpoint = Endpoint::builder()
        .secret_key(secret_key.clone())
        .discovery_n0()
        .bind()
        .await?;

    // Initialize gossip with auto-discovery
    let gossip = Gossip::builder()
        .spawn(endpoint.clone());

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let topic_id = TopicId::new("my-iroh-gossip-topic".to_string());
    let initial_secret = b"my-initial-secret".to_vec();

    // 
    let record_publisher = RecordPublisher::new(
        dht,
        topic_id.clone(),
        endpoint.node_id(),
        secret_key.secret().clone(),
        None,
        initial_secret,
    );
        
    // Split into sink (sending) and stream (receiving)
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
        .await?
        .split().await?;

    println!("Joined topic");

    Ok(())
}
```

## Testing

### Unit Tests
Run unit tests for core components:
```bash
cargo test
```

### End-to-End Tests
Test peer discovery across multiple Docker containers:
```bash
# Requires Docker and Docker Compose
./test-e2e.sh
```

The e2e test verifies that multiple nodes can discover each other through the DHT and successfully join the same gossip topic.

## Roadmap

- [x] Finalize crate name and publish to crates.io
- [x] Tests and CI
- [x] Add more examples
- [ ] Docs (api)
- [ ] Optimize configuration settings

## License

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE.txt) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT.txt) or
  <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

- Try it, then drop feedback:
  https://github.com/rustonbsd/distributed-topic-tracker-exp/issues/5
- PRs, issue and success reports welcome.

Unless explicitly stated, any contribution intentionally submitted for
inclusion in this project shall be dual-licensed as above, without any
additional terms or conditions.

