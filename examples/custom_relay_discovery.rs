use anyhow::Result;
use ed25519_dalek::SigningKey;
use iroh::{Endpoint, RelayMap, SecretKey, address_lookup::{dns::DnsAddressLookup, pkarr::PkarrPublisher}};
use iroh_gossip::net::Gossip;

// Imports from distrubuted-topic-tracker
use distributed_topic_tracker::{AutoDiscoveryGossip, RecordPublisher, TopicId};

#[tokio::main]
async fn main() -> Result<()> {
    // Generate a new random secret key
    let secret_key = SecretKey::generate(&mut rand::rng());
    let signing_key = SigningKey::from_bytes(&secret_key.to_bytes());

    // Set up endpoint with custom discovery enabled
    let relay_map = iroh::RelayMap::empty();//iroh::defaults::prod::default_relay_map();
    relay_map.extend(&RelayMap::from(
        "https://iroh-relay.rustonbsd.com:8443/".parse::<iroh::RelayUrl>()?,
    ));
    let dns_lookup = DnsAddressLookup::builder("https://iroh-dns.rustonbsd.com/".parse()?).build();
    let pkarr_publisher = PkarrPublisher::builder("https://iroh-relay.rustonbsd.com".parse()?).build(secret_key.clone());

    let endpoint = Endpoint::builder()
        .relay_mode(iroh::RelayMode::Custom(relay_map))
        //.address_lookup(DnsAddressLookup::n0_dns().build())
        //.address_lookup(PkarrPublisher::n0_dns().build(secret_key.clone()))
        .address_lookup(dns_lookup)
        .address_lookup(pkarr_publisher)
        .secret_key(secret_key.clone())
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

    // Split into sink (sending) and stream (receiving)

    let record_publisher = RecordPublisher::new(
        topic_id.clone(),
        signing_key.verifying_key(),
        signing_key.clone(),
        None,
        initial_secret,
    );

    let topic = gossip
        .subscribe_and_join_with_auto_discovery(record_publisher)
        .await?;

    println!("[joined topic]");

    // Do something with the gossip topic
    // (bonus: GossipSender and GossipReceiver are safely clonable)
    let (_gossip_sender, _gossip_receiver) = topic.split().await?;

    Ok(())
}
