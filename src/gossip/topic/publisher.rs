use std::time::Duration;

use crate::{
    GossipReceiver, RecordPublisher,
    actor::{Action, Actor, Handle},
};
use anyhow::Result;

#[derive(Debug,Clone)]
pub struct Publisher {
    _api: Handle<PublisherActor>,
}

#[derive(Debug)]
struct PublisherActor {
    rx: tokio::sync::mpsc::Receiver<Action<PublisherActor>>,

    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    ticker: tokio::time::Interval,
}

impl Publisher {
    pub fn new(record_publisher: RecordPublisher, gossip_receiver: GossipReceiver) -> Result<Self> {
        let (api, rx) = Handle::channel(32);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(10));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut actor = PublisherActor {
                rx,
                record_publisher,
                gossip_receiver,
                ticker,
            };
            let _ = actor.run().await;
        });

        Ok(Self { _api: api })
    }
}

impl Actor for PublisherActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
                    action(self).await;
                }
                _ = self.ticker.tick() => {
                    let _ = self.publish().await;
                    self.ticker.reset_after(Duration::from_secs(rand::random::<u64>() % 50));
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }
}

impl PublisherActor {
    async fn publish(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);
        let record = self.record_publisher.new_record(
            unix_minute,
            self.gossip_receiver
                .neighbors()
                .await
                .iter()
                .map(|n| n.public())
                .collect(),
            self.gossip_receiver.last_message_hashes().await,
        );
        self.record_publisher.publish_record(record).await
    }
}
