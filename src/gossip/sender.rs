use crate::{
    actor::{Action, Actor, Handle}
};
use anyhow::Result;
use rand::seq::SliceRandom;

#[derive(Debug, Clone)]
pub struct GossipSender {
    api: Handle<GossipSenderActor>,
    _gossip: iroh_gossip::net::Gossip,
}

#[derive(Debug)]
pub struct GossipSenderActor {
    rx: tokio::sync::mpsc::Receiver<Action<GossipSenderActor>>,
    gossip_sender: iroh_gossip::api::GossipSender,
    _gossip: iroh_gossip::net::Gossip,
}

impl Actor for GossipSenderActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
                    action(self).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl GossipSenderActor {
    async fn broadcast(&mut self, data: Vec<u8>) -> Result<()> {
        self.gossip_sender
            .broadcast(data.into())
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn join_peers(&mut self, peers: Vec<iroh::NodeId>) -> Result<()> {
        self.gossip_sender
            .join_peers(peers)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}

impl GossipSender {
    pub fn new(
        gossip_sender: iroh_gossip::api::GossipSender,
        gossip: iroh_gossip::net::Gossip,
    ) -> Self {
        let (api, rx) = Handle::channel(1024);
        tokio::spawn({
            let gossip = gossip.clone();
            async move {
                let mut actor = GossipSenderActor {
                    rx,
                    gossip_sender,
                    _gossip: gossip.clone(),
                };
                let _ = actor.run().await;
            }
        });

        Self {
            api,
            _gossip: gossip,
        }
    }

    pub async fn broadcast(&self, data: Vec<u8>) -> Result<()> {
        self.api
            .call(move |actor| Box::pin(actor.broadcast(data)))
            .await
    }

    pub async fn join_peers(
        &self,
        peers: Vec<iroh::NodeId>,
        max_peers: Option<usize>,
    ) -> Result<()> {
        let mut peers = peers;
        if let Some(max_peers) = max_peers {
            peers.shuffle(&mut rand::thread_rng());
            peers.truncate(max_peers);
        }

        self.api
            .call(move |actor| Box::pin(actor.join_peers(peers)))
            .await
    }
}
