use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{AutoDiscoveryGossip, RecordPublisher, TopicId};

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
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Set up protocol router
    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let topic_id = TopicId::new("my-iroh-gossip-topic".to_string());
    let initial_secret = b"my-initial-secret".to_vec();

    let record_publisher = RecordPublisher::new(
        topic_id.clone(),
        endpoint.node_id().public(),
        secret_key.secret().clone(),
        None,
        initial_secret,
    );
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
        .await?
        .split()
        .await?;

    tokio::spawn(async move {
        while let Some(Ok(event)) = gossip_receiver.next().await {
            println!("event: {event:?}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    gossip_sender
        .broadcast(format!("hi from {}", endpoint.node_id()).into())
        .await?;

    println!("[joined topic]");

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    println!("[finished]");

    // successfully joined
    // exit with code 0
    Ok(())
}
