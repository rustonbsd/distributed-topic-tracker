use std::{collections::HashSet, str::FromStr, time::Duration};

use anyhow::{Result, bail};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

use ed25519_dalek_hpke::{Ed25519hpkeDecryption, Ed25519hpkeEncryption};
use serde::{Deserialize, Serialize};
use sha2::Digest;

/// Topic identifier derived from a string via SHA512 hashing.
///
/// Used as a stable identifier for peer discovery records.
///
/// # Example
///
/// ```ignore
/// let topic = RecordTopic::from_str("chat-app-v1")?;
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RecordTopic([u8; 32]);

impl FromStr for RecordTopic {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut hasher = sha2::Sha512::new();
        hasher.update(s.as_bytes());
        let hash: [u8; 32] = hasher.finalize()[..32]
            .try_into()
            .map_err(|_| anyhow::anyhow!("hashing failed"))?;
        Ok(RecordTopic(hash))
    }
}

impl RecordTopic {
    /// Create from a pre-computed 32-byte hash.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    /// Get the raw 32-byte hash.
    pub fn hash(&self) -> [u8; 32] {
        self.0
    }
}

/// DHT record encrypted with HPKE.
///
/// Contains encrypted record data and encrypted decryption key.
/// Decryption requires the corresponding private key.
#[derive(Debug, Clone)]
pub struct EncryptedRecord {
    encrypted_record: Vec<u8>,
    encrypted_decryption_key: Vec<u8>,
}

/// A signed DHT record containing peer discovery information.
///
/// Records are timestamped, signed, and include content about active peers
/// and recent messages for bubble detection and message overlap merging.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Record {
    topic: [u8; 32],
    unix_minute: u64,
    pub_key: [u8; 32],
    content: RecordContent,
    signature: [u8; 64],
}

/// Serializable content of a DHT record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordContent(pub Vec<u8>);

impl RecordContent {
    /// Deserialize using postcard codec.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let content: GossipRecordContent = record_content.to()?;
    /// ```
    pub fn to<'a, T: Deserialize<'a>>(&'a self) -> anyhow::Result<T> {
        postcard::from_bytes::<T>(&self.0).map_err(|e| anyhow::anyhow!(e))
    }
    
    /// Serialize from an arbitrary type using postcard.
    pub fn from_arbitrary<T: Serialize>(from: &T) -> anyhow::Result<Self> {
        Ok(Self(
            postcard::to_allocvec(from).map_err(|e| anyhow::anyhow!(e))?,
        ))
    }
}

/// Publisher for creating and distributing signed DHT records.
///
/// Checks existing DHT record count before publishing to respect capacity limits.
#[derive(Debug, Clone)]
pub struct RecordPublisher {
    dht: crate::dht::Dht,

    record_topic: RecordTopic,
    pub_key: VerifyingKey,
    signing_key: SigningKey,
    secret_rotation: Option<crate::crypto::keys::RotationHandle>,
    initial_secret_hash: [u8; 32],
}

impl RecordPublisher {
    /// Create a new record publisher.
    ///
    /// # Arguments
    ///
    /// * `record_topic` - Topic identifier
    /// * `pub_key` - Ed25519 public key (verifying key)
    /// * `signing_key` - Ed25519 secret key (signing key)
    /// * `secret_rotation` - Optional custom key rotation strategy
    /// * `initial_secret` - Initial secret for key derivation
    pub fn new(
        record_topic: impl Into<RecordTopic>,
        pub_key: VerifyingKey,
        signing_key: SigningKey,
        secret_rotation: Option<crate::crypto::keys::RotationHandle>,
        initial_secret: Vec<u8>,
    ) -> Self {
        let mut initial_secret_hash = sha2::Sha512::new();
        initial_secret_hash.update(initial_secret);
        let initial_secret_hash: [u8; 32] = initial_secret_hash.finalize()[..32]
            .try_into()
            .expect("hashing failed");

        Self {
            dht: crate::dht::Dht::new(),
            record_topic: record_topic.into(),
            pub_key,
            signing_key,
            secret_rotation,
            initial_secret_hash,
        }
    }

