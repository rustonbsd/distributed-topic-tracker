//! Bootstrap process for discovering and joining peers via DHT.

use std::collections::HashSet;

use actor_helper::{Handle, act, act_ok};
use anyhow::Result;
use iroh::EndpointId;
use tokio::time::sleep;

use crate::{
    GossipSender, MAX_MESSAGE_HASHES, MAX_RECORD_PEERS, RecordPublisher,
    config::BootstrapConfig,
    crypto::Record,
    gossip::{GossipRecordContent, receiver::GossipReceiver},
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
    config: BootstrapConfig,
}

impl Bootstrap {
    /// Create a new bootstrap process for a topic.
    pub async fn new(
        record_publisher: crate::crypto::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
        cancel_token: tokio_util::sync::CancellationToken,
        timeout_config: crate::config::TimeoutConfig,
        bootstrap_config: BootstrapConfig,
    ) -> Result<Self> {
        let gossip_topic: iroh_gossip::api::GossipTopic = gossip
            .subscribe(
                iroh_gossip::proto::TopicId::from(record_publisher.topic_id().hash()),
                vec![],
            )
            .await?;
        let (gossip_sender, gossip_receiver) = gossip_topic.split();
        let (gossip_sender, gossip_receiver) = (
            GossipSender::new(gossip_sender, timeout_config),
            GossipReceiver::new(gossip_receiver, cancel_token.clone()),
        );

        let api = Handle::spawn(BootstrapActor {
            record_publisher,
            gossip_sender,
            gossip_receiver,
            cancel_token,
            config: bootstrap_config,
        })
        .0;

        Ok(Self { api })
    }

