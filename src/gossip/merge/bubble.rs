//! Bubble detection: merge isolated peer groups in the same topic.
//!
//! If local peer count < `min_neighbors`, extract suggested peers from DHT records and join them.

use actor_helper::{Action, Handle, Receiver};
use iroh::EndpointId;
use std::{collections::HashSet, time::Duration};

use crate::{GossipReceiver, GossipSender, RecordPublisher, gossip::GossipRecordContent};
use anyhow::Result;

/// Detects and merges small isolated peer groups (bubbles).
///
/// Triggers when local peer count < `min_neighbors` and DHT records exist. Extracts `active_peers`
/// from DHT records and joins them.
#[derive(Debug, Clone)]
pub struct BubbleMerge {
    _api: Handle<BubbleMergeActor, anyhow::Error>,
}

#[derive(Debug)]
struct BubbleMergeActor {
    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    gossip_sender: GossipSender,
    ticker: tokio::time::Interval,
    cancel_token: tokio_util::sync::CancellationToken,
    max_join_peers: usize,
    base_interval: Duration,
    max_jitter: Duration,
    min_neighbors: usize,
}

impl BubbleMerge {
    /// Create a new bubble merge detector.
    ///
    /// Spawns a background task that periodically checks cluster size.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
        max_join_peers: usize,
        base_interval: Duration,
        max_jitter: Duration,
        min_neighbors: usize,
    ) -> Result<Self> {
        let base_interval = base_interval.max(Duration::from_secs(1));
        let mut ticker = tokio::time::interval(base_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let api = Handle::spawn_with(
            BubbleMergeActor {
                record_publisher,
                gossip_receiver,
                gossip_sender,
                ticker,
                cancel_token,
                max_join_peers,
                base_interval,
                max_jitter,
                min_neighbors,
            },
            |mut actor, rx| async move { actor.run(rx).await },
        )
        .0;

        Ok(Self { _api: api })
    }
}

impl BubbleMergeActor {
    async fn run(&mut self, rx: Receiver<Action<BubbleMergeActor>>) -> Result<()> {
        tracing::debug!("BubbleMerge: starting bubble merge actor");
        loop {
            tokio::select! {
                result = rx.recv_async() => {
                    match result {
                        Ok(action) => action(self).await,
                        Err(_) => break Ok(()),
                    }
                }
                _ = self.ticker.tick() => {
                    tracing::debug!("BubbleMerge: tick fired, checking for bubbles");
                    if let Err(e) = self.merge().await {
                        tracing::warn!("BubbleMerge: error during merge: {:?}", e);
                    }
                    let jitter = if self.max_jitter > Duration::ZERO {
                        Duration::from_nanos(rand::random::<u64>() % self.max_jitter.as_nanos() as u64)
                    } else {
                        Duration::ZERO
                    };
                    let next_interval = self.base_interval + jitter;
                    tracing::debug!("BubbleMerge: next check in {}ms", next_interval.as_millis());
                    self.ticker.reset_after(next_interval);
                }
                _ = self.cancel_token.cancelled() => {
                    break Ok(());
                }
                else => break Ok(()),
            }
        }
    }
}

impl BubbleMergeActor {
    // Cluster size as bubble indicator
    async fn merge(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let mut records = self.record_publisher.get_records(unix_minute - 1).await?;
        records.extend(self.record_publisher.get_records(unix_minute).await?);

        let neighbors = self.gossip_receiver.neighbors().await?;
        tracing::debug!(
            "BubbleMerge: checking with {} neighbors and {} records",
            neighbors.len(),
            records.len()
        );

        if neighbors.len() < self.min_neighbors && !records.is_empty() {
            tracing::debug!(
                "BubbleMerge: detected small bubble ({} neighbors < {}), attempting merge",
                neighbors.len(),
                self.min_neighbors
            );
            let self_pub_key = EndpointId::from_verifying_key(self.record_publisher.pub_key());
            let pub_keys = records
                .iter()
                .flat_map(|record| {
                    let mut pub_keys = if let Ok(content) = record.content::<GossipRecordContent>()
                    {
                        content
                            .active_peers
                            .iter()
                            .filter_map(|&active_peer| {
                                if active_peer == [0; 32]
                                    || neighbors.contains(&active_peer)
                                    || active_peer == record.pub_key()
                                    || active_peer.eq(self_pub_key.as_bytes())
                                {
                                    None
                                } else {
                                    iroh::EndpointId::from_bytes(&active_peer).ok()
                                }
                            })
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    };
                    if let Ok(pub_key) = EndpointId::from_bytes(&record.pub_key())
                        && !pub_key.eq(&self_pub_key)
                        && !neighbors.contains(&pub_key)
                    {
                        pub_keys.push(pub_key);
                    }
                    pub_keys
                })
                .collect::<HashSet<_>>();


            if !pub_keys.is_empty() {
                tracing::debug!(
                    "BubbleMerge: found {} potential peers to join",
                    pub_keys.len()
                );

                self.gossip_sender
                    .join_peers(
                        pub_keys.iter().cloned().collect::<Vec<_>>(),
                        Some(self.max_join_peers),
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
