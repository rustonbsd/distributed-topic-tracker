use std::{str::FromStr, sync::Arc};

use distributed_topic_tracker::{
    AutoDiscoveryGossip, BubbleMergeConfig, Config, DefaultSecretRotation, EncryptedRecord,
    GossipReceiver, GossipRecordContent, MAX_MESSAGE_HASHES, MAX_RECORD_PEERS, MergeConfig,
    MessageOverlapMergeConfig, PublisherConfig, Record, RecordPublisher, RotationHandle, TopicId,
    encryption_keypair, salt, signing_keypair, unix_minute,
};
use mainline::SigningKey;
use tokio::sync::Barrier;

#[test]
fn test_record_serialization_roundtrip() {
    let signing_key = SigningKey::generate(&mut rand::rng());
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let active_peers = [[3u8; 32]; MAX_RECORD_PEERS];
    let last_message_hashes = [[4u8; 32]; MAX_MESSAGE_HASHES];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, record_content.clone(), &signing_key)
        .expect("failed to sign record");

    // Test serialization roundtrip
    let bytes = record.to_bytes();
    let deserialized = Record::from_bytes(bytes).expect("failed to deserialize record");

    let deserialized_content: GossipRecordContent =
        deserialized.content().expect("failed to get content");

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
    let signing_key = SigningKey::generate(&mut rand::rng());
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let active_peers = [[3u8; 32]; MAX_RECORD_PEERS];
    let last_message_hashes = [[4u8; 32]; MAX_MESSAGE_HASHES];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, record_content, &signing_key)
        .expect("failed to sign record");

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
    let signing_key = SigningKey::generate(&mut rand::rng());
    let encryption_key = SigningKey::generate(&mut rand::rng());
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let active_peers = [[3u8; 32]; MAX_RECORD_PEERS];
    let last_message_hashes = [[4u8; 32]; MAX_MESSAGE_HASHES];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, record_content.clone(), &signing_key)
        .expect("failed to sign record");

    // Test encryption/decryption roundtrip
    let encrypted = record.encrypt(&encryption_key);
    let decrypted = encrypted
        .decrypt(&encryption_key)
        .expect("failed to decrypt record");

    let deserialized_content: GossipRecordContent =
        decrypted.content().expect("failed to get content");

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
    let signing_key = SigningKey::generate(&mut rand::rng());
    let encryption_key = SigningKey::generate(&mut rand::rng());
    let topic = [1u8; 32];
    let unix_minute = 12345u64;
    let active_peers = [[3u8; 32]; MAX_RECORD_PEERS];
    let last_message_hashes = [[4u8; 32]; MAX_MESSAGE_HASHES];

    let record_content = GossipRecordContent {
        active_peers,
        last_message_hashes,
    };

    let record = Record::sign(topic, unix_minute, record_content, &signing_key)
        .expect("failed to sign record");

    let encrypted = record.encrypt(&encryption_key);

    // Test serialization roundtrip
    let bytes = encrypted.to_bytes();
    let deserialized =
        EncryptedRecord::from_bytes(bytes).expect("failed to deserialize encrypted record");

    // Should be able to decrypt the deserialized version
    let decrypted = deserialized
        .decrypt(&encryption_key)
        .expect("failed to decrypt record");
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
    let topic_id = TopicId::from_str("test-topic").expect("failed to create TopicId from_str");
    let unix_minute = 12345u64;

    let key1 = signing_keypair(&topic_id, unix_minute);
    let key2 = signing_keypair(&topic_id, unix_minute);

    // Same inputs should produce same keypair
    assert_eq!(key1.to_bytes(), key2.to_bytes());

    // Different unix_minute should produce different keypair
    let key3 = signing_keypair(&topic_id, unix_minute + 1);
    assert_ne!(key1.to_bytes(), key3.to_bytes());
}

#[test]
fn test_topic_encryption_keypair_deterministic() {
    let topic_id = TopicId::from_str("test-topic").expect("failed to create TopicId from_str");
    let unix_minute = 12345u64;
    let initial_secret_hash = [1u8; 32];
    let rotation = RotationHandle::new(DefaultSecretRotation);

    let key1 = encryption_keypair(&topic_id, &rotation, initial_secret_hash, unix_minute);
    let key2 = encryption_keypair(&topic_id, &rotation, initial_secret_hash, unix_minute);

    // Same inputs should produce same keypair
    assert_eq!(key1.to_bytes(), key2.to_bytes());

    // Different unix_minute should produce different keypair
    let key3 = encryption_keypair(&topic_id, &rotation, initial_secret_hash, unix_minute + 1);
    assert_ne!(key1.to_bytes(), key3.to_bytes());
}

#[test]
fn test_topic_salt_deterministic() {
    let topic_id = TopicId::from_str("test-topic").expect("failed to create TopicId from_str");
    let unix_minute = 12345u64;

    let salt1 = salt(&topic_id, unix_minute);
    let salt2 = salt(&topic_id, unix_minute);

    // Same inputs should produce same salt
    assert_eq!(salt1, salt2);

    // Different unix_minute should produce different salt
    let salt3 = salt(&topic_id, unix_minute + 1);
    assert_ne!(salt1, salt3);
}

