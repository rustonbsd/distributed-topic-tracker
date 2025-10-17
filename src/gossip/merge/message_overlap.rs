//! Split-brain detection via message hash overlap in DHT records.

use actor_helper::{Action, Actor, Handle, Receiver};
use std::{collections::HashSet, time::Duration};

use crate::{GossipReceiver, GossipSender, RecordPublisher, gossip::GossipRecordContent};
use anyhow::Result;

/// Detects network partitions by comparing message hashes across DHT records.
///
/// Joins peers when their published hashes match local message history.
#[derive(Debug, Clone)]
pub struct MessageOverlapMerge {
    _api: Handle<MessageOverlapMergeActor, anyhow::Error>,
}

#[derive(Debug)]
struct MessageOverlapMergeActor {
    rx: Receiver<Action<MessageOverlapMergeActor>>,

    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    gossip_sender: GossipSender,
    ticker: tokio::time::Interval,
}

impl MessageOverlapMerge {
    /// Create a new split-brain detector.
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
    ) -> Result<Self> {
        let (api, rx) = Handle::channel();

        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tokio::spawn(async move {
            let mut actor = MessageOverlapMergeActor {
                rx,
                record_publisher,
                gossip_receiver,
                gossip_sender,
                ticker,
            };
            let _ = actor.run().await;
        });

        Ok(Self { _api: api })
    }
}

impl Actor<anyhow::Error> for MessageOverlapMergeActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    action(self).await;
                }
                _ = self.ticker.tick() => {
                    let _ = self.merge().await;
                    self.ticker.reset_after(Duration::from_secs(rand::random::<u64>() % 50));
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }
}

impl MessageOverlapMergeActor {
    async fn merge(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let mut records = self.record_publisher.get_records(unix_minute-1).await;
        records.extend(self.record_publisher.get_records(unix_minute).await);
        
        if !self.gossip_receiver.last_message_hashes().await.is_empty() {
            let last_message_hashes = self.gossip_receiver.last_message_hashes().await;
            let peers_to_join = records
                .iter()
                .filter(|record| {
                    if let Ok(content) = record.content::<GossipRecordContent>() {
                        content.last_message_hashes.iter().any(|last_message_hash| {
                            *last_message_hash != [0; 32]
                                && last_message_hashes.contains(last_message_hash)
                        })
                    } else {
                        false
                    }
                })
                .collect::<Vec<_>>();
            if !peers_to_join.is_empty() {
                let node_ids = peers_to_join
                    .iter()
                    .flat_map(|&record| {
                        let mut peers = vec![];
                        if let Ok(node_id) = iroh::NodeId::from_bytes(&record.node_id()) {
                            peers.push(node_id);
                        }
                        if let Ok(content) = record.content::<GossipRecordContent>() {
                            for active_peer in content.active_peers {
                                if active_peer == [0; 32] {
                                    continue;
                                }
                                if let Ok(node_id) = iroh::NodeId::from_bytes(&active_peer) {
                                    peers.push(node_id);
                                }
                            }
                        }
                        peers
                    })
                    .collect::<HashSet<_>>();

                self.gossip_sender
                    .join_peers(
                        node_ids.iter().cloned().collect::<Vec<_>>(),
                        Some(super::MAX_JOIN_PEERS_COUNT),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}
