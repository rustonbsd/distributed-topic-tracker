//! Peer merging strategies for recovering from network partitions.
//!
//! Two complementary strategies detect and heal split-brain scenarios:
//! - **Bubble Merge**: Joins small clusters (< 4 peers) with peers advertised in DHT
//! - **Message Overlap**: Detects when isolated clusters share common message hashes,
//!   indicating they have seen the same messages and can be merged

mod bubble;
mod message_overlap;

pub use bubble::BubbleMerge;
pub use message_overlap::MessageOverlapMerge;

pub const MAX_JOIN_PEERS_COUNT: usize = 30;
