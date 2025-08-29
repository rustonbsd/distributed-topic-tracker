pub mod actor;
pub mod crypto;
pub mod gossip;

pub use gossip::sender;

use std::{collections::HashSet, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use arc_swap::ArcSwap;
use futures::StreamExt as _;
use iroh::Endpoint;
use mainline::{MutableItem, async_dht::AsyncDht};
use once_cell::sync::Lazy;
use sha2::Digest;

use tokio::time::{sleep, timeout};

use crate::{
    crypto::record::{EncryptedRecord, Record},
    gossip::{reader::GossipReceiver, sender::GossipSender},
};

pub const MAX_JOIN_PEERS_COUNT: usize = 30;
pub const MAX_BOOTSTRAP_RECORDS: usize = 10;

static DHT: Lazy<ArcSwap<mainline::async_dht::AsyncDht>> = Lazy::new(|| {
    ArcSwap::from_pointee(
        mainline::Dht::builder()
            .build()
            .expect("failed to create dht")
            .as_async(),
    )
});

fn get_dht() -> Arc<AsyncDht> {
    DHT.load_full()
}

async fn reset_dht() {
    let n_dht = mainline::Dht::builder()
        .build()
        .expect("failed to create dht");
    DHT.store(Arc::new(n_dht.as_async()));
}

pub struct Gossip {
    pub gossip: iroh_gossip::net::Gossip,
    endpoint: iroh::Endpoint,
    secret_rotation_function: crate::crypto::keys::RotationHandle,
}

#[derive(Debug)]
pub struct Topic {
    topic_id: TopicId,
    gossip_sender: GossipSender,
    gossip_receiver: GossipReceiver,
    _gossip: iroh_gossip::net::Gossip,
    initial_secret_hash: [u8; 32],
    secret_rotation_function: crate::crypto::keys::RotationHandle,
    node_id: iroh::NodeId,
}

#[derive(Debug, Clone)]
pub struct TopicId {
    _raw: String,
    hash: [u8; 32], // sha512( raw )[..32]
}

impl TopicId {
    pub fn new(raw: String) -> Self {
        let mut raw_hash = sha2::Sha512::new();
        raw_hash.update(raw.as_bytes());

        Self {
            _raw: raw,
            hash: raw_hash.finalize()[..32]
                .try_into()
                .expect("hashing 'raw' failed"),
        }
    }
}

// State: new, split, spawn_publisher
impl Topic {
    pub async fn new(
        topic_id: TopicId,
        endpoint: &iroh::Endpoint,
        node_signing_key: &ed25519_dalek::SigningKey,
        gossip: iroh_gossip::net::Gossip,
        initial_secret: &Vec<u8>,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
        async_bootstrap: bool,
    ) -> Result<Self> {
        // Create secret_hash
        let mut initial_secret_hash = sha2::Sha512::new();
        initial_secret_hash.update(initial_secret);
        let initial_secret_hash: [u8; 32] = initial_secret_hash.finalize()[..32]
            .try_into()
            .expect("hashing failed");

        // Bootstrap to get gossip tx/rx
        let (gossip_tx, gossip_rx) = if async_bootstrap {
            Self::bootstrap_no_wait(
                topic_id.clone(),
                endpoint,
                node_signing_key,
                gossip.clone(),
                initial_secret_hash,
                secret_rotation_function.clone(),
            )
            .await?
        } else {
            Self::bootstrap(
                topic_id.clone(),
                endpoint,
                node_signing_key,
                gossip.clone(),
                initial_secret_hash,
                secret_rotation_function.clone(),
            )
            .await?
        };

        // Spawn publisher
        let _join_handler = Self::spawn_publisher(
            topic_id.clone(),
            secret_rotation_function.clone(),
            initial_secret_hash,
            endpoint.node_id(),
            gossip_rx.clone(),
            gossip_tx.clone(),
            node_signing_key.clone(),
        );

        Ok(Self {
            topic_id,
            gossip_sender: gossip_tx,
            gossip_receiver: gossip_rx,
            _gossip: gossip,
            initial_secret_hash,
            secret_rotation_function: secret_rotation_function.unwrap_or_default(),
            node_id: endpoint.node_id(),
        })
    }

    pub fn split(&self) -> (GossipSender, GossipReceiver) {
        (self.gossip_sender.clone(), self.gossip_receiver.clone())
    }

    pub fn topic_id(&self) -> &TopicId {
        &self.topic_id
    }

    pub fn node_id(&self) -> &iroh::NodeId {
        &self.node_id
    }

    pub fn gossip_sender(&self) -> GossipSender {
        self.gossip_sender.clone()
    }

    pub fn gossip_receiver(&self) -> GossipReceiver {
        self.gossip_receiver.clone()
    }

    pub fn secret_rotation_function(&self) -> crate::crypto::keys::RotationHandle {
        self.secret_rotation_function.clone()
    }

    pub fn initial_secret_hash(&self) -> [u8; 32] {
        self.initial_secret_hash
    }

    pub fn set_initial_secret_hash(&mut self, initial_secret_hash: [u8; 32]) {
        self.initial_secret_hash = initial_secret_hash;
    }
}

// Procedures: Bootstrap, Publishing, Publisher
impl Topic {
    pub async fn bootstrap_no_wait(
        topic_id: TopicId,
        endpoint: &iroh::Endpoint,
        node_signing_key: &ed25519_dalek::SigningKey,
        gossip: iroh_gossip::net::Gossip,
        initial_secret_hash: [u8; 32],
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
    ) -> Result<(GossipSender, GossipReceiver)> {
        let gossip_topic: iroh_gossip::api::GossipTopic = gossip
            .subscribe(iroh_gossip::proto::TopicId::from(topic_id.hash), vec![])
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender, gossip.clone()),
            GossipReceiver::new(gossip_receiver, gossip.clone()),
        );

        tokio::spawn({
            let gossip_sender = gossip_sender.clone();
            let gossip_receiver = gossip_receiver.clone();
            let endpoint = endpoint.clone();
            let node_signing_key = node_signing_key.clone();
            async move {
                Self::bootstrap_from_gossip(
                    gossip_sender,
                    gossip_receiver,
                    topic_id,
                    &endpoint,
                    &node_signing_key,
                    initial_secret_hash,
                    secret_rotation_function,
                )
                .await
            }
        });

        Ok((gossip_sender, gossip_receiver))
    }

    pub async fn bootstrap(
        topic_id: TopicId,
        endpoint: &iroh::Endpoint,
        node_signing_key: &ed25519_dalek::SigningKey,
        gossip: iroh_gossip::net::Gossip,
        initial_secret_hash: [u8; 32],
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
    ) -> Result<(GossipSender, GossipReceiver)> {
        let gossip_topic: iroh_gossip::api::GossipTopic = gossip
            .subscribe(iroh_gossip::proto::TopicId::from(topic_id.hash), vec![])
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender, gossip.clone()),
            GossipReceiver::new(gossip_receiver, gossip.clone()),
        );
        let res = Self::bootstrap_from_gossip(
            gossip_sender,
            gossip_receiver,
            topic_id,
            endpoint,
            node_signing_key,
            initial_secret_hash,
            secret_rotation_function,
        )
        .await;
        println!("bootstrap done: {:?}", res);
        res
    }

    async fn bootstrap_from_gossip(
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
        topic_id: TopicId,
        endpoint: &iroh::Endpoint,
        node_signing_key: &ed25519_dalek::SigningKey,
        initial_secret_hash: [u8; 32],
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
    ) -> Result<(GossipSender, GossipReceiver)> {
        let mut last_published_unix_minute = 0;
        loop {
            // Check if we are connected to at least one node
            if gossip_receiver.is_joined().await {
                return Ok((gossip_sender, gossip_receiver));
            }

            // On the first try we check the prev unix minute, after that the current one
            let unix_minute = crate::unix_minute(if last_published_unix_minute == 0 {
                -1
            } else {
                0
            });

            // Unique, verified records for the unix minute
            let records = Topic::get_unix_minute_records(
                &topic_id.clone(),
                unix_minute,
                secret_rotation_function.clone(),
                initial_secret_hash,
                &endpoint.node_id(),
            )
            .await;

            // If there are no records, invoke the publish_proc (the publishing procedure)
            // continue the loop after
            if records.is_empty() {
                if unix_minute != last_published_unix_minute {
                    last_published_unix_minute = unix_minute;
                    tokio::spawn({
                        let topic_id = topic_id.clone();
                        let node_id = endpoint.node_id();
                        let node_signing_key = node_signing_key.clone();
                        let secret_rotation_function = secret_rotation_function.clone();
                        async move {
                            let _ = Self::publish_proc(
                                unix_minute,
                                &topic_id,
                                secret_rotation_function,
                                initial_secret_hash,
                                node_id,
                                &node_signing_key,
                                HashSet::new(),
                                vec![],
                            )
                            .await;
                        }
                    });
                }
                sleep(Duration::from_millis(100)).await;
                continue;
            }

            // We found records

            // Collect node ids from active_peers and record.node_id (of publisher)
            let bootstrap_nodes = records
                .iter()
                .flat_map(|record| {
                    let mut v = vec![record.node_id()];
                    for peer in record.active_peers() {
                        if peer != [0; 32] {
                            v.push(peer);
                        }
                    }
                    v
                })
                .filter_map(|node_id| iroh::NodeId::from_bytes(&node_id).ok())
                .collect::<HashSet<_>>();

            // Maybe in the meantime someone connected to us via one of our published records
            // we don't want to disrup the gossip rotations any more then we have to
            // so we check again before joining new peers
            println!(
                "checking if joined before joining peers: {}",
                gossip_receiver.neighbors().await.len()
            );
            println!("bootstrap_records: {}", bootstrap_nodes.len());
            if gossip_receiver.is_joined().await {
                return Ok((gossip_sender, gossip_receiver));
            }

            // Instead of throwing everything into join_peers() at once we go node_id by node_id
            // again to disrupt as little nodes peer neighborhoods as possible.
            for node_id in bootstrap_nodes.iter() {
                match gossip_sender.join_peers(vec![*node_id], None).await {
                    Ok(_) => {
                        sleep(Duration::from_millis(100)).await;
                        println!("joined peer: {}", node_id);
                        if gossip_receiver.is_joined().await {
                            break;
                        }
                    }
                    Err(_) => {
                        println!("failed to join peers");
                        continue;
                    }
                }
            }

            // If we are still not connected to anyone:
            // give it the default iroh-gossip connection timeout before the final is_joined() check
            if !gossip_receiver.is_joined().await {
                sleep(Duration::from_millis(500)).await;
            }

            // If we are connected: return
            if gossip_receiver.is_joined().await {
                return Ok((gossip_sender, gossip_receiver));
            } else {
                // If we are not connected: check if we should publish a record this minute
                if unix_minute != last_published_unix_minute {
                    last_published_unix_minute = unix_minute;
                    tokio::spawn({
                        let topic_id = topic_id.clone();
                        let node_id = endpoint.node_id();
                        let node_signing_key = node_signing_key.clone();
                        let secret_rotation_function = secret_rotation_function.clone();
                        async move {
                            let _ = Self::publish_proc(
                                unix_minute,
                                &topic_id,
                                secret_rotation_function,
                                initial_secret_hash,
                                node_id,
                                &node_signing_key,
                                HashSet::new(),
                                vec![],
                            )
                            .await;
                        }
                    });
                }
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
    }

    // publishing procedure: if more then MAX_BOOTSTRAP_RECORDS are written, don't write.
    // returns all valid records found from nodes already connected to the iroh-gossip network.
    #[allow(clippy::too_many_arguments)]
    async fn publish_proc(
        unix_minute: u64,
        topic_id: &TopicId,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
        initial_secret_hash: [u8; 32],
        node_id: iroh::NodeId,
        node_signing_key: &ed25519_dalek::SigningKey,
        neighbors: HashSet<iroh::NodeId>,
        last_message_hashes: Vec<[u8; 32]>,
    ) -> Result<HashSet<Record>> {
        // Get verified records that have active_peers or last_message_hashes set (active participants)
        let records = Topic::get_unix_minute_records(
            &topic_id.clone(),
            unix_minute,
            secret_rotation_function.clone(),
            initial_secret_hash,
            &node_id,
        )
        .await
        .iter()
        .filter(|&record| {
            record
                .active_peers()
                .iter()
                .filter(|&peer| peer.eq(&[0u8; 32]))
                .count()
                > 0
                || record
                    .last_message_hashes()
                    .iter()
                    .filter(|&hash| hash.eq(&[0u8; 32]))
                    .count()
                    > 0
        })
        .cloned()
        .collect::<HashSet<_>>();

        // Don't publish if there are more then MAX_BOOTSTRAP_RECORDS already written
        // that either have active_peers or last_message_hashes set (active participants)
        if records.len() >= MAX_BOOTSTRAP_RECORDS {
            return Ok(records);
        }

        // Publish own records
        let mut active_peers: [[u8; 32]; 5] = [[0; 32]; 5];
        for (i, peer) in neighbors.iter().take(5).enumerate() {
            active_peers[i] = *peer.as_bytes()
        }

        let mut last_message_hashes_array = [[0u8; 32]; 5];
        for (i, hash) in last_message_hashes.iter().take(5).enumerate() {
            last_message_hashes_array[i] = *hash;
        }

        let record = Record::sign(
            topic_id.hash,
            unix_minute,
            *node_id.as_bytes(),
            active_peers,
            last_message_hashes_array,
            node_signing_key,
        );
        Topic::publish_unix_minute_record(
            unix_minute,
            &topic_id.clone(),
            secret_rotation_function.clone(),
            initial_secret_hash,
            record,
            Some(3),
        )
        .await?;

        Ok(records)
    }

    // Runs after bootstrap to keep anouncing the topic on mainline and help identify and merge network bubbles
    fn spawn_publisher(
        topic_id: TopicId,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
        initial_secret_hash: [u8; 32],
        node_id: iroh::NodeId,
        gossip_receiver: GossipReceiver,
        gossip_sender: GossipSender,
        node_signing_key: ed25519_dalek::SigningKey,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff = 1;
            loop {
                let unix_minute = crate::unix_minute(0);

                // Run publish_proc() (publishing procedure that is aware of MAX_BOOTSTRAP_RECORDS already written)
                if let Ok(records) = Topic::publish_proc(
                    unix_minute,
                    &topic_id.clone(),
                    Some(secret_rotation_function.clone().unwrap_or_default()),
                    initial_secret_hash,
                    node_id,
                    &node_signing_key,
                    gossip_receiver.neighbors().await,
                    gossip_receiver.last_message_hashes().await,
                )
                .await
                {
                    // Cluster size as bubble indicator
                    let neighbors = gossip_receiver.neighbors().await;
                    if neighbors.len() < 4 && !records.is_empty() {
                        let node_ids = records
                            .iter()
                            .flat_map(|record| {
                                record
                                    .active_peers()
                                    .iter()
                                    .filter_map(|&active_peer| {
                                        if active_peer == [0; 32]
                                            || neighbors.contains(&active_peer)
                                            || active_peer.eq(record.node_id().to_vec().as_slice())
                                            || active_peer.eq(node_id.as_bytes())
                                        {
                                            None
                                        } else {
                                            iroh::NodeId::from_bytes(&active_peer).ok()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect::<HashSet<_>>();
                        if gossip_sender
                            .join_peers(
                                node_ids.iter().cloned().collect::<Vec<_>>(),
                                Some(MAX_JOIN_PEERS_COUNT),
                            )
                            .await
                            .is_ok()
                        {
                            //println!("group-merger -> joined peer {}", node_id);
                        }
                    }

                    // Message overlap indicator
                    if !gossip_receiver.last_message_hashes().await.is_empty() {
                        let last_message_hashes = gossip_receiver.last_message_hashes().await;
                        let peers_to_join = records
                            .iter()
                            .filter(|record| {
                                !record
                                    .last_message_hashes()
                                    .iter()
                                    .all(|last_message_hash| {
                                        *last_message_hash != [0; 32]
                                            && last_message_hashes.contains(last_message_hash)
                                    })
                            })
                            .collect::<Vec<_>>();
                        if !peers_to_join.is_empty() {
                            let node_ids = peers_to_join
                                .iter()
                                .flat_map(|&record| {
                                    let mut peers = vec![];
                                    if let Ok(node_id) = iroh::NodeId::from_bytes(&record.node_id())
                                    {
                                        peers.push(node_id);
                                    }
                                    for active_peer in record.active_peers() {
                                        if active_peer == [0; 32] {
                                            continue;
                                        }
                                        if let Ok(node_id) = iroh::NodeId::from_bytes(&active_peer)
                                        {
                                            peers.push(node_id);
                                        }
                                    }
                                    peers
                                })
                                .collect::<HashSet<_>>();

                            if gossip_sender
                                .join_peers(
                                    node_ids.iter().cloned().collect::<Vec<_>>(),
                                    Some(MAX_JOIN_PEERS_COUNT),
                                )
                                .await
                                .is_ok()
                            {
                                /*println!(
                                    "bouble detected: no-message-overlap -> joined {} peers",
                                    node_ids.len()
                                );*/
                            }
                        }
                    }
                } else {
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).max(60);
                    continue;
                }

                backoff = 1;
                sleep(Duration::from_secs(rand::random::<u64>() % 60)).await;
            }
        })
    }
}

// Basic building blocks
impl Topic {
    async fn get_unix_minute_records(
        topic_id: &TopicId,
        unix_minute: u64,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
        initial_secret_hash: [u8; 32],
        node_id: &iroh::NodeId,
    ) -> HashSet<Record> {
        let topic_sign = crate::crypto::keys::signing_keypair(topic_id, unix_minute);
        let encryption_key = crate::crypto::keys::encryption_keypair(
            topic_id,
            &secret_rotation_function.clone().unwrap_or_default(),
            initial_secret_hash,
            unix_minute,
        );
        let salt = crate::crypto::keys::salt(topic_id, unix_minute);

        // Get records, decrypt and verify
        let dht = get_dht();

        let records_iter = timeout(
            Duration::from_secs(10),
            dht.get_mutable(topic_sign.verifying_key().as_bytes(), Some(&salt), None)
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap_or_default();

        records_iter
            .iter()
            .filter_map(
                |record| match EncryptedRecord::from_bytes(record.value().to_vec()) {
                    Ok(encrypted_record) => match encrypted_record.decrypt(&encryption_key) {
                        Ok(record) => match record.verify(&topic_id.hash, unix_minute) {
                            Ok(_) => match record.node_id().eq(node_id.as_bytes()) {
                                true => None,
                                false => Some(record),
                            },
                            Err(_) => None,
                        },
                        Err(_) => None,
                    },
                    Err(_) => None,
                },
            )
            .collect::<HashSet<_>>()
    }

    async fn publish_unix_minute_record(
        unix_minute: u64,
        topic_id: &TopicId,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
        initial_secret_hash: [u8; 32],
        record: Record,
        retry_count: Option<usize>,
    ) -> Result<()> {
        let sign_key = crypto::keys::signing_keypair(&topic_id.clone(), unix_minute);
        let salt = crypto::keys::salt(topic_id, unix_minute);
        let encryption_key = crypto::keys::encryption_keypair(
            &topic_id.clone(),
            &secret_rotation_function.clone().unwrap_or_default(),
            initial_secret_hash,
            unix_minute,
        );
        let encrypted_record = record.encrypt(&encryption_key);

        for i in 0..retry_count.unwrap_or(3) {
            let dht = get_dht();

            let most_recent_result = timeout(
                Duration::from_secs(10),
                dht.get_mutable_most_recent(
                    sign_key.clone().verifying_key().as_bytes(),
                    Some(&salt),
                ),
            )
            .await
            .unwrap_or_default();

            let item = if let Some(mut_item) = most_recent_result {
                MutableItem::new(
                    sign_key.clone(),
                    &encrypted_record.to_bytes(),
                    mut_item.seq() + 1,
                    Some(&salt),
                )
            } else {
                MutableItem::new(
                    sign_key.clone(),
                    &encrypted_record.to_bytes(),
                    0,
                    Some(&salt),
                )
            };

            let put_result = match timeout(
                Duration::from_secs(10),
                dht.put_mutable(item.clone(), Some(item.seq())),
            )
            .await
            {
                Ok(result) => result.ok(),
                Err(_) => None,
            };

            if put_result.is_some() {
                break;
            } else if i == retry_count.unwrap_or(3) - 1 {
                bail!("failed to publish record")
            }

            reset_dht().await;

            sleep(Duration::from_millis(rand::random::<u64>() % 2000)).await;
        }
        Ok(())
    }
}

pub trait AutoDiscoveryBuilder {
    #[allow(async_fn_in_trait)]
    async fn spawn_with_auto_discovery(
        self,
        endpoint: Endpoint,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
    ) -> Result<Gossip>;
}

impl AutoDiscoveryBuilder for iroh_gossip::net::Builder {
    async fn spawn_with_auto_discovery(
        self,
        endpoint: Endpoint,
        secret_rotation_function: Option<crate::crypto::keys::RotationHandle>,
    ) -> Result<Gossip> {
        Ok(Gossip {
            gossip: self.spawn(endpoint.clone()),
            endpoint: endpoint.clone(),
            secret_rotation_function: secret_rotation_function.unwrap_or_default(),
        })
    }
}

pub trait AutoDiscoveryGossip {
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        topic_id: TopicId,
        initial_secret: Vec<u8>,
    ) -> Result<Topic>;

    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        topic_id: TopicId,
        initial_secret: Vec<u8>,
    ) -> Result<Topic>;
}

impl AutoDiscoveryGossip for Gossip {
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        topic_id: TopicId,
        initial_secret: Vec<u8>,
    ) -> Result<Topic> {
        Topic::new(
            topic_id,
            &self.endpoint,
            self.endpoint.secret_key().secret(),
            self.gossip.clone(),
            &initial_secret,
            Some(self.secret_rotation_function.clone()),
            false,
        )
        .await
    }

    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        topic_id: TopicId,
        initial_secret: Vec<u8>,
    ) -> Result<Topic> {
        Topic::new(
            topic_id,
            &self.endpoint,
            self.endpoint.secret_key().secret(),
            self.gossip.clone(),
            &initial_secret,
            Some(self.secret_rotation_function.clone()),
            true,
        )
        .await
    }
}

pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}

#[cfg(test)]
mod tests {
    use crate::crypto::keys::{DefaultSecretRotation, RotationHandle, SecretRotation};

    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_topic_id_creation() {
        let topic_id = TopicId::new("test-topic".to_string());
        assert_eq!(topic_id._raw, "test-topic");
        assert_eq!(topic_id.hash.len(), 32);

        // Same input should produce same hash
        let topic_id2 = TopicId::new("test-topic".to_string());
        assert_eq!(topic_id.hash, topic_id2.hash);

        // Different input should produce different hash
        let topic_id3 = TopicId::new("different-topic".to_string());
        assert_ne!(topic_id.hash, topic_id3.hash);
    }

    #[test]
    fn test_record_serialization_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let topic = [1u8; 32];
        let unix_minute = 12345u64;
        let node_id = [2u8; 32];
        let active_peers = [[3u8; 32]; 5];
        let last_message_hashes = [[4u8; 32]; 5];

        let record = Record::sign(
            topic,
            unix_minute,
            node_id,
            active_peers,
            last_message_hashes,
            &signing_key,
        );

        // Test serialization roundtrip
        let bytes = record.to_bytes();
        let deserialized = Record::from_bytes(bytes).unwrap();

        assert_eq!(record.topic(), deserialized.topic());
        assert_eq!(record.unix_minute(), deserialized.unix_minute());
        assert_eq!(record.node_id(), deserialized.node_id());
        assert_eq!(record.active_peers(), deserialized.active_peers());
        assert_eq!(
            record.last_message_hashes(),
            deserialized.last_message_hashes()
        );
        assert_eq!(record.signature(), deserialized.signature());
    }

    #[test]
    fn test_record_verification() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let topic = [1u8; 32];
        let unix_minute = 12345u64;
        let node_id = signing_key.verifying_key().to_bytes();
        let active_peers = [[3u8; 32]; 5];
        let last_message_hashes = [[4u8; 32]; 5];

        let record = Record::sign(
            topic,
            unix_minute,
            node_id,
            active_peers,
            last_message_hashes,
            &signing_key,
        );

        // Valid verification should pass
        assert!(record.verify(&topic, unix_minute).is_ok());

        // Wrong topic should fail
        let wrong_topic = [99u8; 32];
        assert!(record.verify(&wrong_topic, unix_minute).is_err());

        // Wrong unix_minute should fail
        assert!(record.verify(&topic, unix_minute + 1).is_err());
    }

    #[test]
    fn test_encrypted_record_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let encryption_key = SigningKey::generate(&mut OsRng);
        let topic = [1u8; 32];
        let unix_minute = 12345u64;
        let node_id = signing_key.verifying_key().to_bytes();
        let active_peers = [[3u8; 32]; 5];
        let last_message_hashes = [[4u8; 32]; 5];

        let record = Record::sign(
            topic,
            unix_minute,
            node_id,
            active_peers,
            last_message_hashes,
            &signing_key,
        );

        // Test encryption/decryption roundtrip
        let encrypted = record.encrypt(&encryption_key);
        let decrypted = encrypted.decrypt(&encryption_key).unwrap();

        assert_eq!(record.topic(), decrypted.topic());
        assert_eq!(record.unix_minute(), decrypted.unix_minute());
        assert_eq!(record.node_id(), decrypted.node_id());
        assert_eq!(record.active_peers(), decrypted.active_peers());
        assert_eq!(
            record.last_message_hashes(),
            decrypted.last_message_hashes()
        );
        assert_eq!(record.signature(), decrypted.signature());
    }

    #[test]
    fn test_encrypted_record_serialization() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let encryption_key = SigningKey::generate(&mut OsRng);
        let topic = [1u8; 32];
        let unix_minute = 12345u64;
        let node_id = signing_key.verifying_key().to_bytes();
        let active_peers = [[3u8; 32]; 5];
        let last_message_hashes = [[4u8; 32]; 5];

        let record = Record::sign(
            topic,
            unix_minute,
            node_id,
            active_peers,
            last_message_hashes,
            &signing_key,
        );

        let encrypted = record.encrypt(&encryption_key);

        // Test serialization roundtrip
        let bytes = encrypted.to_bytes();
        let deserialized = EncryptedRecord::from_bytes(bytes).unwrap();

        // Should be able to decrypt the deserialized version
        let decrypted = deserialized.decrypt(&encryption_key).unwrap();
        assert_eq!(record.topic(), decrypted.topic());
        assert_eq!(record.unix_minute(), decrypted.unix_minute());
    }

    #[test]
    fn test_default_secret_rotation() {
        let rotation = RotationHandle::new(DefaultSecretRotation);
        let topic_hash = [1u8; 32];
        let unix_minute = 12345u64;
        let initial_secret_hash = [2u8; 32];

        let secret1 = rotation.derive(topic_hash, unix_minute, initial_secret_hash);
        let secret2 = rotation.derive(topic_hash, unix_minute, initial_secret_hash);

        // Same inputs should produce same secret
        assert_eq!(secret1, secret2);

        // Different unix_minute should produce different secret
        let secret3 = rotation.derive(topic_hash, unix_minute + 1, initial_secret_hash);
        assert_ne!(secret1, secret3);

        // Different topic should produce different secret
        let different_topic = [99u8; 32];
        let secret4 = rotation.derive(different_topic, unix_minute, initial_secret_hash);
        assert_ne!(secret1, secret4);
    }

    #[test]
    fn test_unix_minute_function() {
        let current = unix_minute(0);
        let prev = unix_minute(-1);
        let next = unix_minute(1);

        assert_eq!(current, prev + 1);
        assert_eq!(next, current + 1);

        // Should be deterministic
        let current2 = unix_minute(0);
        assert_eq!(current, current2);
    }

    #[test]
    fn test_topic_signing_keypair_deterministic() {
        let topic_id = TopicId::new("test-topic".to_string());
        let unix_minute = 12345u64;

        let key1 = crate::crypto::keys::signing_keypair(&topic_id, unix_minute);
        let key2 = crate::crypto::keys::signing_keypair(&topic_id, unix_minute);

        // Same inputs should produce same keypair
        assert_eq!(key1.to_bytes(), key2.to_bytes());

        // Different unix_minute should produce different keypair
        let key3 = crate::crypto::keys::signing_keypair(&topic_id, unix_minute + 1);
        assert_ne!(key1.to_bytes(), key3.to_bytes());
    }

    #[test]
    fn test_topic_encryption_keypair_deterministic() {
        let topic_id = TopicId::new("test-topic".to_string());
        let unix_minute = 12345u64;
        let initial_secret_hash = [1u8; 32];
        let rotation = RotationHandle::new(DefaultSecretRotation);

        let key1 = crate::crypto::keys::encryption_keypair(
            &topic_id,
            &rotation,
            initial_secret_hash,
            unix_minute,
        );
        let key2 = crate::crypto::keys::encryption_keypair(
            &topic_id,
            &rotation,
            initial_secret_hash,
            unix_minute,
        );

        // Same inputs should produce same keypair
        assert_eq!(key1.to_bytes(), key2.to_bytes());

        // Different unix_minute should produce different keypair
        let key3 = crypto::keys::encryption_keypair(
            &topic_id,
            &rotation,
            initial_secret_hash,
            unix_minute + 1,
        );
        assert_ne!(key1.to_bytes(), key3.to_bytes());
    }

    #[test]
    fn test_topic_salt_deterministic() {
        let topic_id = TopicId::new("test-topic".to_string());
        let unix_minute = 12345u64;

        let salt1 = crypto::keys::salt(&topic_id, unix_minute);
        let salt2 = crypto::keys::salt(&topic_id, unix_minute);

        // Same inputs should produce same salt
        assert_eq!(salt1, salt2);

        // Different unix_minute should produce different salt
        let salt3 = crypto::keys::salt(&topic_id, unix_minute + 1);
        assert_ne!(salt1, salt3);
    }
}
