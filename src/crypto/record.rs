use anyhow::{bail, Result};
use ed25519_dalek::ed25519::signature::SignerMut;
use ed25519_dalek_hpke::{Ed25519hpkeDecryption, Ed25519hpkeEncryption};


#[derive(Debug, Clone)]
pub struct EncryptedRecord {
    encrypted_record: Vec<u8>,
    encrypted_decryption_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Record {
    topic: [u8; 32],
    unix_minute: u64,
    node_id: [u8; 32],
    active_peers: [[u8; 32]; 5],
    last_message_hashes: [[u8; 32]; 5],
    signature: [u8; 64],
}


impl EncryptedRecord {
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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let encrypted_record_len = self.encrypted_record.len() as u32;
        buf.extend_from_slice(&encrypted_record_len.to_le_bytes());
        buf.extend_from_slice(&self.encrypted_record);
        buf.extend_from_slice(&self.encrypted_decryption_key);
        buf
    }

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
    pub fn sign(
        topic: [u8; 32],
        unix_minute: u64,
        node_id: [u8; 32],
        active_peers: [[u8; 32]; 5],
        last_message_hashes: [[u8; 32]; 5],
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Self {
        let mut signature_data = Vec::new();
        signature_data.extend_from_slice(&topic);
        signature_data.extend_from_slice(&unix_minute.to_le_bytes());
        signature_data.extend_from_slice(&node_id);
        for active_peer in active_peers {
            signature_data.extend_from_slice(&active_peer);
        }
        for last_message_hash in last_message_hashes {
            signature_data.extend_from_slice(&last_message_hash);
        }
        let mut signing_key = signing_key.clone();
        let signature = signing_key.sign(&signature_data);
        Self {
            topic,
            unix_minute,
            node_id,
            active_peers,
            last_message_hashes,
            signature: signature.to_bytes(),
        }
    }

    pub fn from_bytes(buf: Vec<u8>) -> Result<Self> {
        let (topic, buf) = buf.split_at(32);
        let (unix_minute, buf) = buf.split_at(8);
        let (node_id, mut buf) = buf.split_at(32);

        let mut active_peers: [[u8; 32]; 5] = [[0; 32]; 5];
        #[allow(clippy::needless_range_loop)]
        for i in 0..active_peers.len() {
            let (active_peer, _buf) = buf.split_at(32);
            active_peers[i] = active_peer.try_into()?;
            buf = _buf;
        }
        let mut last_message_hashes: [[u8; 32]; 5] = [[0; 32]; 5];
        #[allow(clippy::needless_range_loop)]
        for i in 0..last_message_hashes.len() {
            let (last_message_hash, _buf) = buf.split_at(32);
            last_message_hashes[i] = last_message_hash.try_into()?;
            buf = _buf;
        }

        let (signature, buf) = buf.split_at(64);

        if !buf.is_empty() {
            bail!("buffer not empty after reconstruction")
        }

        Ok(Self {
            topic: topic.try_into()?,
            unix_minute: u64::from_le_bytes(unix_minute.try_into()?),
            node_id: node_id.try_into()?,
            active_peers,
            last_message_hashes,
            signature: signature.try_into()?,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.topic);
        buf.extend_from_slice(&self.unix_minute.to_le_bytes());
        buf.extend_from_slice(&self.node_id);
        for active_peer in self.active_peers {
            buf.extend_from_slice(&active_peer);
        }
        for last_message_hash in self.last_message_hashes {
            buf.extend_from_slice(&last_message_hash);
        }
        buf.extend_from_slice(&self.signature);
        buf
    }

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
        let node_id = ed25519_dalek::VerifyingKey::from_bytes(&self.node_id)?;

        node_id.verify_strict(signature_data.as_slice(), &signature)?;

        Ok(())
    }

    pub fn encrypt(&self, encryption_key: &ed25519_dalek::SigningKey) -> EncryptedRecord {
        let one_time_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
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

// fields only 
impl Record {
    pub fn topic(&self) -> [u8; 32] {
        self.topic
    }

    pub fn unix_minute(&self) -> u64 {
        self.unix_minute
    }

    pub fn node_id(&self) -> [u8; 32] {
        self.node_id
    }

    pub fn active_peers(&self) -> [[u8; 32]; 5] {
        self.active_peers
    }

    pub fn last_message_hashes(&self) -> [[u8; 32]; 5] {
        self.last_message_hashes
    }

    pub fn signature(&self) -> [u8; 64] {
        self.signature
    }
}