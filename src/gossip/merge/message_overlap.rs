//! Split-brain detection via message hash overlap in DHT records.

use actor_helper::{Action, Handle, Receiver};
use iroh::EndpointId;
use std::{collections::HashSet, time::Duration};

use crate::{GossipReceiver, GossipSender, RecordPublisher, gossip::GossipRecordContent};
use anyhow::Result;

/// Detects network partitions by comparing message hashes across DHT records.
///
/// Joins peers whose published hashes do not overlap with local message history.
#[derive(Debug, Clone)]
pub struct MessageOverlapMerge {
    _api: Handle<MessageOverlapMergeActor, anyhow::Error>,
}

#[derive(Debug)]
struct MessageOverlapMergeActor {
    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    gossip_sender: GossipSender,
    ticker: tokio::time::Interval,
    cancel_token: tokio_util::sync::CancellationToken,

    max_join_peers: usize,
    base_interval: Duration,
    max_jitter: Duration,
}

impl MessageOverlapMerge {
    /// Create a new split-brain detector.
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
        max_join_peers: usize,
        base_interval: Duration,
        max_jitter: Duration,
    ) -> Result<Self> {
        let base_interval = base_interval.max(Duration::from_secs(1));
        let mut ticker = tokio::time::interval(base_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let api = Handle::spawn_with(
            MessageOverlapMergeActor {
                record_publisher,
                gossip_receiver,
                gossip_sender,
                ticker,
                cancel_token,
                max_join_peers,
                base_interval,
                max_jitter,
            },
            |mut actor, rx| async move { actor.run(rx).await },
        )
        .0;
        Ok(Self { _api: api })
    }
}

impl MessageOverlapMergeActor {
    async fn run(&mut self, rx: Receiver<Action<MessageOverlapMergeActor>>) -> Result<()> {
        tracing::debug!("MessageOverlapMerge: starting message overlap merge actor");
        loop {
            tokio::select! {
                result = rx.recv_async() => {
                    match result {
                        Ok(action) => action(self).await,
                        Err(_) => break Ok(()),
                    }
                }
                _ = self.ticker.tick() => {
                    tracing::debug!("MessageOverlapMerge: tick fired, checking for split-brain");
                    let _ = self.merge().await;
                    let jitter_ms = if self.max_jitter.as_millis() > 0 {
                        rand::random::<u128>() % self.max_jitter.as_millis()
                    } else {
                        0
                    };
                    let next_interval = self.base_interval.as_millis() + jitter_ms;
                    tracing::debug!("MessageOverlapMerge: next check in {}ms", next_interval);
                    self.ticker.reset_after(Duration::from_millis(next_interval as u64));
                }
                _ = self.cancel_token.cancelled() => {
                    break Ok(());
                }
                else => break Ok(()),
            }
        }
    }
}

impl MessageOverlapMergeActor {
    async fn merge(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let mut records = self.record_publisher.get_records(unix_minute - 1).await;
        records.extend(self.record_publisher.get_records(unix_minute).await);

        let local_hashes = self.gossip_receiver.last_message_hashes().await?;
        tracing::debug!(
            "MessageOverlapMerge: checking {} records with {} local message hashes",
            records.len(),
            local_hashes.len()
        );

        if !local_hashes.is_empty() {
            let last_message_hashes = local_hashes;
            let peers_to_join = records
                .iter()
                .filter(|record| {
                    if let Ok(content) = record.content::<GossipRecordContent>() {
                        content.last_message_hashes.iter().any(|last_message_hash| {
                            *last_message_hash != [0; 32]
                                && !last_message_hashes.contains(last_message_hash)
                        })
                    } else {
                        false
                    }
                })
                .collect::<Vec<_>>();

            tracing::debug!(
                "MessageOverlapMerge: found {} peers with no overlapping message hashes",
                peers_to_join.len()
            );

            if !peers_to_join.is_empty() {
                let active_neighbors = self.gossip_receiver.neighbors().await?;
                let node_ids = peers_to_join
                    .iter()
                    .flat_map(|&record| {
                        let mut peers = vec![];
                        if let Ok(endpoint_id) = EndpointId::from_bytes(&record.node_id())
                            && endpoint_id
                                != EndpointId::from_verifying_key(self.record_publisher.pub_key())
                        {
                            peers.push(endpoint_id);
                        }
                        if let Ok(content) = record.content::<GossipRecordContent>() {
                            for active_peer in content.active_peers {
                                if active_peer == [0; 32] {
                                    continue;
                                }
                                if let Ok(endpoint_id) = EndpointId::from_bytes(&active_peer)
                                    && endpoint_id
                                        != EndpointId::from_verifying_key(
                                            self.record_publisher.pub_key(),
                                        )
                                {
                                    peers.push(endpoint_id);
                                }
                            }
                        }
                        peers
                    })
                    .filter(|node_id| !active_neighbors.contains(node_id))
                    .collect::<HashSet<_>>();

                tracing::debug!(
                    "MessageOverlapMerge: attempting to join {} node_ids with no overlapping messages",
                    node_ids.len()
                );

                self.gossip_sender
                    .join_peers(
                        node_ids.iter().cloned().collect::<Vec<_>>(),
                        Some(self.max_join_peers),
                    )
                    .await?;

                tracing::debug!(
                    "MessageOverlapMerge: join_peers request sent for split-brain recovery"
                );
            }
        } else {
            tracing::debug!(
                "MessageOverlapMerge: no local message hashes yet, skipping overlap detection"
            );
        }
        Ok(())
    }
}
