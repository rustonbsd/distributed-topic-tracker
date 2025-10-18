//! Bubble detection: merge isolated peer groups in the same topic.
//!
//! If local peer count < 4, extract suggested peers from DHT records and join them.

use actor_helper::{Action, Actor, Handle, Receiver};
use std::{collections::HashSet, time::Duration};

use crate::{GossipReceiver, GossipSender, RecordPublisher, gossip::GossipRecordContent};
use anyhow::Result;

/// Detects and merges small isolated peer groups (bubbles).
///
/// Triggers when local peer count < 4 and DHT records exist. Extracts `active_peers`
/// from DHT records and joins them.
#[derive(Debug, Clone)]
pub struct BubbleMerge {
    _api: Handle<BubbleMergeActor, anyhow::Error>,
}

#[derive(Debug)]
struct BubbleMergeActor {
    rx: Receiver<Action<BubbleMergeActor>>,

    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    gossip_sender: GossipSender,
    ticker: tokio::time::Interval,
}

impl BubbleMerge {
    /// Create a new bubble merge detector.
    ///
    /// Spawns a background task that periodically checks cluster size.
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
    ) -> Result<Self> {
        let (api, rx) = Handle::channel();

        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tokio::spawn(async move {
            let mut actor = BubbleMergeActor {
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

impl Actor<anyhow::Error> for BubbleMergeActor {
    async fn run(&mut self) -> Result<()> {
        tracing::debug!("BubbleMerge: starting bubble merge actor");
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    action(self).await;
                }
                _ = self.ticker.tick() => {
                    tracing::debug!("BubbleMerge: tick fired, checking for bubbles");
                    let _ = self.merge().await;
                    let next_interval = rand::random::<u64>() % 50;
                    tracing::debug!("BubbleMerge: next check in {}s", next_interval);
                    self.ticker.reset_after(Duration::from_secs(next_interval));
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }
}

impl BubbleMergeActor {
    // Cluster size as bubble indicator
    async fn merge(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let mut records = self.record_publisher.get_records(unix_minute - 1).await;
        records.extend(self.record_publisher.get_records(unix_minute).await);

        let neighbors = self.gossip_receiver.neighbors().await;
        tracing::debug!(
            "BubbleMerge: checking with {} neighbors and {} records",
            neighbors.len(),
            records.len()
        );

        if neighbors.len() < 4 && !records.is_empty() {
            tracing::debug!(
                "BubbleMerge: detected small bubble ({} neighbors < 4), attempting merge",
                neighbors.len()
            );
            let node_ids = records
                .iter()
                .flat_map(|record| {
                    let mut node_ids = if let Ok(content) = record.content::<GossipRecordContent>()
                    {
                        content
                            .active_peers
                            .iter()
                            .filter_map(|&active_peer| {
                                if active_peer == [0; 32]
                                    || neighbors.contains(&active_peer)
                                    || active_peer.eq(record.node_id().to_vec().as_slice())
                                    || active_peer.eq(self.record_publisher.pub_key().as_bytes())
                                {
                                    None
                                } else {
                                    iroh::NodeId::from_bytes(&active_peer).ok()
                                }
                            })
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    };
                    if let Ok(node_id) = iroh::NodeId::from_bytes(&record.node_id()) {
                        if node_id != self.record_publisher.pub_key().into() {
                            node_ids.push(node_id);
                        }
                    }
                    node_ids
                })
                .collect::<HashSet<_>>();

            tracing::debug!(
                "BubbleMerge: found {} potential peers to join",
                node_ids.len()
            );

            if !node_ids.is_empty() {
                self.gossip_sender
                    .join_peers(
                        node_ids.iter().cloned().collect::<Vec<_>>(),
                        Some(super::MAX_JOIN_PEERS_COUNT),
                    )
                    .await?;
                tracing::debug!("BubbleMerge: join_peers request sent");
            }
        } else {
            tracing::debug!(
                "BubbleMerge: no merge needed (neighbors={}, records={})",
                neighbors.len(),
                records.len()
            );
        }
        Ok(())
    }
}
