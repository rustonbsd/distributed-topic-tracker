#![doc = include_str!("../README.md")]

#[cfg(not(feature = "experimental"))]
mod crypto;
#[cfg(not(feature = "experimental"))]
mod dht;
#[cfg(feature = "experimental")]
mod dht_experimental;

mod core;

pub use core::*;

#[cfg(not(feature = "experimental"))]
#[cfg(feature = "iroh-gossip")]
mod gossip;
#[cfg(not(feature = "experimental"))]
#[cfg(feature = "iroh-gossip")]
pub use gossip::{
    AutoDiscoveryGossip, Bootstrap, BubbleMerge, GossipReceiver, GossipRecordContent, GossipSender,
    MessageOverlapMerge, Publisher, Topic, TopicId,
};
#[cfg(feature = "experimental")]
#[cfg(feature = "iroh-gossip")]
mod gossip_experimental;
#[cfg(feature = "experimental")]
#[cfg(feature = "iroh-gossip")]
pub use gossip_experimental::{
    AutoDiscoveryGossip, Topic
};


#[cfg(not(feature = "experimental"))]
pub use crypto::{
    DefaultSecretRotation, EncryptedRecord, Record, RecordPublisher, RecordTopic, RotationHandle,
    SecretRotation, encryption_keypair, salt, signing_keypair,
};

#[cfg(feature = "experimental")]
pub use mainline_exp::Dht;
#[cfg(not(feature = "experimental"))]
pub use mainline::Dht;





/// Maximum number of bootstrap records allowed per topic per time slot (minute).
///
/// When publishing to the DHT, records are not published if this threshold
/// has already been reached for the current minute slot.
#[cfg(not(feature = "experimental"))]
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
#[cfg(not(feature = "experimental"))]
pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}
