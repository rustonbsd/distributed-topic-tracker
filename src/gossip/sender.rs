//! Actor-based wrapper for iroh-gossip broadcast operations.

use std::sync::Arc;

use actor_helper::{Handle, act};
use anyhow::Result;
use iroh::EndpointId;
use rand::seq::SliceRandom;

use crate::{TimeoutConfig, Topic};

/// Gossip sender that broadcasts messages to peers.
///
/// Provides methods for broadcasting to all peers or just direct neighbors,
/// with peer joining capabilities for topology management.
#[derive(Debug, Clone)]
pub struct GossipSender {
    api: Handle<GossipSenderActor, anyhow::Error>,
    pub(crate) _topic_keep_alive: Option<Arc<Topic>>,
    pub(crate) timeout_config: TimeoutConfig,
}

/// Internal actor for gossip send operations.
#[derive(Debug)]
pub struct GossipSenderActor {
    gossip_sender: iroh_gossip::api::GossipSender,
}

impl GossipSender {
    /// Create a new gossip sender from an iroh topic sender.
    pub fn new(gossip_sender: iroh_gossip::api::GossipSender, timeout_config: TimeoutConfig) -> Self {
        let api = Handle::spawn(GossipSenderActor { gossip_sender }).0;

        Self {
            api,
            _topic_keep_alive: None,
            timeout_config,
        }
    }

    /// Broadcast a message to all peers in the topic.
    pub async fn broadcast(&self, data: Vec<u8>) -> Result<()> {
        tracing::debug!("GossipSender: broadcasting message ({} bytes)", data.len());
        let broadcast_timeout = self.timeout_config.broadcast_timeout();
        self.api
            .call(act!(actor => async move {
                tokio::time::timeout(
                    broadcast_timeout,
                    actor.gossip_sender.broadcast(data.into())
                ).await
                .map_err(|_| anyhow::anyhow!("broadcast timed out"))?
                .map_err(|e| anyhow::anyhow!(e))
            }))
            .await
    }

    /// Broadcast a message only to direct neighbors.
    pub async fn broadcast_neighbors(&self, data: Vec<u8>) -> Result<()> {
        tracing::debug!(
            "GossipSender: broadcasting to neighbors ({} bytes)",
            data.len()
        );
        let broadcast_neighbors_timeout = self.timeout_config.broadcast_neighbors_timeout();
        self.api
            .call(act!(actor => async move {
                tokio::time::timeout(
                    broadcast_neighbors_timeout,
                    actor.gossip_sender.broadcast_neighbors(data.into())
                ).await
                .map_err(|_| anyhow::anyhow!("broadcast_neighbors timed out"))?
                .map_err(|e| anyhow::anyhow!(e))
            }))
            .await
    }

    /// Join specific peer nodes.
    ///
    /// # Arguments
    ///
    /// * `peers` - List of node IDs to join
    /// * `max_peers` - Optional maximum number of peers to join (randomly selected if exceeded)
    pub async fn join_peers(&self, peers: Vec<EndpointId>, max_peers: Option<usize>) -> Result<()> {
        let mut peers = peers;
        if let Some(max_peers) = max_peers {
            peers.shuffle(&mut rand::rng());
            peers.truncate(max_peers);
        }

        tracing::debug!("GossipSender: joining {} peers", peers.len());

        let join_peers_timeout = self.timeout_config.join_peer_timeout();
        self.api
            .call(act!(actor => async move {
                tokio::time::timeout(
                    join_peers_timeout,
                    actor.gossip_sender.join_peers(peers)
                ).await
                .map_err(|_| anyhow::anyhow!("join_peers timed out"))?
                .map_err(|e| anyhow::anyhow!(e))
            }))
            .await
    }
}
