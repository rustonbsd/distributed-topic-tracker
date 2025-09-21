mod crypto;
mod dht;

#[cfg(feature = "iroh-gossip")]
mod gossip;
#[cfg(feature = "iroh-gossip")]
pub use gossip::{
    AutoDiscoveryGossip, Bootstrap, BubbleMerge, GossipReceiver, GossipSender, MessageOverlapMerge,
    Publisher, Topic, TopicId,
};

pub use crypto::{
    DefaultSecretRotation, EncryptedRecord, Record, RecordPublisher, RecordTopic, RotationHandle,
    SecretRotation, encryption_keypair, salt, signing_keypair,
};
pub use dht::Dht;
pub const MAX_BOOTSTRAP_RECORDS: usize = 10;

pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}
