#![doc = include_str!("../README.md")]

mod config;
mod crypto;
mod dht;

#[cfg(feature = "iroh-gossip")]
mod gossip;
#[cfg(feature = "iroh-gossip")]
pub use gossip::{
    AutoDiscoveryGossip, Bootstrap, BubbleMerge, GossipReceiver, GossipRecordContent, GossipSender,
    MessageOverlapMerge, Publisher, Topic, TopicId,
};

pub use config::{
    BootstrapConfig, BubbleMergeConfig, Config, ConfigBuilder, DhtConfig, MergeConfig,
    MessageOverlapMergeConfig, PublisherConfig, TimeoutConfig, TimeoutConfigBuilder,
};
pub use crypto::{
    DefaultSecretRotation, EncryptedRecord, Record, RecordPublisher, RotationHandle,
    SecretRotation, encryption_keypair, salt, signing_keypair,
};
pub use dht::Dht;

pub const MAX_RECORD_PEERS: usize = 5;
pub const MAX_MESSAGE_HASHES: usize = 5;

/// Get the current Unix minute timestamp, optionally offset.
///
/// # Arguments
///
/// * `minute_offset` - Offset in minutes from now (can be negative)
///
/// # Example
///
/// ```ignore
/// let now = unix_minute(0);
/// let prev_minute = unix_minute(-1);
/// ```
#[doc(hidden)]
pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() / 60).saturating_add(minute_offset))
        .try_into()
        .expect("timestamp overflow")
}
