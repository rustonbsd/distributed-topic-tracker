//! Actor-based wrapper for iroh-gossip message receiving.

use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

use actor_helper::{Action, Handle, Receiver, act, act_ok};
use anyhow::Result;
use futures_lite::StreamExt;
use iroh::EndpointId;
use sha2::Digest;

use crate::Topic;

/// Gossip receiver that collects incoming messages and neighbor info.
///
/// Tracks SHA512 message hashes (first 32 bytes) for overlap detection and provides
/// neighbor list for topology analysis.
#[derive(Debug, Clone)]
pub struct GossipReceiver {
    api: Handle<GossipReceiverActor, anyhow::Error>,
    pub(crate) _topic_keep_alive: Option<Arc<Topic>>,
}

#[derive(Debug)]
pub struct GossipReceiverActor {
    gossip_receiver: iroh_gossip::api::GossipReceiver,
    last_message_hashes: VecDeque<[u8; 32]>,
    msg_queue: VecDeque<Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>>,
    waiters: VecDeque<
        tokio::sync::oneshot::Sender<
            Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>,
        >,
    >,
    cancel_token: tokio_util::sync::CancellationToken,
}

impl GossipReceiver {
    /// Create a new gossip receiver from an iroh topic receiver.
    pub fn new(
        gossip_receiver: iroh_gossip::api::GossipReceiver,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        let api = Handle::spawn_with(
            GossipReceiverActor {
                gossip_receiver,
                last_message_hashes: VecDeque::with_capacity(5),
                msg_queue: VecDeque::new(),
                waiters: VecDeque::new(),
                cancel_token,
            },
            |mut actor, rx| async move { actor.run(rx).await },
        )
        .0;

        Self {
            api,
            _topic_keep_alive: None,
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
        &self,
    ) -> Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.api
            .call(act!(actor => actor.register_next(tx)))
            .await
            .ok()?;
        rx.await.ok()?
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
                    match raw_event {
                        None => {
                            // if none => receiver closed
                            while let Some(waiter) = self.waiters.pop_back() {
                                let _ = waiter.send(None);
                            }
                            break Ok(());
                        }
                        Some(Err(_)) => {
                            // if Err => stream error, treat as closed
                            if let Some(waiter) = self.waiters.pop_back() {
                                let _ = waiter.send(raw_event);
                            }
                            while let Some(waiter) = self.waiters.pop_back() {
                                let _ = waiter.send(None);
                            }
                            break Ok(());
                        }
                        Some(Ok(ref event)) => {
                            match event {
                                iroh_gossip::api::Event::Received(msg) => {
                                    tracing::debug!("GossipReceiver: received message from {:?}", msg.delivered_from);
                                    let mut hash = sha2::Sha512::new();
                                    hash.update(&msg.content);
                                    if let Ok(lmh) = hash.finalize()[..32].try_into() {
                                        if self.last_message_hashes.len() == 5 {
                                            self.last_message_hashes.pop_front();
                                        }
                                        self.last_message_hashes.push_back(lmh);
                                    }
                                }
                                iroh_gossip::api::Event::NeighborUp(node_id) => {
                                    tracing::debug!("GossipReceiver: neighbor UP: {}", node_id);
                                }
                                iroh_gossip::api::Event::NeighborDown(node_id) => {
                                    tracing::debug!("GossipReceiver: neighbor DOWN: {}", node_id);
                                }
                                iroh_gossip::api::Event::Lagged => {
                                    tracing::debug!("GossipReceiver: event stream lagged");
                                }
                            }
                        }
                    };
                    self.msg_queue.push_front(raw_event);

                    if let Some(waiter) = self.waiters.pop_back() {
                        if let Some(event) = self.msg_queue.pop_back() {
                            let _ = waiter.send(event);
                        } else {
                            let _ = waiter.send(None);
                            // this should never happen
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    while let Some(waiter) = self.waiters.pop_back() {
                        let _ = waiter.send(None);
                    }
                    break Ok(());
                }
                else => break Ok(()),
            }
        }
    }
}

impl GossipReceiverActor {
    pub async fn register_next(
        &mut self,
        waiter: tokio::sync::oneshot::Sender<
            Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>,
        >,
    ) -> Result<()> {
        if let Some(event) = self.msg_queue.pop_back() {
            let _ = waiter.send(event);
        } else {
            self.waiters.push_front(waiter);
        }
        Ok(())
    }
}