    /// Create a new signed record with content.
    ///
    /// # Arguments
    ///
    /// * `unix_minute` - Time slot for this record
    /// * `record_content` - Serializable content
    pub fn new_record<'a>(
        &'a self,
        unix_minute: u64,
        record_content: impl Serialize + Deserialize<'a> + Sized,
    ) -> Result<Record> {
        Record::sign(
            self.record_topic.hash(),
            unix_minute,
            self.pub_key.to_bytes(),
            record_content,
            &self.signing_key,
        )
    }

    /// Get this publisher's Ed25519 verifying key.
    pub fn pub_key(&self) -> ed25519_dalek::VerifyingKey {
        self.pub_key.clone()
    }

    /// Get the record topic.
    pub fn record_topic(&self) -> RecordTopic {
        self.record_topic.clone()
    }

    /// Get the signing key.
    pub fn signing_key(&self) -> ed25519_dalek::SigningKey {
        self.signing_key.clone()
    }

    /// Get the secret rotation handle if set.
    pub fn secret_rotation(&self) -> Option<crate::crypto::keys::RotationHandle> {
        self.secret_rotation.clone()
    }

    /// Get the initial secret hash.
    pub fn initial_secret_hash(&self) -> [u8; 32] {
        self.initial_secret_hash
    }
}

impl RecordPublisher {
    /// Publish a record to the DHT if slot capacity allows.
    ///
    /// Checks existing record count for this time slot and skips publishing if
    /// `MAX_BOOTSTRAP_RECORDS` limit reached.
    pub async fn publish_record(&self, record: Record) -> Result<()> {
        let records = self
            .get_records(record.unix_minute())
            .await
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        if records.len() >= crate::MAX_BOOTSTRAP_RECORDS {
            return Ok(());
        }

        // Publish own records
        let sign_key = crate::crypto::keys::signing_keypair(self.record_topic, record.unix_minute);
        let salt = crate::crypto::keys::salt(self.record_topic, record.unix_minute);
        let encryption_key = crate::crypto::keys::encryption_keypair(
            self.record_topic.clone(),
            &self.secret_rotation.clone().unwrap_or_default(),
            self.initial_secret_hash,
            record.unix_minute,
        );
        let encrypted_record = record.encrypt(&encryption_key);

        self.dht
            .put_mutable(
                sign_key.clone(),
                sign_key.verifying_key().into(),
                Some(salt.to_vec()),
                encrypted_record.to_bytes().to_vec(),
                Some(3),
                Duration::from_secs(10),
            )
            .await?;

        Ok(())
    }

    /// Retrieve all verified records for a given time slot from the DHT.
    ///
    /// Filters out records from this publisher's own node ID.
    pub async fn get_records(&self, unix_minute: u64) -> HashSet<Record> {
        let topic_sign = crate::crypto::keys::signing_keypair(self.record_topic, unix_minute);
        let encryption_key = crate::crypto::keys::encryption_keypair(
            self.record_topic.clone(),
            &self.secret_rotation.clone().unwrap_or_default(),
            self.initial_secret_hash,
            unix_minute,
        );
        let salt = crate::crypto::keys::salt(self.record_topic, unix_minute);

        // Get records, decrypt and verify
        let records_iter = self
            .dht
            .get(
                topic_sign.verifying_key().into(),
                Some(salt.to_vec()),
                None,
                Duration::from_secs(10),
            )
            .await
            .unwrap_or_default();

        records_iter
            .iter()
            .filter_map(
                |record| match EncryptedRecord::from_bytes(record.value().to_vec()) {
                    Ok(encrypted_record) => match encrypted_record.decrypt(&encryption_key) {
                        Ok(record) => match record.verify(&self.record_topic.hash(), unix_minute) {
                            Ok(_) => match record.node_id().eq(self.pub_key.as_bytes()) {
                                true => None,
                                false => Some(record),
                            },
                            Err(_) => None,
                        },
                        Err(_) => None,
                    },
                    Err(_) => None,
                },
            )
            .collect::<HashSet<_>>()
    }
}

impl EncryptedRecord {
    /// Decrypt using an Ed25519 HPKE private key.
    pub fn decrypt(&self, decryption_key: &ed25519_dalek::SigningKey) -> Result<Record> {
        let one_time_key_bytes: [u8; 32] = decryption_key
            .decrypt(&self.encrypted_decryption_key)?
            .as_slice()
            .try_into()?;
        let one_time_key = ed25519_dalek::SigningKey::from_bytes(&one_time_key_bytes);

        let decrypted_record = one_time_key.decrypt(&self.encrypted_record)?;
        let record = Record::from_bytes(decrypted_record)?;
        Ok(record)
    }

