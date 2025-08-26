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

distributed-topic-tracker = "0.1.1"
```

Minimal example:

```rust
use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::{api::Event, net::Gossip};

// Crate imports
use distributed_topic_tracker::{
    AutoDiscoveryBuilder, AutoDiscoveryGossip, DefaultSecretRotation, TopicId,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a fresh node key
    let secret_key = SecretKey::generate(rand::rngs::OsRng);

    // Endpoint with discovery enabled
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .discovery_n0()
        .bind()
        .await?;

    // Gossip with auto-discovery
    let gossip = Gossip::builder()
        .spawn_with_auto_discovery::<DefaultSecretRotation>(endpoint.clone(), None)
        .await?;

    // Protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.gossip.clone())
        .spawn();

    // Topic and initial shared secret (pre-agreed out of band)
    let topic_id = TopicId::new("my-iroh-gossip-topic".to_string());
    let initial_secret = b"my-initial-secret".to_vec();

    // Join + subscribe
    let (sink, mut stream) = gossip
        .subscribe_and_join_with_auto_discovery(topic_id, &initial_secret)
        .await?
        .split();

    // Listener for incoming events
    tokio::spawn(async move {
        while let Ok(event) = stream.recv().await {
            if let Event::Received(msg) = event {
                let from = &msg.delivered_from.to_string();
                let from_short = &from[0..8];
                let body = String::from_utf8(msg.content.to_vec()).unwrap();
                println!("\nMessage from {}: {}", from_short, body);
            } else if let Event::NeighborUp(peer) = event {
                let peer_short = &peer.to_string()[0..8];
                println!("\nJoined by {}", peer_short);
            }
        }
    });

    // Simple stdin loop
    let mut buffer = String::new();
    let stdin = std::io::stdin();
    loop {
        print!("\n> ");
        stdin.read_line(&mut buffer).unwrap();
        let msg = buffer.clone().replace('\n', "");
        sink.broadcast(msg.into()).await.unwrap();
        print!(" - (sent)\n");
        buffer.clear();
    }
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

