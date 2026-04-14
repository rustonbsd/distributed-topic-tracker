//! Bootstrap process for discovering and joining peers via DHT.

use std::{collections::HashSet, time::Duration};

use actor_helper::{Handle, act, act_ok};
use anyhow::Result;
use iroh::EndpointId;
use tokio::time::sleep;

use crate::{
    GossipSender, crypto::Record, gossip::{GossipRecordContent, receiver::GossipReceiver}
};

/// Manages the peer discovery and joining process.
///
/// Queries DHT for bootstrap records, extracts node IDs, and progressively
/// joins peers until the local node is connected to the topic.
#[derive(Debug, Clone)]
pub struct Bootstrap {
    api: Handle<BootstrapActor, anyhow::Error>,
}

#[derive(Debug)]
struct BootstrapActor {
    record_publisher: crate::crypto::RecordPublisher,
    gossip_sender: GossipSender,
    gossip_receiver: GossipReceiver,
    cancel_token: tokio_util::sync::CancellationToken,
}

impl Bootstrap {
    /// Create a new bootstrap process for a topic.
    pub async fn new(
        record_publisher: crate::crypto::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<Self> {
        let gossip_topic: iroh_gossip::api::GossipTopic = gossip
            .subscribe(
                iroh_gossip::proto::TopicId::from(record_publisher.record_topic().hash()),
                vec![],
            )
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender),
            GossipReceiver::new(gossip_receiver, cancel_token.clone()),
        );

        let api = Handle::spawn(BootstrapActor {
            record_publisher,
            gossip_sender,
            gossip_receiver,
            cancel_token,
        })
        .0;

        Ok(Self { api })
    }

    /// Start the bootstrap process.
    ///
    /// Returns a receiver that signals completion when the node has joined the topic (has at least one neighbor).
    pub async fn bootstrap(&self) -> Result<tokio::sync::oneshot::Receiver<()>> {
        self.api.call(act!(actor=> actor.start_bootstrap())).await
    }

    /// Get the gossip sender for this topic.
    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api
            .call(act_ok!(actor => async move { actor.gossip_sender.clone() }))
            .await
    }

    /// Get the gossip receiver for this topic.
    pub async fn gossip_receiver(&self) -> Result<GossipReceiver> {
        self.api
            .call(act_ok!(actor => async move { actor.gossip_receiver.clone() }))
            .await
    }
}

