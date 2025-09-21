mod sender;
mod receiver;
mod topic;
mod merge;

pub use sender::GossipSender;
pub use receiver::GossipReceiver;
use serde::{Deserialize, Serialize};
pub use topic::{Topic, TopicId, Publisher, Bootstrap};
pub use merge::{BubbleMerge, MessageOverlapMerge};

use crate::RecordPublisher;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipRecordContent {    
    pub active_peers: [[u8; 32]; 5],
    pub last_message_hashes: [[u8; 32]; 5],
}

pub trait AutoDiscoveryGossip {
    #[allow(async_fn_in_trait)]
    async fn subscribe_and_join_with_auto_discovery(
        &self,
        record_publisher: RecordPublisher,
    ) -> anyhow::Result<Topic>;

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
    ) -> anyhow::Result<Topic> {
        Topic::new(
            record_publisher,
            self.clone(),
            true,
        )
        .await
    }
}