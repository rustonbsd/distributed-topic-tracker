//! Actor-based wrapper for iroh-gossip message receiving.

use std::collections::{HashSet, VecDeque};

use actor_helper::{Action, Actor, Handle, Receiver, act, act_ok};
use anyhow::Result;
use futures_lite::StreamExt;
use sha2::Digest;

/// Gossip receiver that collects incoming messages and neighbor info.
///
/// Tracks SHA512 message hashes (first 32 bytes) for overlap detection and provides
/// neighbor list for topology analysis.
#[derive(Debug, Clone)]
pub struct GossipReceiver {
    api: Handle<GossipReceiverActor, anyhow::Error>,
    _gossip: iroh_gossip::net::Gossip,
}

#[derive(Debug)]
pub struct GossipReceiverActor {
    rx: Receiver<Action<GossipReceiverActor>>,
    gossip_receiver: iroh_gossip::api::GossipReceiver,
    last_message_hashes: Vec<[u8; 32]>,
    msg_queue: VecDeque<Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>>,
    waiters: VecDeque<
        tokio::sync::oneshot::Sender<
            Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>,
        >,
    >,
    _gossip: iroh_gossip::net::Gossip,
}

impl GossipReceiver {
    /// Create a new gossip receiver from an iroh topic receiver.
    pub fn new(
        gossip_receiver: iroh_gossip::api::GossipReceiver,
        gossip: iroh_gossip::net::Gossip,
    ) -> Self {
        let (api, rx) = Handle::channel();
        tokio::spawn({
            let gossip = gossip.clone();
            async move {
                let mut actor = GossipReceiverActor {
                    rx,
                    gossip_receiver,
                    last_message_hashes: Vec::new(),
                    msg_queue: VecDeque::new(),
                    waiters: VecDeque::new(),
                    _gossip: gossip.clone(),
                };
                let _ = actor.run().await;
            }
        });

        Self {
            api,
            _gossip: gossip.clone(),
        }
    }

    /// Get the set of currently connected neighbor node IDs.
    pub async fn neighbors(&self) -> HashSet<iroh::NodeId> {
        self.api
            .call(act_ok!(actor => async move {
                actor.gossip_receiver.neighbors().collect::<HashSet<iroh::NodeId>>()
            }))
            .await
            .expect("actor stopped")
    }

    /// Check if the local node has joined the topic.
    pub async fn is_joined(&self) -> bool {
        self.api
            .call(act_ok!(actor => async move { actor.gossip_receiver.is_joined() }))
            .await
            .expect("actor stopped")
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
    pub async fn last_message_hashes(&self) -> Vec<[u8; 32]> {
        self.api
            .call(act_ok!(actor => async move { actor.last_message_hashes.clone() }))
            .await
            .expect("void")
    }
}

impl Actor<anyhow::Error> for GossipReceiverActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    action(self).await;
                }
                raw_event = self.gossip_receiver.next() => {
                    self.msg_queue.push_front(raw_event);

                    if let Some(waiter) = self.waiters.pop_back() {
                        if let Some(event) = self.msg_queue.pop_back() {
                            let _ = waiter.send(event);
                        } else {
                            let _ = waiter.send(None);
                            // this should never happen
                        }
                    }
                    if let Some(Some(Ok(event))) = self.msg_queue.front() {
                        match event {
                            iroh_gossip::api::Event::Received(msg) => {
                                let mut hash = sha2::Sha512::new();
                                hash.update(msg.content.clone());
                                if let Ok(lmh) = hash.finalize()[..32].try_into() {
                                    self.last_message_hashes.push(lmh);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
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
