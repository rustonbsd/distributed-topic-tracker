use anyhow::Result;
use distributed_topic_tracker::AutoDiscoveryGossip;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::{api::Event, net::Gossip};

use ed25519_dalek::SigningKey;

// Imports from distrubuted-topic-tracker

#[tokio::main]
async fn main() -> Result<()> {
    // tracing init - only show distributed_topic_tracker logs
    use tracing_subscriber::filter::EnvFilter;

    tracing_subscriber::fmt()
        .with_thread_ids(true)
        .with_ansi(true)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("distributed_topic_tracker=debug")),
        )
        .init();

    // Generate a new random secret key
    let secret_key = SecretKey::generate(&mut rand::rng());
    let signing_key = SigningKey::from_bytes(&secret_key.to_bytes());

    // Set up endpoint with discovery enabled
    let endpoint = Endpoint::builder()
        .secret_key(secret_key.clone())
        .bind()
        .await?;

    // Initialize gossip with auto-discovery
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let topic_id = "my-iroh-gossip-topic-experimental".as_bytes().to_vec();

    // Split into sink (sending) and stream (receiving)
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(topic_id, signing_key)
        .await?
        .split()
        .await;

    println!("Joined topic");

    // Spawn listener for incoming messages
    tokio::spawn(async move {
        while let Some(Ok(event)) = gossip_receiver.next().await {
            if let Event::Received(msg) = event {
                println!(
                    "\nMessage from {}: {}",
                    &msg.delivered_from.to_string()[0..8],
                    String::from_utf8(msg.content.to_vec()).unwrap()
                );
            } else if let Event::NeighborUp(peer) = event {
                println!("\nJoined by {}", &peer.to_string()[0..8]);
            }
        }
    });

    // Main input loop for sending messages
    let mut buffer = String::new();
    let stdin = std::io::stdin();
    loop {
        print!("\n> ");
        stdin.read_line(&mut buffer).unwrap();
        gossip_sender
            .broadcast(buffer.clone().replace("\n", "").into())
            .await
            .unwrap();
        println!(" - (sent)");
        buffer.clear();
    }
}
