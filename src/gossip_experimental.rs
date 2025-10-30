use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use ed25519_dalek::{SigningKey, VerifyingKey};
use iroh::PublicKey;
use iroh_gossip::Gossip;
use sha2::Digest;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{dht_experimental::Dht, receiver::GossipReceiver, sender::GossipSender};

pub trait AutoDiscoveryGossip {
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        topic_id: Vec<u8>,
        signing_key: SigningKey,
    ) -> anyhow::Result<Topic>;

    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        topic_id: Vec<u8>,
        signing_key: SigningKey,
    ) -> anyhow::Result<Topic>;
}

impl AutoDiscoveryGossip for iroh_gossip::net::Gossip {
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        topic_id: Vec<u8>,
        signing_key: SigningKey,
    ) -> anyhow::Result<Topic> {
        let topic = self
            .subscribe_and_join_with_auto_discovery_no_wait(topic_id, signing_key)
            .await?;
        topic.wait_for_join().await;
        Ok(topic)
    }

    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        topic_id: Vec<u8>,
        signing_key: SigningKey,
    ) -> anyhow::Result<Topic> {
        let gossip_topic: iroh_gossip::api::GossipTopic = self
            .subscribe(iroh_gossip::proto::TopicId::from_bytes(topic_hash_32(&topic_id)), vec![])
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender, self.clone()),
            GossipReceiver::new(gossip_receiver, self.clone()),
        );

        let topic = Topic::new(
            topic_id,
            &signing_key,
            self.clone(),
            gossip_sender,
            gossip_receiver,
        );

        topic.start_self_publish()?;
        topic.start_infinite_bootstrap()?;

        Ok(topic)
    }
}

pub struct Topic {
    dht: Arc<Mutex<Dht>>,
    _gossip: Gossip,
    sender: GossipSender,
    receiver: GossipReceiver,
    topic_bytes: Vec<u8>,

    // never used an atomic bool before but why not ^^
    running: Arc<AtomicBool>,
    added_nodes: Arc<Mutex<HashSet<VerifyingKey>>>,
}

impl Topic {
    pub async fn split(&self) -> (GossipSender, crate::core::receiver::GossipReceiver) {
        (self.sender.clone(), self.receiver.clone())
    }

    pub async fn gossip_sender(&self) -> GossipSender {
        self.sender.clone()
    }

    pub async fn gossip_receiver(&self) -> GossipReceiver {
        self.receiver.clone()
    }

    pub async fn stop_background_loops(&self) {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);        
    }
}

impl Topic {
    pub(self) fn new(
        topic_bytes: Vec<u8>,
        signing_key: &SigningKey,
        gossip: Gossip,
        sender: GossipSender,
        receiver: GossipReceiver,
    ) -> Self {
        let dht = Dht::new(signing_key);
        Self {
            dht: Arc::new(Mutex::new(dht)),
            _gossip: gossip,
            sender,
            receiver,
            running: Arc::new(AtomicBool::new(true)),
            topic_bytes,
            added_nodes: Arc::new(Mutex::new(HashSet::from([signing_key.verifying_key()]))),
        }
    }

    pub(self) fn start_self_publish(&self) -> anyhow::Result<JoinHandle<()>> {
        let is_running = self.running.clone();
        let dht = self.dht.clone();
        let topic_bytes = self.topic_bytes.clone();
        Ok(tokio::spawn(async move {
            while is_running.load(std::sync::atomic::Ordering::Relaxed) {
                {
                    let mut lock = dht.lock().await;
                    let res = lock.announce_self(&topic_bytes).await;
                    tracing::debug!("self_announce: {:?}", res);
                }
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        }))
    }

    pub(self) fn start_infinite_bootstrap(&self) -> anyhow::Result<JoinHandle<()>> {
        let is_running = self.running.clone();
        let dht = self.dht.clone();
        let topic_bytes = self.topic_bytes.clone();
        let sender = self.sender.clone();
        let added_nodes = self.added_nodes.clone();
        Ok(tokio::spawn(async move {
            while is_running.load(std::sync::atomic::Ordering::Relaxed) {
                let peers = {
                    let mut lock = dht.lock().await;
                    lock.get_peers(&topic_bytes).await.unwrap_or_default()
                };
                if !peers.is_empty() {
                    let mut added_nodes_lock = added_nodes.lock().await;
                    let unknown_peers = peers
                        .iter()
                        .filter_map(|peer| {
                            if !added_nodes_lock.contains(peer) {
                                Some(*peer)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<VerifyingKey>>();

                    if let Err(e) = sender
                        .join_peers(
                            unknown_peers
                                .iter()
                                .filter_map(|p| PublicKey::from_bytes(p.as_bytes()).ok())
                                .collect::<Vec<PublicKey>>(),
                            Some(5),
                        )
                        .await
                    {
                        tracing::debug!("bootstrap join_peers error: {:?}", e);
                    } else {
                        tracing::debug!("bootstrap joined {} new peers", unknown_peers.len());
                        added_nodes_lock.extend(unknown_peers);
                    }
                }

                if added_nodes.lock().await.len() < 2 {
                    tracing::debug!("not enough peers yet, waiting before next bootstrap attempt");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                } else {
                    tokio::time::sleep(Duration::from_secs(20)).await;
                }
            }
        }))
    }

    pub(self) async fn wait_for_join(&self) {
        loop {
            if self.added_nodes.lock().await.len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn topic_hash_32(topic_bytes: &Vec<u8>) -> [u8; 32] {
    let mut hasher = sha2::Sha256::new();
    hasher.update(topic_bytes);
    hasher.finalize()[..32].try_into().expect("hashing failed")
}
