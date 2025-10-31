use std::sync::{Arc, atomic::AtomicUsize};

use anyhow::Result;
use iroh::{Endpoint, SecretKey};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::AutoDiscoveryGossip;
use tokio::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    
    use tracing_subscriber::filter::EnvFilter;

    tracing_subscriber::fmt()
        .with_thread_ids(true)
        .with_ansi(true)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("distributed_topic_tracker=debug")),
        )
        .init();
    

    // from input first param
    let expected_neighbours = std::env::args().nth(1).unwrap_or("1".to_string()).parse::<usize>()?;

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

    let cold_start_timer = Instant::now();
    let topic_id = "my-iroh-gossip-topic-experimental-1".as_bytes().to_vec();
    let (gossip_sender, gossip_receiver) = gossip
        .subscribe_and_join_with_auto_discovery(topic_id, signing_key)
        .await?
        .split()
        .await;

    println!("Cold start time: {:.0}ms", cold_start_timer.elapsed().as_millis());

    let total_messages_recv = Arc::new(AtomicUsize::new(0));
    tokio::spawn({

        let total_messages_recv = total_messages_recv.clone();
        async move {
        while let Some(Ok(event)) = gossip_receiver.next().await {
            println!("event: {event:?}");
            if matches!(event, iroh_gossip::api::Event::NeighborUp(_)) {
                total_messages_recv.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }});

    gossip_sender
        .broadcast(format!("hi from {}", endpoint.id()).into())
        .await?;

    println!("[joined topic]");

    while total_messages_recv.load(std::sync::atomic::Ordering::Relaxed) < expected_neighbours {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    println!("[finished]");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // successfully joined
    // exit with code 0
    Ok(())
}
