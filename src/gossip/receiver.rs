use std::collections::{HashSet, VecDeque};

use crate::actor::{Action, Actor, Handle};
use anyhow::Result;
use futures::StreamExt;
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct GossipReceiver {
    api: Handle<GossipReceiverActor>,
    _gossip: iroh_gossip::net::Gossip,
}

#[derive(Debug)]
pub struct GossipReceiverActor {
    rx: tokio::sync::mpsc::Receiver<Action<GossipReceiverActor>>,
    gossip_receiver: iroh_gossip::api::GossipReceiver,
    last_message_hashes: Vec<[u8; 32]>,
    msg_queue: VecDeque<Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>>,
    waiters: VecDeque<tokio::sync::oneshot::Sender<Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>>>,
    _gossip: iroh_gossip::net::Gossip,
}

impl Actor for GossipReceiverActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
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
    pub fn neighbors(&mut self) -> HashSet<iroh::NodeId> {
        self.gossip_receiver
            .neighbors()
            .collect::<HashSet<iroh::NodeId>>()
    }

    pub fn is_joined(&mut self) -> bool {
        self.gossip_receiver.is_joined()
    }

    pub async fn register_next(
        &mut self,
        waiter: tokio::sync::oneshot::Sender<Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>>>,
    ) -> Result<()> {
        if let Some(event) = self.msg_queue.pop_back() {
            let _ = waiter.send(event);
        } else {
            self.waiters.push_front(waiter);
        }
        Ok(())
    }

    pub fn last_message_hashes(&self) -> Vec<[u8; 32]> {
        self.last_message_hashes.clone()
    }
}

impl GossipReceiver {
    pub fn new(
        gossip_receiver: iroh_gossip::api::GossipReceiver,
        gossip: iroh_gossip::net::Gossip,
    ) -> Self {
        let (api, rx) = Handle::channel(1024);
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

    pub async fn neighbors(&self) -> HashSet<iroh::NodeId> {
        self.api
            .call(move |actor| Box::pin(async move { Ok(actor.neighbors()) }))
            .await
            .expect("actor stopped running: neighbors call failed")
    }

    pub async fn is_joined(&self) -> bool {
        self.api
            .call(move |actor| Box::pin(async move { Ok(actor.is_joined()) }))
            .await
            .expect("actor stopped running: is_joined call failed")
    }

    pub async fn next(&self) -> Option<Result<iroh_gossip::api::Event, iroh_gossip::api::ApiError>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Enqueue request
        self.api
            .call(move |actor| {
                Box::pin(async move {
                    actor.register_next(tx).await?;
                    Ok(())
                })
            })
            .await.ok()?;
        // Await delivery
        rx.await.ok()?
    }

    pub async fn last_message_hashes(&self) -> Vec<[u8; 32]> {
        self.api
            .call(move |actor| Box::pin(async move { Ok(actor.last_message_hashes()) }))
            .await
            .expect("actor stopped running: last_message_hashes call failed")
    }
}