#[test]
fn test_topic_id_creation() {
    let topic_id = TopicId::new("test-topic".to_string());
    assert_eq!(topic_id.hash().len(), 32);

    // Same input should produce same hash
    let topic_id2 = TopicId::new("test-topic".to_string());
    assert_eq!(topic_id.hash(), topic_id2.hash());

    // Different input should produce different hash
    let topic_id3 = TopicId::new("different-topic".to_string());
    assert_ne!(topic_id.hash(), topic_id3.hash());
}

#[cfg(feature = "iroh-gossip")]
#[tokio::test]
async fn test_multiple_receivers_all_get_events() {
    const N: usize = 3;
    const MSG_COUNT: usize = 3;

    let config = Config::builder()
        .publisher_config(PublisherConfig::Disabled)
        .merge_config(MergeConfig::new(
            BubbleMergeConfig::Disabled,
            MessageOverlapMergeConfig::Disabled,
        ))
        .build();

    let topic_id = TopicId::new("test-multi-receiver".to_string());

    // Peer A
    let secret_a = iroh::SecretKey::generate(&mut rand::rng());
    let signing_a = mainline::SigningKey::from_bytes(&secret_a.to_bytes());
    let endpoint_a = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_a)
        .bind()
        .await
        .expect("failed to bind endpoint A");
    let gossip_a = iroh_gossip::net::Gossip::builder().spawn(endpoint_a.clone());
    let _router_a = iroh::protocol::Router::builder(endpoint_a.clone())
        .accept(iroh_gossip::ALPN, gossip_a.clone())
        .spawn();

    let rp_a = RecordPublisher::new(
        topic_id.clone(),
        signing_a,
        None,
        b"secret".to_vec(),
        config.clone(),
    );

    let topic_a = gossip_a
        .subscribe_and_join_with_auto_discovery_no_wait(rp_a)
        .await
        .expect("failed to subscribe and join topic A");
    let (sender_a, mut receiver_a) = topic_a.split().await.expect("failed to split topic A");

    // Peer B
    let secret_b = iroh::SecretKey::generate(&mut rand::rng());
    let signing_b = mainline::SigningKey::from_bytes(&secret_b.to_bytes());
    let endpoint_b = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_b)
        .bind()
        .await
        .expect("failed to bind endpoint B");
    let gossip_b = iroh_gossip::net::Gossip::builder().spawn(endpoint_b.clone());
    let _router_b = iroh::protocol::Router::builder(endpoint_b.clone())
        .accept(iroh_gossip::ALPN, gossip_b.clone())
        .spawn();

    let rp_b = RecordPublisher::new(
        topic_id.clone(),
        signing_b,
        None,
        b"secret".to_vec(),
        config,
    );

    let topic_b = gossip_b
        .subscribe_and_join_with_auto_discovery(rp_b)
        .await
        .expect("failed to subscribe and join topic B");
    let (sender_b, mut receiver_b) = topic_b.split().await.expect("failed to split topic B");

    // Join peers
    sender_a
        .join_peers(vec![endpoint_b.id()], None)
        .await
        .expect("failed to join peers from sender A");
    sender_b
        .join_peers(vec![endpoint_a.id()], None)
        .await
        .expect("failed to join peers from sender B");

    receiver_a
        .joined()
        .await
        .expect("failed to wait for receiver A to join");
    receiver_b
        .joined()
        .await
        .expect("failed to wait for receiver B to join");

    let receivers: Vec<GossipReceiver> = (0..N)
        .map(|_| receiver_b.clone())
        .chain((0..N).map(|_| receiver_a.clone()))
        .collect();
    let barrier = Arc::new(Barrier::new(receivers.len() + 1));

    let handles = receivers
        .into_iter()
        .enumerate()
        .map(|(i, mut rx)| {
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;

                let mut received = Vec::new();
                while received.len() < MSG_COUNT {
                    match tokio::time::timeout(std::time::Duration::from_secs(30), rx.next()).await
                    {
                        Ok(Some(iroh_gossip::api::Event::Received(msg))) => {
                            received.push(msg.content.to_vec());
                        }
                        Ok(Some(_)) => continue,
                        other => panic!("receiver {i}: unexpected result: {other:?}"),
                    }
                }
                received
            })
        })
        .collect::<Vec<_>>();

    barrier.wait().await;

    for i in 0..MSG_COUNT {
        sender_a
            .broadcast(format!("msg-a-{i}").into_bytes())
            .await
            .expect("failed to broadcast from sender A");
        sender_b
            .broadcast(format!("msg-b-{i}").into_bytes())
            .await
            .expect("failed to broadcast from sender B");
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let received = tokio::time::timeout(std::time::Duration::from_secs(60), handle)
            .await
            .expect(&format!("receiver {i} timed out"))
            .expect(&format!("receiver {i} panicked"));

        assert_eq!(
            received.len(),
            MSG_COUNT,
            "receiver {i} got {} messages, expected {MSG_COUNT}",
            received.len(),
        );
    }
}
