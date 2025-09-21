use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::{api::Event, net::Gossip};
use sha2::Digest;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{
    AutoDiscoveryGossip, RecordPublisher, RotationHandle, SecretRotation, TopicId
};


struct MySecretRotation;
impl SecretRotation for MySecretRotation {
    fn derive(
        &self,
        topic_hash: [u8; 32],
        unix_minute: u64,
        initial_secret_hash: [u8; 32],
    ) -> [u8; 32] {
        let mut hash = sha2::Sha512::new();
        hash.update(topic_hash);
        hash.update(unix_minute.to_be_bytes());
        hash.update(initial_secret_hash);
        hash.update(b"as long as you return 32 bytes this is a valid secret rotation function");
        hash.finalize()[..32].try_into().expect("hashing failed")
    }
}

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

    // Split into sink (sending) and stream (receiving)

    let record_publisher = RecordPublisher::new(
        topic_id.clone(),
        endpoint.node_id().public(),
        secret_key.secret().clone(),
        Some(RotationHandle::new(MySecretRotation)),
        initial_secret,
    );
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
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
