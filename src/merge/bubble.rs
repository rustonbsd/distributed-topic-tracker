use std::{collections::HashSet, time::Duration};

use crate::{
    GossipReceiver, GossipSender, RecordPublisher,
    actor::{Action, Actor, Handle},
};
use anyhow::Result;

pub struct BubbleMerge {
    _api: Handle<BubbleMergeActor>,
}

struct BubbleMergeActor {
    rx: tokio::sync::mpsc::Receiver<Action<BubbleMergeActor>>,

    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    gossip_sender: GossipSender,
    ticker: tokio::time::Interval,
}

impl BubbleMerge {
    pub fn new(
        record_publisher: RecordPublisher,
        gossip_sender: GossipSender,
        gossip_receiver: GossipReceiver,
    ) -> Result<Self> {
        let (api, rx) = Handle::channel(32);

        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tokio::spawn(async move {
            let mut actor = BubbleMergeActor {
                rx,
                record_publisher,
                gossip_receiver,
                gossip_sender,
                ticker,
            };
            let _ = actor.run().await;
        });

        Ok(Self { _api: api })
    }
}

impl Actor for BubbleMergeActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
                    action(self).await;
                }
                _ = self.ticker.tick() => {
                    let _ = self.merge().await;
                    self.ticker.reset_after(Duration::from_secs(rand::random::<u64>() % 50));
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }
}

impl BubbleMergeActor {
    // Cluster size as bubble indicator
    async fn merge(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let records = self.record_publisher.get_records(unix_minute).await;
        let neighbors = self.gossip_receiver.neighbors().await;
        if neighbors.len() < 4 && !records.is_empty() {
            let node_ids = records
                .iter()
                .flat_map(|record| {
                    record
                        .active_peers()
                        .iter()
                        .filter_map(|&active_peer| {
                            if active_peer == [0; 32]
                                || neighbors.contains(&active_peer)
                                || active_peer.eq(record.node_id().to_vec().as_slice())
                                || active_peer.eq(self.record_publisher.node_id().as_bytes())
                            {
                                None
                            } else {
                                iroh::NodeId::from_bytes(&active_peer).ok()
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<HashSet<_>>();
            self.gossip_sender
                .join_peers(
                    node_ids.iter().cloned().collect::<Vec<_>>(),
                    Some(crate::MAX_JOIN_PEERS_COUNT),
                )
                .await?;
        }
        Ok(())
    }
}
