use std::sync::Arc;

use sha2::Digest;

use crate::TopicId;

pub trait SecretRotation: Send + Sync {
    fn derive(
        &self,
        topic_hash: [u8; 32],
        unix_minute: u64,
        initial_secret_hash: [u8; 32],
    ) -> [u8; 32];
}

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
    pub fn new(rotation: impl SecretRotation + 'static) -> Self {
        Self(Arc::new(rotation))
    }

    pub fn derive(&self, topic_hash: [u8; 32], unix_minute: u64, initial_secret_hash: [u8; 32]) -> [u8; 32] {
        self.0.derive(topic_hash, unix_minute, initial_secret_hash)
    }
}

pub fn signing_keypair(topic_id: &TopicId, unix_minute: u64) -> ed25519_dalek::SigningKey {
    let mut sign_keypair_hash = sha2::Sha512::new();
    sign_keypair_hash.update(topic_id.hash);
    sign_keypair_hash.update(unix_minute.to_le_bytes());
    let sign_keypair_seed: [u8; 32] = sign_keypair_hash.finalize()[..32]
        .try_into()
        .expect("hashing failed");
    ed25519_dalek::SigningKey::from_bytes(&sign_keypair_seed)
}

pub fn encryption_keypair(
    topic_id: &TopicId,
    secret_rotation_function: &RotationHandle,
    initial_secret_hash: [u8; 32],
    unix_minute: u64,
) -> ed25519_dalek::SigningKey {
    let enc_keypair_seed =
        secret_rotation_function
            .0
            .derive(topic_id.hash, unix_minute, initial_secret_hash);
    ed25519_dalek::SigningKey::from_bytes(&enc_keypair_seed)
}

// salt = hash (topic + unix_minute)
pub fn salt(topic_id: &TopicId, unix_minute: u64) -> [u8; 32] {
    let mut slot_hash = sha2::Sha512::new();
    slot_hash.update(topic_id.hash);
    slot_hash.update(unix_minute.to_le_bytes());
    slot_hash.finalize()[..32]
        .try_into()
        .expect("hashing failed")
}