    /// Serialize to bytes (length-prefixed format).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let encrypted_record_len = self.encrypted_record.len() as u32;
        buf.extend_from_slice(&encrypted_record_len.to_le_bytes());
        buf.extend_from_slice(&self.encrypted_record);
        buf.extend_from_slice(&self.encrypted_decryption_key);
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: Vec<u8>) -> Result<Self> {
        let (encrypted_record_len, buf) = buf.split_at(4);
        let encrypted_record_len = u32::from_le_bytes(encrypted_record_len.try_into()?);
        let (encrypted_record, encrypted_decryption_key) =
            buf.split_at(encrypted_record_len as usize);

        Ok(Self {
            encrypted_record: encrypted_record.to_vec(),
            encrypted_decryption_key: encrypted_decryption_key.to_vec(),
        })
    }
}

impl Record {
    /// Create and sign a new record.
    pub fn sign<'a>(
        topic: [u8; 32],
        unix_minute: u64,
        node_id: [u8; 32],
        record_content: impl Serialize + Deserialize<'a>,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> anyhow::Result<Self> {
        let record_content = RecordContent::from_arbitrary(&record_content)?;
        let mut signature_data = Vec::new();
        signature_data.extend_from_slice(&topic);
        signature_data.extend_from_slice(&unix_minute.to_le_bytes());
        signature_data.extend_from_slice(&node_id);
        signature_data.extend(&record_content.clone().0);
        let signing_key = signing_key.clone();
        let signature = signing_key.sign(&signature_data);
        Ok(Self {
            topic,
            unix_minute,
            pub_key: node_id,
            content: record_content,
            signature: signature.to_bytes(),
        })
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: Vec<u8>) -> Result<Self> {
        let (topic, buf) = buf.split_at(32);
        let (unix_minute, buf) = buf.split_at(8);
        let (node_id, buf) = buf.split_at(32);
        let (record_content, buf) = buf.split_at(buf.len() - 64);

        let (signature, buf) = buf.split_at(64);

        if !buf.is_empty() {
            bail!("buffer not empty after reconstruction")
        }

        Ok(Self {
            topic: topic.try_into()?,
            unix_minute: u64::from_le_bytes(unix_minute.try_into()?),
            pub_key: node_id.try_into()?,
            content: RecordContent(record_content.to_vec()),
            signature: signature.try_into()?,
        })
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.topic);
        buf.extend_from_slice(&self.unix_minute.to_le_bytes());
        buf.extend_from_slice(&self.pub_key);
        buf.extend(&self.content.0);
        buf.extend_from_slice(&self.signature);
        buf
    }

    /// Verify signature against topic and timestamp.
    pub fn verify(&self, actual_topic: &[u8; 32], actual_unix_minute: u64) -> Result<()> {
        if self.topic != *actual_topic {
            bail!("topic mismatch")
        }
        if self.unix_minute != actual_unix_minute {
            bail!("unix minute mismatch")
        }

        let record_bytes = self.to_bytes();
        let signature_data = record_bytes[..record_bytes.len() - 64].to_vec();
        let signature = ed25519_dalek::Signature::from_bytes(&self.signature);
        let node_id = ed25519_dalek::VerifyingKey::from_bytes(&self.pub_key)?;

        node_id.verify_strict(signature_data.as_slice(), &signature)?;

        Ok(())
    }

    /// Encrypt record with HPKE.
    pub fn encrypt(&self, encryption_key: &ed25519_dalek::SigningKey) -> EncryptedRecord {
        let one_time_key = ed25519_dalek::SigningKey::generate(&mut rand::rng());
        let p_key = one_time_key.verifying_key();
        let data_enc = p_key.encrypt(&self.to_bytes()).expect("encryption failed");
        let key_enc = encryption_key
            .verifying_key()
            .encrypt(&one_time_key.to_bytes())
            .expect("encryption failed");

        EncryptedRecord {
            encrypted_record: data_enc,
            encrypted_decryption_key: key_enc,
        }
    }
}

// Field accessors
impl Record {
    /// Get the topic hash.
    pub fn topic(&self) -> [u8; 32] {
        self.topic
    }

    /// Get the Unix minute timestamp.
    pub fn unix_minute(&self) -> u64 {
        self.unix_minute
    }

    /// Get the node ID (publisher's public key).
    pub fn node_id(&self) -> [u8; 32] {
        self.pub_key
    }

    /// Deserialize the record content.
    pub fn content<'a, T: Deserialize<'a>>(&'a self) -> anyhow::Result<T> {
        self.content.to::<T>()
    }

    /// Get the raw signature bytes.
    pub fn signature(&self) -> [u8; 64] {
        self.signature
    }
}
