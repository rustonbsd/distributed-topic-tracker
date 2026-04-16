//! Actor-based wrapper for iroh-gossip message receiving.

use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

use actor_helper::{Action, Handle, Receiver, act_ok};
use anyhow::Result;
use futures_lite::StreamExt;
use iroh::EndpointId;
use sha2::Digest;

use crate::{MAX_MESSAGE_HASHES, Topic};

/// Gossip receiver that collects incoming messages and neighbor info.
///
/// Tracks SHA512 message hashes (first 32 bytes) for overlap detection and provides
/// neighbor list for topology analysis.
#[derive(Debug)]
pub struct GossipReceiver {
    api: Handle<GossipReceiverActor, anyhow::Error>,
    pub(crate) _topic_keep_alive: Option<Arc<Topic>>,
    _next_channel_sender: tokio::sync::broadcast::Sender<
        Option<iroh_gossip::api::Event>>,
    next_channel_receiver: tokio::sync::broadcast::Receiver<
        Option<iroh_gossip::api::Event>,
    >,
    _join_channel_sender: tokio::sync::broadcast::Sender<Option<()>>,
    join_channel_receiver: tokio::sync::broadcast::Receiver<Option<()>>,
}

impl Clone for GossipReceiver {
    fn clone(&self) -> Self {
        Self {
            api: self.api.clone(),
            _topic_keep_alive: self._topic_keep_alive.clone(),
            _next_channel_sender: self._next_channel_sender.clone(),
            next_channel_receiver: self._next_channel_sender.subscribe(),
            _join_channel_sender: self._join_channel_sender.clone(),
            join_channel_receiver: self._join_channel_sender.subscribe(),
        }
    }
}

#[derive(Debug)]
pub struct GossipReceiverActor {
    gossip_receiver: iroh_gossip::api::GossipReceiver,
    last_message_hashes: VecDeque<[u8; 32]>,
    cancel_token: tokio_util::sync::CancellationToken,
    next_channel_sender: tokio::sync::broadcast::Sender<
        Option<iroh_gossip::api::Event>,
    >,
    join_channel_sender: tokio::sync::broadcast::Sender<Option<()>>,
}

impl GossipReceiver {
    /// Create a new gossip receiver from an iroh topic receiver.
    pub fn new(
        gossip_receiver: iroh_gossip::api::GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        let (next_tx, next_rx) = tokio::sync::broadcast::channel(64);
        let (join_tx, join_rx) = tokio::sync::broadcast::channel(64);
        let api = Handle::spawn_with(
            GossipReceiverActor {
                gossip_receiver,
                last_message_hashes: VecDeque::with_capacity(MAX_MESSAGE_HASHES),
                cancel_token,
                next_channel_sender: next_tx.clone(),
                join_channel_sender: join_tx.clone(),
            },
            |mut actor, rx| async move { actor.run(rx).await },
        )
        .0;

        Self {
            api,
            _topic_keep_alive: None,
            next_channel_receiver: next_rx,
            _next_channel_sender: next_tx,
            join_channel_receiver: join_rx,
            _join_channel_sender: join_tx,
        }
    }

    /// Get the set of currently connected neighbor node IDs.
    pub async fn neighbors(&self) -> Result<HashSet<EndpointId>> {
        self.api
            .call(act_ok!(actor => async move {
                actor.gossip_receiver.neighbors().collect::<HashSet<EndpointId>>()
            }))
            .await
    }

    /// Check if the local node has joined the topic.
    pub async fn is_joined(&self) -> Result<bool> {
        self.api
            .call(act_ok!(actor => async move { actor.gossip_receiver.is_joined() }))
            .await
    }

    /// Receive the next gossip event.
    ///
    /// Returns `None` if the receiver is closed.
    pub async fn next(
        &mut self,
    ) -> Option<iroh_gossip::api::Event> {
        match self.next_channel_receiver.recv().await {
            Ok(event) => event,
            Err(err) => match err {
                tokio::sync::broadcast::error::RecvError::Closed => None,
                tokio::sync::broadcast::error::RecvError::Lagged(skipped) => {
                    tracing::warn!("GossipReceiver: event stream lagged, {skipped} events may have been missed");
                    Box::pin(self.next()).await
                }
            },
        }
    }

    pub async fn joined(&mut self) -> Option<()> {
        if self.is_joined().await.unwrap_or(false) {
            return Some(());
        }
        match self.join_channel_receiver.recv().await {
            Ok(event) => event,
            Err(err) => match err {
                tokio::sync::broadcast::error::RecvError::Closed => None,
                tokio::sync::broadcast::error::RecvError::Lagged(skipped) => {
                    tracing::warn!("GossipReceiver: join event stream lagged, {skipped} events may have been missed");
                    Box::pin(self.joined()).await
                }
            },
        }
    }

    /// Get SHA512 hashes (first 32 bytes) of recently received messages.
    ///
    /// Used for detecting message overlap during network partition recovery.
    pub async fn last_message_hashes(&self) -> Result<VecDeque<[u8; 32]>> {
        self.api
            .call(act_ok!(actor => async move { actor.last_message_hashes.clone() }))
            .await
    }
}

impl GossipReceiverActor {
    async fn run(&mut self, rx: Receiver<Action<GossipReceiverActor>>) -> Result<()> {
        tracing::debug!("GossipReceiver: starting gossip receiver actor");
        loop {
            tokio::select! {
                result = rx.recv_async() => {
                    match result {
                        Ok(action) => action(self).await,
                        Err(_) => break Ok(()),
                    }
                }
                raw_event = self.gossip_receiver.next() => {
                    let event = match raw_event {
                        None => {
                            tracing::debug!("GossipReceiver: gossip receiver closed");
                            self.join_channel_sender.send(None).ok();
                            self.next_channel_sender.send(None).ok();
                            break Ok(());
                        }
                        Some(Err(err)) => {
                            tracing::warn!("GossipReceiver: error receiving gossip event: {err}");
                            self.next_channel_sender.send(None).ok();
                            self.join_channel_sender.send(None).ok();
                            break Ok(());
                        }
                        Some(Ok(ref event)) => {
                            match event {
                                iroh_gossip::api::Event::Received(msg) => {
                                    tracing::debug!("GossipReceiver: received message from {:?}", msg.delivered_from);
                                    let mut hash = sha2::Sha512::new();
                                    hash.update(&msg.content);
                                    if let Ok(lmh) = hash.finalize()[..32].try_into() {
                                        if self.last_message_hashes.len() == MAX_MESSAGE_HASHES {
                                            self.last_message_hashes.pop_front();
                                        }
                                        self.last_message_hashes.push_back(lmh);
                                    }
                                    self.join_channel_sender.send(Some(())).ok();
                                }
                                iroh_gossip::api::Event::NeighborUp(node_id) => {
                                    tracing::debug!("GossipReceiver: neighbor UP: {}", node_id);
                                    self.join_channel_sender.send(Some(())).ok();
                                }
                                iroh_gossip::api::Event::NeighborDown(node_id) => {
                                    tracing::debug!("GossipReceiver: neighbor DOWN: {}", node_id);
                                }
                                iroh_gossip::api::Event::Lagged => {
                                    tracing::debug!("GossipReceiver: event stream lagged");
                                }
                            };
                            event.clone()
                        }
                    };

                    self.next_channel_sender.send(Some(event.clone())).ok();
                }
                _ = self.cancel_token.cancelled() => {
                    self.join_channel_sender.send(None).ok();
                    self.next_channel_sender.send(None).ok();
                    break Ok(());
                }
                else => break Ok(()),
            }
        }
    }
}