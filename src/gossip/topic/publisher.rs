//! Background publisher that updates DHT records with active peer info.

use actor_helper::{Action, Handle, Receiver};
use std::time::Duration;

use crate::{GossipReceiver, MAX_MESSAGE_HASHES, MAX_RECORD_PEERS, RecordPublisher};
use anyhow::Result;

/// Periodically publishes node state to DHT for peer discovery.
///
/// Publishes a record after an initial delay initial_delay, then repeatedly with
/// randomized base_interval + rand(0 to max_jitter) intervals, containing this node's active neighbor list
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
    base_interval: Duration,
    max_jitter: Duration,
}

impl Publisher {
    /// Create a new background publisher.
    ///
    /// Spawns a background task that periodically publishes records.
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_receiver: GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
        initial_delay: Duration,
        base_interval: Duration,
        max_jitter: Duration,
    ) -> Result<Self> {
        let base_interval = base_interval.max(Duration::from_secs(1));
        let mut ticker =
            tokio::time::interval_at(tokio::time::Instant::now() + initial_delay, base_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let api = Handle::spawn_with(
            PublisherActor {
                record_publisher,
                gossip_receiver,
                ticker,
                cancel_token,
                base_interval,
                max_jitter,
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
                    let jitter_ms = if self.max_jitter.as_millis() > 0 {
                        rand::random::<u128>() % self.max_jitter.as_millis()
                    } else {
                        0
                    };
                    let next_interval = self.base_interval.as_millis() + jitter_ms;
                    tracing::debug!("Publisher: next publish in {}ms", next_interval);
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

impl PublisherActor {
    async fn publish(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);

        let active_peers = self
            .gossip_receiver
            .neighbors()
            .await?
            .iter()
            .filter_map(|pub_key| pub_key.as_bytes().as_ref().try_into().ok())
            .take(MAX_RECORD_PEERS)
            .collect::<Vec<_>>();

        let last_message_hashes = self
            .gossip_receiver
            .last_message_hashes()
            .await?
            .iter()
            .filter_map(|hash| hash.as_ref().try_into().ok())
            .take(MAX_MESSAGE_HASHES)
            .collect::<Vec<_>>();

        tracing::debug!(
            "Publisher: publishing record for unix_minute {} with {} active_peers and {} message_hashes",
            unix_minute,
            active_peers.len(),
            last_message_hashes.len()
        );

        let record_content = crate::gossip::GossipRecordContent {
            active_peers: {
                let mut peers = [Default::default(); MAX_RECORD_PEERS];
                peers[..active_peers.len()].copy_from_slice(&active_peers);
                peers
            },
            last_message_hashes: {
                let mut hashes = [Default::default(); MAX_MESSAGE_HASHES];
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
