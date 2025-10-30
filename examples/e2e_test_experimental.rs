use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::AutoDiscoveryGossip;

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(&mut rand::rng());
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret_key.to_bytes());

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
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(topic_id, signing_key).await?.split().await;

    tokio::spawn(async move {
        while let Some(Ok(event)) = gossip_receiver.next().await {
            println!("event: {event:?}");
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    gossip_sender
        .broadcast(format!("hi from {}", endpoint.id()).into())
        .await?;

    println!("[joined topic]");

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    println!("[finished]");

    // successfully joined
    // exit with code 0
    Ok(())
}
