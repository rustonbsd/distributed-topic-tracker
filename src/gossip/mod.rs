//! Decentralized bootstrap for iroh-gossip topics via DHT auto-discovery.
//!
//! Combines iroh-gossip with mainline DHT to enable automatic peer discovery
//! and topic joining without prior knowledge of peers. Includes bubble detection
//! and message overlap merging for cluster topology optimization.

mod merge;
mod receiver;
mod sender;
mod topic;

pub use merge::{BubbleMerge, MessageOverlapMerge};
pub use receiver::GossipReceiver;
pub use sender::GossipSender;
use serde::{Deserialize, Serialize};
pub use topic::{Bootstrap, Publisher, Topic, TopicId};

use crate::RecordPublisher;

/// Record content for peer discovery.
///
/// Stored in DHT records to advertise this node's active peers
/// and recently seen message hashes for cluster merging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipRecordContent {
    /// Fixed array of 5 peer node IDs (as 32-byte arrays, empty slots are zero-filled)
    pub active_peers: [[u8; 32]; 5],
    /// Fixed array of 5 recent message hashes for overlap detection (empty slots are zero-filled)
    pub last_message_hashes: [[u8; 32]; 5],
}

/// Extension trait for iroh Gossip enabling auto-discovery.
pub trait AutoDiscoveryGossip {
    /// Subscribe to a topic and bootstrap with DHT peer discovery.
    ///
    /// Starts bootstrap and waits until at least one neighbor connection is established.
    /// Returns a `Topic` for sending/receiving messages.
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        record_publisher: RecordPublisher,
    ) -> anyhow::Result<Topic>;

    /// Subscribe to a topic and bootstrap asynchronously.
    ///
    /// Returns immediately with a `Topic` while bootstrap proceeds in background.
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        record_publisher: RecordPublisher,
    ) -> anyhow::Result<Topic>;
}

impl AutoDiscoveryGossip for iroh_gossip::net::Gossip {
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        record_publisher: RecordPublisher,
    ) -> anyhow::Result<Topic> {
        Topic::new(record_publisher, self.clone(), false).await
    }

    async fn subscribe_and_join_with_auto_discovery_no_wait(
        &self,
        record_publisher: RecordPublisher,
    ) -> anyhow::Result<Topic> {
        Topic::new(record_publisher, self.clone(), true).await
    }
}
