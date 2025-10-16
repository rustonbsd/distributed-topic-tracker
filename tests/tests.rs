use std::str::FromStr;

use distributed_topic_tracker::{
    DefaultSecretRotation, EncryptedRecord, GossipRecordContent, Record, RecordTopic,
    RotationHandle, encryption_keypair, salt, signing_keypair, unix_minute,
};
use mainline::SigningKey;
use rand::rngs::OsRng;

#[test]
fn test_record_serialization_roundtrip() {
    let signing_key = SigningKey::generate(&mut OsRng);
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let node_id = [2u8; 32];
    let active_peers = [[3u8; 32]; 5];
    let last_message_hashes = [[4u8; 32]; 5];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(
        topic,
        unix_minute,
        node_id,
        record_content.clone(),
        &signing_key,
    )
    .expect("Failed to sign record");

    // Test serialization roundtrip
    let bytes = record.to_bytes();
    let deserialized = Record::from_bytes(bytes).expect("Failed to deserialize record");

    let deserialized_content: GossipRecordContent =
        deserialized.content().expect("Failed to get content");

    assert_eq!(record.topic(), deserialized.topic());
    assert_eq!(record.unix_minute(), deserialized.unix_minute());
    assert_eq!(record.node_id(), deserialized.node_id());
    assert_eq!(
        record_content.active_peers,
        deserialized_content.active_peers
    );
    assert_eq!(
        record_content.last_message_hashes,
        deserialized_content.last_message_hashes
    );
    assert_eq!(record.signature(), deserialized.signature());
}

#[test]
fn test_record_verification() {
    let signing_key = SigningKey::generate(&mut OsRng);
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let node_id = signing_key.verifying_key().to_bytes();
    let active_peers = [[3u8; 32]; 5];
    let last_message_hashes = [[4u8; 32]; 5];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, node_id, record_content, &signing_key).unwrap();

    // Valid verification should pass
    assert!(record.verify(&topic, unix_minute).is_ok());

    // Wrong topic should fail
    let wrong_topic = [99u8; 32];
    assert!(record.verify(&wrong_topic, unix_minute).is_err());

    // Wrong unix_minute should fail
    assert!(record.verify(&topic, unix_minute + 1).is_err());
}

#[test]
fn test_encrypted_record_roundtrip() {
    let signing_key = SigningKey::generate(&mut OsRng);
    let encryption_key = SigningKey::generate(&mut OsRng);
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let node_id = signing_key.verifying_key().to_bytes();
    let active_peers = [[3u8; 32]; 5];
    let last_message_hashes = [[4u8; 32]; 5];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(
        topic,
        unix_minute,
        node_id,
        record_content.clone(),
        &signing_key,
    )
    .expect("Failed to sign record");

    // Test encryption/decryption roundtrip
    let encrypted = record.encrypt(&encryption_key);
    let decrypted = encrypted.decrypt(&encryption_key).unwrap();

    let deserialized_content: GossipRecordContent =
        decrypted.content().expect("Failed to get content");

    assert_eq!(record.topic(), decrypted.topic());
    assert_eq!(record.unix_minute(), decrypted.unix_minute());
    assert_eq!(record.node_id(), decrypted.node_id());
    assert_eq!(
        record_content.active_peers,
        deserialized_content.active_peers
    );
    assert_eq!(
        record_content.last_message_hashes,
        deserialized_content.last_message_hashes
    );
    assert_eq!(record.signature(), decrypted.signature());
}

