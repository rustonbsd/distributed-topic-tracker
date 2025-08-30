use std::{collections::HashSet, time::Duration};

use crate::{actor::Actor, GossipSender};
use anyhow::Result;
use sha2::Digest;


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

    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    #[allow(dead_code)]
    pub(crate) fn raw(&self) -> &str {
        &self._raw
    }
}

pub struct Topic {
    api: crate::actor::Handle<TopicActor>,
}

struct TopicActor {
    rx: tokio::sync::mpsc::Receiver<crate::actor::Action<Self>>,
    bootstrap: crate::topic::bootstrap::Bootstrap,
    record_publisher: crate::crypto::record::RecordPublisher,
}

impl Topic {
    pub async fn new(
        record_publisher: crate::crypto::record::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
        async_bootstrap: bool,
    ) -> Result<Self> {
        let (api, rx) = crate::actor::Handle::channel(32);

        let bootstrap =
            crate::topic::bootstrap::Bootstrap::new(record_publisher.clone(), gossip.clone()).await?;

        tokio::spawn({
            let bootstrap = bootstrap.clone();
            async move {
                let mut actor = TopicActor {
                    rx,
                    bootstrap: bootstrap.clone(),
                    record_publisher,
                };
                let _ = actor.run().await;
            }
        });

        if async_bootstrap {
            tokio::spawn({
                let bootstrap = bootstrap.clone();
                async move {
                    let _ = bootstrap.bootstrap().await;
                }
            });
        } else {
            bootstrap.bootstrap().await?;
        }

        // Spawn publisher after bootstrap
        api.call(move |actor| Box::pin(actor.spawn_publisher())).await?;

        Ok(Self { api })
    }

    pub async fn split(
        &self,
    ) -> Result<(
        GossipSender,
        crate::gossip::receiver::GossipReceiver,
    )> {
        Ok((self.gossip_sender().await?, self.gossip_receiver().await?))
    }

    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api.call(move |actor| Box::pin(actor.gossip_sender())).await
    }

    pub async fn gossip_receiver(&self) -> Result<crate::gossip::receiver::GossipReceiver> {
        self.api.call(move |actor| Box::pin(actor.gossip_receiver())).await
    }

    pub async fn record_creator(&self) -> Result<crate::crypto::record::RecordPublisher> {
        self.api
            .call(move |actor| Box::pin(async move { Ok(actor.record_publisher.clone()) }))
            .await
    }
}

impl Actor for TopicActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
                    let _ = action(self).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl TopicActor {
    pub async fn gossip_receiver(&mut self) -> Result<crate::gossip::receiver::GossipReceiver> {
        self.bootstrap.gossip_receiver().await
    }

    pub async fn gossip_sender(&mut self) -> Result<GossipSender> {
        self.bootstrap.gossip_sender().await
    }
        // Runs after bootstrap to keep anouncing the topic on mainline and help identify and merge network bubbles
    pub async fn spawn_publisher(&mut self) -> Result<tokio::task::JoinHandle<()>> {
        Ok(tokio::spawn({
            let mut backoff = 1;
            let gossip_sender = match self.gossip_sender().await {
                Ok(gossip_sender) => gossip_sender,
                Err(err) => return Err(anyhow::anyhow!(err)),
            };
            let gossip_receiver = match self.gossip_receiver().await {
                Ok(gossip_receiver) => gossip_receiver,
                Err(err) => return Err(anyhow::anyhow!(err)),
            };
            let record_publisher = self.record_publisher.clone();
            async move {
            
            loop {
                let unix_minute = crate::unix_minute(0);

                // Run publish_proc() (publishing procedure that is aware of MAX_BOOTSTRAP_RECORDS already written)
                let record = record_publisher.new_record(unix_minute, gossip_receiver.neighbors().await.iter().cloned().collect(), gossip_receiver.last_message_hashes().await);
                if let Ok(records) = record_publisher.publish_record(record).await {
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
                                            || active_peer.eq(record_publisher.node_id().as_bytes())
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
                                Some(crate::MAX_JOIN_PEERS_COUNT),
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
                                    Some(crate::MAX_JOIN_PEERS_COUNT),
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
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).max(60);
                    continue;
                }

                backoff = 1;
                tokio::time::sleep(Duration::from_secs(rand::random::<u64>() % 60)).await;
            }
        }}))
    }
}
