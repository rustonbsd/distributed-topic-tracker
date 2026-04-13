//! Background publisher that updates DHT records with active peer info.

use actor_helper::{Action, Handle, Receiver};
use std::time::Duration;

use crate::{GossipReceiver, RecordPublisher};
use anyhow::Result;

/// Periodically publishes node state to DHT for peer discovery.
///
/// Publishes a record after an initial 10s delay, then repeatedly with
/// randomized 0-49s intervals, containing this node's active neighbor list
/// and message hashes for bubble detection and merging.
#[derive(Debug, Clone)]
pub struct Publisher {
    _api: Handle<PublisherActor, anyhow::Error>,
}

#[derive(Debug)]
struct PublisherActor {
    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    ticker: tokio::time::Interval,
    cancel_token: tokio_util::sync::CancellationToken,
}

impl Publisher {
    /// Create a new background publisher.
    ///
    /// Spawns a background task that periodically publishes records.
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_receiver: GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<Self> {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let api = Handle::spawn_with(
            PublisherActor {
                record_publisher,
                gossip_receiver,
                ticker,
                cancel_token,
            },
            |mut actor, rx| async move { actor.run(rx).await },
        )
        .0;

        Ok(Self { _api: api })
    }
}

impl PublisherActor {
    async fn run(&mut self, rx: Receiver<Action<PublisherActor>>) -> Result<()> {
        tracing::debug!("Publisher: starting publisher actor");
        loop {
            tokio::select! {
                result = rx.recv_async() => {
                    match result {
                        Ok(action) => action(self).await,
                        Err(_) => break Ok(()),
                    }
                }
                _ = self.ticker.tick() => {
                    tracing::debug!("Publisher: tick fired, attempting to publish");
                    let _ = self.publish().await;
                    let next_interval = rand::random::<u64>() % 50;
                    tracing::debug!("Publisher: next publish in {}s", next_interval);
                    self.ticker.reset_after(Duration::from_secs(next_interval));
                }
                _ = self.cancel_token.cancelled() => {
                    break Ok(());
                }
                else => break Ok(()),
            }
        }
    }
}

impl PublisherActor {
    async fn publish(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);

        let active_peers = self
            .gossip_receiver
            .neighbors()
            .await
            .iter()
            .filter_map(|pub_key| TryInto::<[u8; 32]>::try_into(pub_key.as_slice()).ok())
            .take(5)
            .collect::<Vec<_>>();

        let last_message_hashes = self
            .gossip_receiver
            .last_message_hashes()
            .await
            .iter()
            .filter_map(|hash| TryInto::<[u8; 32]>::try_into(hash.as_slice()).ok())
            .take(5)
            .collect::<Vec<_>>();

        tracing::debug!(
            "Publisher: publishing record for unix_minute {} with {} active_peers and {} message_hashes",
            unix_minute,
            active_peers.len(),
            last_message_hashes.len()
        );

        let record_content = crate::gossip::GossipRecordContent {
            active_peers: {
                let mut peers = [Default::default(); 5];
                peers[..active_peers.len()].copy_from_slice(&active_peers);
                peers
            },
            last_message_hashes: {
                let mut hashes = [Default::default(); 5];
                hashes[..last_message_hashes.len()].copy_from_slice(&last_message_hashes);
                hashes
            },
        };

        tracing::debug!("Publisher: created record content: {:?}", record_content);

        let res = self
            .record_publisher
            .new_record(unix_minute, record_content);
        tracing::debug!("Publisher: created new record: {:?}", res);
        let record = res?;
        let result = self.record_publisher.publish_record(record).await;

        if result.is_ok() {
            tracing::debug!("Publisher: successfully published record");
        } else {
            tracing::debug!("Publisher: failed to publish record: {:?}", result);
        }

        result
    }
}
