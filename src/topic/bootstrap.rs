use std::{collections::HashSet, time::Duration};

use anyhow::Result;
use tokio::time::sleep;

use crate::{
    GossipSender,
    actor::{Action, Actor, Handle},
    crypto::record::Record,
    gossip::receiver::GossipReceiver,
};

#[derive(Debug, Clone)]
pub struct Bootstrap {
    api: Handle<BootstrapActor>,
}

#[derive(Debug)]
struct BootstrapActor {
    rx: tokio::sync::mpsc::Receiver<Action<Self>>,

    record_publisher: crate::crypto::record::RecordPublisher,

    gossip_sender: GossipSender,
    gossip_receiver: GossipReceiver,
}

impl Bootstrap {
    pub async fn new(
        record_publisher: crate::crypto::record::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
    ) -> Result<Self> {
        let gossip_topic: iroh_gossip::api::GossipTopic = gossip
            .subscribe(
                iroh_gossip::proto::TopicId::from(record_publisher.topic_id().hash()),
                vec![],
            )
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender, gossip.clone()),
            GossipReceiver::new(gossip_receiver, gossip.clone()),
        );

        let (api, rx) = Handle::channel(32);

        tokio::spawn(async move {
            let mut actor = BootstrapActor {
                rx,
                record_publisher,
                gossip_sender,
                gossip_receiver,
            };
            let _ = actor.run().await;
        });

        Ok(Self { api: api })
    }

    pub async fn bootstrap(&self) -> Result<tokio::sync::oneshot::Receiver<()>> {
        self.api.call(|actor| Box::pin(actor.bootstrap())).await        
    }

    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api
            .call(move |actor| Box::pin(actor.gossip_sender()))
            .await
    }

    pub async fn gossip_receiver(&self) -> Result<GossipReceiver> {
        self.api
            .call(move |actor| Box::pin(actor.gossip_receiver()))
            .await
    }
}

impl Actor for BootstrapActor {
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

impl BootstrapActor {
    pub async fn bootstrap(&mut self) -> Result<tokio::sync::oneshot::Receiver<()>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn({
            let mut last_published_unix_minute = 0;
            let (gossip_sender, gossip_receiver) =
                    (self.gossip_sender.clone(), self.gossip_receiver.clone());
            let record_publisher = self.record_publisher.clone();
            async move {
                
                loop {
                    // Check if we are connected to at least one node
                    if gossip_receiver.is_joined().await {
                        break;
                    }

                    // On the first try we check the prev unix minute, after that the current one
                    let unix_minute = crate::unix_minute(if last_published_unix_minute == 0 {
                        -1
                    } else {
                        0
                    });

                    // Unique, verified records for the unix minute
                    let records = record_publisher.get_records(unix_minute).await;

                    // If there are no records, invoke the publish_proc (the publishing procedure)
                    // continue the loop after
                    if records.is_empty() {
                        if unix_minute != last_published_unix_minute {
                            last_published_unix_minute = unix_minute;
                            tokio::spawn({
                                let record_creator = record_publisher.clone();
                                let record = Record::sign(
                                    record_publisher.topic_id().hash(),
                                    unix_minute,
                                    record_publisher.node_id().public().to_bytes(),
                                    [[0; 32]; 5],
                                    [[0; 32]; 5],
                                    &record_publisher.signing_key(),
                                );
                                async move {
                                    let _ = record_creator.publish_record(record).await;
                                }
                            });
                        }
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }

                    // We found records

                    // Collect node ids from active_peers and record.node_id (of publisher)
                    let bootstrap_nodes = records
                        .iter()
                        .flat_map(|record| {
                            let mut v = vec![record.node_id()];
                            for peer in record.active_peers() {
                                if peer != [0; 32] {
                                    v.push(peer);
                                }
                            }
                            v
                        })
                        .filter_map(|node_id| iroh::NodeId::from_bytes(&node_id).ok())
                        .collect::<HashSet<_>>();

                    // Maybe in the meantime someone connected to us via one of our published records
                    // we don't want to disrup the gossip rotations any more then we have to
                    // so we check again before joining new peers
                    /*
                    println!(
                        "checking if joined before joining peers: {}",
                        gossip_receiver.neighbors().await.len()
                    );
                    println!("bootstrap_records: {}", bootstrap_nodes.len());
                    */
                    if gossip_receiver.is_joined().await {
                        break;
                    }

                    // Instead of throwing everything into join_peers() at once we go node_id by node_id
                    // again to disrupt as little nodes peer neighborhoods as possible.
                    for node_id in bootstrap_nodes.iter() {
                        match gossip_sender.join_peers(vec![*node_id], None).await {
                            Ok(_) => {
                                sleep(Duration::from_millis(100)).await;
                                //println!("joined peer: {}", node_id);
                                if gossip_receiver.is_joined().await {
                                    break;
                                }
                            }
                            Err(_) => {
                                //println!("failed to join peers");
                                continue;
                            }
                        }
                    }

                    // If we are still not connected to anyone:
                    // give it the default iroh-gossip connection timeout before the final is_joined() check
                    if !gossip_receiver.is_joined().await {
                        sleep(Duration::from_millis(500)).await;
                    }

                    // If we are connected: return
                    if gossip_receiver.is_joined().await {
                        break;
                    } else {
                        // If we are not connected: check if we should publish a record this minute
                        if unix_minute != last_published_unix_minute {
                            last_published_unix_minute = unix_minute;
                            tokio::spawn({
                                let record_creator = record_publisher.clone();
                                let record = Record::sign(
                                    record_publisher.topic_id().hash(),
                                    unix_minute,
                                    record_publisher.node_id().public().to_bytes(),
                                    [[0; 32]; 5],
                                    [[0; 32]; 5],
                                    &record_publisher.signing_key(),
                                );
                                async move {
                                    let _ = record_creator.publish_record(record).await;
                                }
                            });
                        }
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
                let _ = sender.send(());
            }
        });

        Ok(receiver)
    }

    pub async fn gossip_sender(&mut self) -> Result<GossipSender> {
        Ok(self.gossip_sender.clone())
    }

    pub async fn gossip_receiver(&mut self) -> Result<GossipReceiver> {
        Ok(self.gossip_receiver.clone())
    }
}
