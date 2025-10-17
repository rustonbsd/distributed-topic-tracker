#![doc = include_str!("../README.md")]

mod crypto;
mod dht;

#[cfg(feature = "iroh-gossip")]
mod gossip;
#[cfg(feature = "iroh-gossip")]
pub use gossip::{
    AutoDiscoveryGossip, Bootstrap, BubbleMerge, GossipReceiver, GossipRecordContent, GossipSender,
    MessageOverlapMerge, Publisher, Topic, TopicId,
};

pub use crypto::{
    DefaultSecretRotation, EncryptedRecord, Record, RecordPublisher, RecordTopic, RotationHandle,
    SecretRotation, encryption_keypair, salt, signing_keypair,
};
pub use dht::Dht;

/// Maximum number of bootstrap records allowed per topic per time slot (minute).
/// 
/// When publishing to the DHT, records are not published if this threshold
/// has already been reached for the current minute slot.
pub const MAX_BOOTSTRAP_RECORDS: usize = 100;

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
pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}
