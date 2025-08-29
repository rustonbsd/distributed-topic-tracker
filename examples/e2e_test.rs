use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{
    AutoDiscoveryBuilder, AutoDiscoveryGossip, DefaultSecretRotation, TopicId
};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(rand::rngs::OsRng);

    // Set up endpoint with discovery enabled
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .discovery_n0()
        .bind()
        .await?;

    // Initialize gossip with auto-discovery
    let gossip = Gossip::builder()
        .spawn_with_auto_discovery::<DefaultSecretRotation>(endpoint.clone(), None)
        .await?;

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.gossip.clone())
        .spawn();

    let topic_id = TopicId::new("my-iroh-gossip-topic2".to_string());
    let initial_secret = b"my-initial-secret".to_vec();

    // Split into sink (sending) and stream (receiving)
    let (tx, rx) = gossip
        .subscribe_and_join_with_auto_discovery(topic_id, initial_secret)
        .await?.split();

    tokio::spawn(async move {
        while let Some(Ok(event)) = rx.next().await {
            println!("event: {event:?}");
        }
    });


    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    tx.broadcast(format!("hi from {}",endpoint.node_id()).into()).await?;


    // print "[joined topic]" to stdout in success case
    println!("[joined topic]");

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    println!("[finished]");

    // successfully joined
    // exit with code 0
    Ok(())
}
