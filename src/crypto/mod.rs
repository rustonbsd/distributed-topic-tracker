//! Cryptographic primitives for DHT record signing and encryption.
//!
//! This module handles Ed25519 signing keys, HPKE encryption, and secret rotation
//! for securing DHT records used in peer discovery.

mod keys;
mod record;

pub use keys::{
    DefaultSecretRotation, RotationHandle, SecretRotation, encryption_keypair, salt,
    signing_keypair,
};
pub use record::{EncryptedRecord, Record, RecordPublisher, RecordTopic};
