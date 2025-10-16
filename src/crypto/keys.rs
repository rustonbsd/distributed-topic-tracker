use std::sync::Arc;

use sha2::Digest;

use crate::crypto::RecordTopic;

/// Trait for deriving time-rotated encryption keys.
///
/// Implementations control how encryption keys rotate based on time,
/// providing key isolation across time slots.
pub trait SecretRotation: Send + Sync {
    /// Derive an encryption key for a specific time slot.
    ///
    /// # Arguments
    ///
    /// * `topic_hash` - 32-byte topic identifier
    /// * `unix_minute` - Time slot (minute precision)
    /// * `initial_secret_hash` - 32-byte hashed initial secret
    ///
    /// # Returns
    ///
    /// A 32-byte derived key unique to this topic/time combination.
    fn derive(
        &self,
        topic_hash: [u8; 32],
        unix_minute: u64,
        initial_secret_hash: [u8; 32],
    ) -> [u8; 32];
}

/// Default implementation: SHA512-based KDF.
///
/// Combines topic hash, time slot, and initial secret into a unique key.
#[derive(Debug, Clone)]
pub struct DefaultSecretRotation;

impl SecretRotation for DefaultSecretRotation {
    fn derive(
        &self,
        topic_hash: [u8; 32],
        unix_minute: u64,
        initial_secret_hash: [u8; 32],
    ) -> [u8; 32] {
        use sha2::Digest;
        let mut h = sha2::Sha512::new();
        h.update(topic_hash);
        h.update(unix_minute.to_be_bytes());
        h.update(initial_secret_hash);
        h.finalize()[..32].try_into().unwrap()
    }
}

/// Wrapper for custom or default secret rotation implementations.
///
/// Allows pluggable key derivation strategies while maintaining a consistent API.
#[derive(Clone)]
pub struct RotationHandle(Arc<dyn SecretRotation>);

impl core::fmt::Debug for RotationHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotationHandle").finish()
    }
}

impl Default for RotationHandle {
    fn default() -> Self {
        Self(Arc::new(DefaultSecretRotation))
    }
}

impl RotationHandle {
    /// Create a new rotation handle with a custom implementation.
    pub fn new(rotation: impl SecretRotation + 'static) -> Self {
        Self(Arc::new(rotation))
    }

    /// Derive a key using the underlying strategy.
    pub fn derive(
        &self,
        topic_hash: [u8; 32],
        unix_minute: u64,
        initial_secret_hash: [u8; 32],
    ) -> [u8; 32] {
        self.0.derive(topic_hash, unix_minute, initial_secret_hash)
    }
}

/// Derive Ed25519 signing key for DHT record authentication.
///
/// Keys are deterministic per topic and time slot, derived from topic hash.
/// All nodes use the same derived keypair for a given topic+time combination,
/// and its verifying key serves as the DHT routing key for storing/retrieving
/// bootstrap records. The actual record content is signed separately by each
/// node's individual keypair (not this one).
///
/// # Example
///
/// ```ignore
/// let topic = RecordTopic::from_str("my-topic")?;
/// let unix_minute = crate::unix_minute(0);
/// let signing_key = signing_keypair(topic, unix_minute);
/// ```
pub fn signing_keypair(record_topic: RecordTopic, unix_minute: u64) -> ed25519_dalek::SigningKey {
    let mut sign_keypair_hash = sha2::Sha512::new();
    sign_keypair_hash.update(record_topic.hash());
    sign_keypair_hash.update(unix_minute.to_le_bytes());
    let sign_keypair_seed: [u8; 32] = sign_keypair_hash.finalize()[..32]
        .try_into()
        .expect("hashing failed");
    ed25519_dalek::SigningKey::from_bytes(&sign_keypair_seed)
}

/// Derive Ed25519 key for HPKE encryption/decryption.
///
/// Incorporates the secret rotation strategy for time-slot isolation.
///
/// # Example
///
/// ```ignore
/// let topic = RecordTopic::from_str("my-topic")?;
/// let rotation = RotationHandle::default();
/// let enc_key = encryption_keypair(topic, &rotation, initial_hash, 0);
/// ```
pub fn encryption_keypair(
    record_topic: RecordTopic,
    secret_rotation_function: &RotationHandle,
    initial_secret_hash: [u8; 32],
    unix_minute: u64,
) -> ed25519_dalek::SigningKey {
    let enc_keypair_seed =
        secret_rotation_function
            .0
            .derive(record_topic.hash(), unix_minute, initial_secret_hash);
    ed25519_dalek::SigningKey::from_bytes(&enc_keypair_seed)
}

/// Derive DHT salt for mutable record lookups.
///
/// Salt = SHA512(topic_hash || unix_minute.to_le_bytes())[..32]
/// Ensures records are stored in different DHT slots per minute.
pub fn salt(record_topic: RecordTopic, unix_minute: u64) -> [u8; 32] {
    let mut slot_hash = sha2::Sha512::new();
    slot_hash.update(record_topic.hash());
    slot_hash.update(unix_minute.to_le_bytes());
    slot_hash.finalize()[..32]
        .try_into()
        .expect("hashing failed")
}
