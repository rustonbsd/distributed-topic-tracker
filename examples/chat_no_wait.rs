use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::{api::Event, net::Gossip};

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{
    AutoDiscoveryGossip, RecordPublisher, TopicId
};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(rand::rngs::OsRng);

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

    let record_publisher = RecordPublisher::new(
        topic_id.clone(),
        endpoint.node_id(),
        secret_key.secret().clone(),
        None,
        initial_secret,
    );
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery_no_wait(record_publisher)
        .await?
        .split().await?;

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
        gossip_sender.broadcast(buffer.clone().replace("\n", "").into())
            .await
            .unwrap();
        println!(" - (sent)");
        buffer.clear();
    }
}