    /// Start the bootstrap process.
    ///
    /// Returns a receiver that signals completion when the node has joined the topic (has at least one neighbor).
    pub async fn bootstrap(&self) -> Result<tokio::sync::oneshot::Receiver<Result<()>>> {
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
    pub async fn start_bootstrap(&mut self) -> Result<tokio::sync::oneshot::Receiver<Result<()>>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn({
            let mut last_published_unix_minute = 0;
            let (gossip_sender, mut gossip_receiver) =
                (self.gossip_sender.clone(), self.gossip_receiver.clone());
            let record_publisher = self.record_publisher.clone();
            let cancel_token = self.cancel_token.clone();
            let bootstrap_config = self.config.clone();
            let mut is_joined_ret = false;

            if self.config.publish_record_on_startup() {
                let unix_minute = crate::unix_minute(0);
                tracing::debug!("Bootstrap: initial startup record publish {}", unix_minute);
                last_published_unix_minute =
                    if self.config.check_last_minute_record_first_on_startup() {
                        0
                    } else {
                        unix_minute
                    };
                let record_creator = record_publisher.clone();
                let record_content = GossipRecordContent {
                    active_peers: [[0; 32]; MAX_RECORD_PEERS],
                    last_message_hashes: [[0; 32]; MAX_MESSAGE_HASHES],
                };
                if let Ok(record) = Record::sign(
                    record_publisher.topic_id().hash(),
                    unix_minute,
                    record_content,
                    record_publisher.signing_key(),
                ) {
                    publish_record_fire_and_forget(record_creator, record, None);
                }
            }

            async move {
                tracing::debug!("Bootstrap: starting bootstrap process");
                'bootstrap: while !cancel_token.is_cancelled() {
                    // Check if we are connected to at least one node
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined
                        && is_joined
                    {
                        tracing::debug!("Bootstrap: already joined, exiting bootstrap loop");
                        is_joined_ret = true;
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // On the first try we check the prev unix minute, after that the current one
                    let mut use_cached_next = true;
                    let unix_minute = crate::unix_minute(
                        if last_published_unix_minute == 0
                            && bootstrap_config.check_last_minute_record_first_on_startup()
                        {
                            use_cached_next = false;
                            -1
                        } else {
                            0
                        },
                    );

                    // Unique, verified records for the unix minute
                    let mut records = record_publisher
                        .get_records(unix_minute - 1)
                        .await
                        .unwrap_or_default();
                    let current_records = record_publisher
                        .get_records(unix_minute)
                        .await
                        .unwrap_or_default();
                    records.extend(current_records.clone());

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
                                active_peers: [[0; 32]; MAX_RECORD_PEERS],
                                last_message_hashes: [[0; 32]; MAX_MESSAGE_HASHES],
                            };
                            if let Ok(record) = Record::sign(
                                record_publisher.topic_id().hash(),
                                unix_minute,
                                record_content,
                                record_publisher.signing_key(),
                            ) {
                                publish_record_fire_and_forget(
                                    record_creator,
                                    record,
                                    if use_cached_next {
                                        Some(current_records.clone())
                                    } else {
                                        None
                                    },
                                );
                            }
                        }
                        tokio::select! {
                            _ = sleep(bootstrap_config.no_peers_retry_interval()) => {}
                            _ = gossip_receiver.joined() => continue,
                            _ = cancel_token.cancelled() => break,
                        }
                        continue;
                    }

                    // We found records

                    // Collect node ids from active_peers and record.pub_key (of publisher)
                    let bootstrap_nodes = records
                        .iter()
                        .flat_map(|record| {
                            let mut v = vec![record.pub_key()];
                            if let Ok(record_content) = record.content::<GossipRecordContent>() {
                                for peer in record_content.active_peers {
                                    if peer != [0; 32] {
                                        v.push(peer);
                                    }
                                }
                            }
                            v
                        })
                        .filter_map(|pub_key| EndpointId::from_bytes(&pub_key).ok())
                        .collect::<HashSet<_>>();

                    tracing::debug!(
                        "Bootstrap: extracted {} potential bootstrap nodes",
                        bootstrap_nodes.len()
                    );

                    // Maybe in the meantime someone connected to us via one of our published records
                    // we don't want to disrup the gossip rotations any more then we have to
                    // so we check again before joining new peers
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined
                        && is_joined
                    {
                        tracing::debug!("Bootstrap: joined while processing records, exiting");
                        is_joined_ret = true;
                        break;
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // Instead of throwing everything into join_peers() at once we go pub_key by pub_key
                    // again to disrupt as little nodes peer neighborhoods as possible.
                    for pub_key in bootstrap_nodes.iter() {
                        match gossip_sender.join_peers(vec![*pub_key], None).await {
                            Ok(_) => {
                                tracing::debug!("Bootstrap: attempted to join peer {}", pub_key);

                                tokio::select! {
                                    _ = sleep(bootstrap_config.per_peer_join_settle_time()) => {}
                                    _ = gossip_receiver.joined() => {},
                                    _ = cancel_token.cancelled() => break 'bootstrap,
                                }
                                let is_joined = gossip_receiver.is_joined().await;
                                if let Ok(is_joined) = is_joined
                                    && is_joined
                                {
                                    tracing::debug!(
                                        "Bootstrap: successfully joined via peer {}",
                                        pub_key
                                    );
                                    is_joined_ret = true;
                                    break;
                                } else if let Err(e) = is_joined {
                                    tracing::debug!(
                                        "Bootstrap: error checking join status: {:?}",
                                        e
                                    );
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Bootstrap: failed to join peer {}: {:?}",
                                    pub_key,
                                    e
                                );
                                continue;
                            }
                        }
                    }

                    // If we are still not connected to anyone:
                    // give it the default iroh-gossip connection timeout before the final is_joined() check
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined
                        && !is_joined
                    {
                        tracing::debug!(
                            "Bootstrap: not joined yet, waiting {:?} before final check",
                            bootstrap_config.join_confirmation_wait_time()
                        );
                        tokio::select! {
                            _ = sleep(bootstrap_config.join_confirmation_wait_time()) => {}
                            _ = gossip_receiver.joined() => {},
                            _ = cancel_token.cancelled() => break,
                        }
                    } else if let Err(e) = is_joined {
                        tracing::debug!("Bootstrap: error checking join status: {:?}", e);
                        break;
                    }

                    // If we are connected: return
                    let is_joined = gossip_receiver.is_joined().await;
                    if let Ok(is_joined) = is_joined
                        && is_joined
                    {
                        tracing::debug!("Bootstrap: successfully joined after final wait");
                        is_joined_ret = true;
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
                                record_publisher.topic_id().hash(),
                                unix_minute,
                                GossipRecordContent {
                                    active_peers: [[0; 32]; MAX_RECORD_PEERS],
                                    last_message_hashes: [[0; 32]; MAX_MESSAGE_HASHES],
                                },
                                record_publisher.signing_key(),
                            ) {
                                publish_record_fire_and_forget(
                                    record_creator,
                                    record,
                                    if use_cached_next {
                                        Some(current_records)
                                    } else {
                                        None
                                    },
                                );
                            }
                        }
                        tokio::select! {
                            _ = sleep(bootstrap_config.discovery_poll_interval()) => continue,
                            _ = gossip_receiver.joined() => continue,
                            _ = cancel_token.cancelled() => break,
                        }
                    }
                }
                tracing::debug!("Bootstrap: exited");

                if is_joined_ret {
                    let _ = sender.send(Ok(()));
                } else {
                    let _ = sender.send(Err(anyhow::anyhow!(
                        "Bootstrap process failed or was cancelled"
                    )));
                }
            }
        });

        Ok(receiver)
    }
}

fn publish_record_fire_and_forget(
    record_publisher: RecordPublisher,
    record: Record,
    cached_records: Option<HashSet<Record>>,
) {
    tokio::spawn(async move {
        if let Err(err) = record_publisher
            .publish_record_cached_records(record, cached_records)
            .await
        {
            tracing::warn!("Failed to publish record: {:?}", err);
        }
    });
}
