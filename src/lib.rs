mod actor;
mod crypto;
mod dht;
mod gossip;
mod topic;
mod merge;

#[cfg(test)]
mod tests;

pub use crypto::keys::{DefaultSecretRotation,RotationHandle,SecretRotation,signing_keypair,encryption_keypair,salt};
pub use crypto::record::{Record,EncryptedRecord, RecordPublisher};
pub use topic::topic::{Topic, TopicId};
pub use gossip::sender::GossipSender;
pub use gossip::receiver::GossipReceiver;
pub use dht::Dht;

use iroh_gossip::net::Gossip;
use anyhow::Result;


pub const MAX_JOIN_PEERS_COUNT: usize = 30;
pub const MAX_BOOTSTRAP_RECORDS: usize = 10;

pub trait AutoDiscoveryGossip {
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        record_publisher: RecordPublisher,
    ) -> Result<Topic>;

    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        record_publisher: RecordPublisher,
    ) -> Result<Topic>;
}

impl AutoDiscoveryGossip for Gossip {
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        record_publisher: RecordPublisher,
    ) -> Result<Topic> {
        Topic::new(
            record_publisher,
            self.clone(),
            false,
        )
        .await
    }

    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        record_publisher: RecordPublisher,
    ) -> Result<Topic> {
        Topic::new(
            record_publisher,
            self.clone(),
            true,
        )
        .await
    }
}

pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}