impl BootstrapActor {
    pub async fn start_bootstrap(&mut self) -> Result<tokio::sync::oneshot::Receiver<()>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn({
            let mut last_published_unix_minute = 0;
            let (gossip_sender, gossip_receiver) =
                (self.gossip_sender.clone(), self.gossip_receiver.clone());
            let record_publisher = self.record_publisher.clone();
            let cancel_token = self.cancel_token.clone();
            async move {
                tracing::debug!("Bootstrap: starting bootstrap process");
                while !cancel_token.is_cancelled() {
                    // Check if we are connected to at least one node
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined && is_joined {
                        tracing::debug!("Bootstrap: already joined, exiting bootstrap loop");
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // On the first try we check the prev unix minute, after that the current one
                    let unix_minute = crate::unix_minute(if last_published_unix_minute == 0 {
                        -1
                    } else {
                        0
                    });

                    // Unique, verified records for the unix minute
                    let mut records = record_publisher.get_records(unix_minute - 1).await;
                    records.extend(record_publisher.get_records(unix_minute).await);

                    tracing::debug!(
                        "Bootstrap: fetched {} records for unix_minute {}",
                        records.len(),
                        unix_minute
                    );

                    // If there are no records, invoke the publish_proc (the publishing procedure)
                    // continue the loop after
                    if records.is_empty() {
                        if unix_minute != last_published_unix_minute {
                            tracing::debug!(
                                "Bootstrap: no records found, publishing own record for unix_minute {}",
                                unix_minute
                            );
                            last_published_unix_minute = unix_minute;
                            let record_creator = record_publisher.clone();
                            let record_content = GossipRecordContent {
                                active_peers: [[0; 32]; 5],
                                last_message_hashes: [[0; 32]; 5],
                            };
                            if let Ok(record) = Record::sign(
                                record_publisher.record_topic().hash(),
                                unix_minute,
                                record_publisher.pub_key().to_bytes(),
                                record_content,
                                &record_publisher.signing_key(),
                            ) {
                                tokio::spawn(async move {
                                    let _ = record_creator.publish_record(record).await;
                                });
                            }
                        }
                        tokio::select! {
                            _ = sleep(Duration::from_millis(100)) => {}
                            _ = cancel_token.cancelled() => break,
                        }
                        continue;
                    }

                    // We found records

                    // Collect node ids from active_peers and record.node_id (of publisher)
                    let bootstrap_nodes = records
                        .iter()
                        .flat_map(|record| {
                            let mut v = vec![record.node_id()];
                            if let Ok(record_content) = record.content::<GossipRecordContent>() {
                                for peer in record_content.active_peers {
                                    if peer != [0; 32] {
                                        v.push(peer);
                                    }
                                }
                            }
                            v
                        })
                        .filter_map(|node_id| EndpointId::from_bytes(&node_id).ok())
                        .collect::<HashSet<_>>();

                    tracing::debug!(
                        "Bootstrap: extracted {} potential bootstrap nodes",
                        bootstrap_nodes.len()
                    );

                    // Maybe in the meantime someone connected to us via one of our published records
                    // we don't want to disrup the gossip rotations any more then we have to
                    // so we check again before joining new peers
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined && is_joined {
                        tracing::debug!("Bootstrap: joined while processing records, exiting");
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // Instead of throwing everything into join_peers() at once we go node_id by node_id
                    // again to disrupt as little nodes peer neighborhoods as possible.
                    for node_id in bootstrap_nodes.iter() {
                        match gossip_sender.join_peers(vec![*node_id], None).await {
                            Ok(_) => {
                                tracing::debug!("Bootstrap: attempted to join peer {}", node_id);
                                
                                tokio::select! {
                                    _ = sleep(Duration::from_millis(100)) => {}
                                    _ = cancel_token.cancelled() => break,
                                }
                                let is_joined = gossip_receiver.is_joined().await;
                                if let Ok(is_joined) = is_joined && is_joined {
                                    tracing::debug!("Bootstrap: successfully joined via peer {}", node_id);
                                    break;
                                } else if let Err(e) = is_joined {
                                    tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Bootstrap: failed to join peer {}: {:?}",
                                    node_id,
                                    e
                                );
                                continue;
                            }
                        }
                    }

                    // If we are still not connected to anyone:
                    // give it the default iroh-gossip connection timeout before the final is_joined() check
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined && is_joined {
                        tracing::debug!(
                            "Bootstrap: not joined yet, waiting 500ms before final check"
                        );
                        tokio::select! {
                            _ = sleep(Duration::from_millis(500)) => {}
                            _ = cancel_token.cancelled() => break,
                        }
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // If we are connected: return
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined && is_joined {
                        tracing::debug!("Bootstrap: successfully joined after final wait");
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    } else {
                        tracing::debug!("Bootstrap: still not joined after attempting all peers");
                        // If we are not connected: check if we should publish a record this minute
                        if unix_minute != last_published_unix_minute {
                            tracing::debug!(
                                "Bootstrap: publishing fallback record for unix_minute {}",
                                unix_minute
                            );
                            last_published_unix_minute = unix_minute;
                            let record_creator = record_publisher.clone();
                            if let Ok(record) = Record::sign(
                                record_publisher.record_topic().hash(),
                                unix_minute,
                                record_publisher.pub_key().to_bytes(),
                                GossipRecordContent {
                                    active_peers: [[0; 32]; 5],
                                    last_message_hashes: [[0; 32]; 5],
                                },
                                &record_publisher.signing_key(),
                            ) {
                                tokio::spawn(async move {
                                    let _ = record_creator.publish_record(record).await;
                                });
                            }
                        }
                        tokio::select! {
                            _ = sleep(Duration::from_millis(100)) => {}
                            _ = cancel_token.cancelled() => break,
                        }
                        continue;
                    }
                }
                tracing::debug!("Bootstrap: exited");
                let _ = sender.send(());
            }
        });

        Ok(receiver)
    }
}