#[test]
fn test_encrypted_record_serialization() {
    let signing_key = SigningKey::generate(&mut OsRng);
    let encryption_key = SigningKey::generate(&mut OsRng);
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let node_id = signing_key.verifying_key().to_bytes();
    let active_peers = [[3u8; 32]; 5];
    let last_message_hashes = [[4u8; 32]; 5];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, node_id, record_content, &signing_key)
        .expect("Failed to sign record");

    let encrypted = record.encrypt(&encryption_key);

    // Test serialization roundtrip
    let bytes = encrypted.to_bytes();
    let deserialized =
        EncryptedRecord::from_bytes(bytes).expect("Failed to deserialize encrypted record");

    // Should be able to decrypt the deserialized version
    let decrypted = deserialized
        .decrypt(&encryption_key)
        .expect("Failed to decrypt record");
    assert_eq!(record.topic(), decrypted.topic());
    assert_eq!(record.unix_minute(), decrypted.unix_minute());
}

#[test]
fn test_default_secret_rotation() {
    let rotation = RotationHandle::new(DefaultSecretRotation);
    let topic_hash = [1u8; 32];
    let unix_minute = 12345u64;
    let initial_secret_hash = [2u8; 32];

    let secret1 = rotation.derive(topic_hash, unix_minute, initial_secret_hash);
    let secret2 = rotation.derive(topic_hash, unix_minute, initial_secret_hash);

    // Same inputs should produce same secret
    assert_eq!(secret1, secret2);

    // Different unix_minute should produce different secret
    let secret3 = rotation.derive(topic_hash, unix_minute + 1, initial_secret_hash);
    assert_ne!(secret1, secret3);

    // Different topic should produce different secret
    let different_topic = [99u8; 32];
    let secret4 = rotation.derive(different_topic, unix_minute, initial_secret_hash);
    assert_ne!(secret1, secret4);
}

#[test]
fn test_unix_minute_function() {
    let current = unix_minute(0);
    let prev = unix_minute(-1);
    let next = unix_minute(1);

    assert_eq!(current, prev + 1);
    assert_eq!(next, current + 1);

    // Should be deterministic
    let current2 = unix_minute(0);
    assert_eq!(current, current2);
}

#[test]
fn test_topic_signing_keypair_deterministic() {
    let topic_id = RecordTopic::from_str("test-topic").unwrap();
    let record_topic = topic_id.into();
    let unix_minute = 12345u64;

    let key1 = signing_keypair(record_topic, unix_minute);
    let key2 = signing_keypair(record_topic, unix_minute);

    // Same inputs should produce same keypair
    assert_eq!(key1.to_bytes(), key2.to_bytes());

    // Different unix_minute should produce different keypair
    let key3 = signing_keypair(record_topic, unix_minute + 1);
    assert_ne!(key1.to_bytes(), key3.to_bytes());
}

#[test]
fn test_topic_encryption_keypair_deterministic() {
    let topic_id = RecordTopic::from_str("test-topic").unwrap();
    let record_topic = topic_id.into();
    let unix_minute = 12345u64;
    let initial_secret_hash = [1u8; 32];
    let rotation = RotationHandle::new(DefaultSecretRotation);

    let key1 = encryption_keypair(record_topic, &rotation, initial_secret_hash, unix_minute);
    let key2 = encryption_keypair(record_topic, &rotation, initial_secret_hash, unix_minute);

    // Same inputs should produce same keypair
    assert_eq!(key1.to_bytes(), key2.to_bytes());

    // Different unix_minute should produce different keypair
    let key3 = crate::encryption_keypair(
        record_topic,
        &rotation,
        initial_secret_hash,
        unix_minute + 1,
    );
    assert_ne!(key1.to_bytes(), key3.to_bytes());
}

#[test]
fn test_topic_salt_deterministic() {
    let topic_id = RecordTopic::from_str("test-topic").unwrap();
    let record_topic = topic_id.into();
    let unix_minute = 12345u64;

    let salt1 = salt(record_topic, unix_minute);
    let salt2 = salt(record_topic, unix_minute);

    // Same inputs should produce same salt
    assert_eq!(salt1, salt2);

    // Different unix_minute should produce different salt
    let salt3 = salt(record_topic, unix_minute + 1);
    assert_ne!(salt1, salt3);
}
