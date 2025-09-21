use crate::{
    actor::Actor, crypto::RecordTopic, gossip::{
        merge::{BubbleMerge, MessageOverlapMerge},
        topic::{bootstrap::Bootstrap, publisher::Publisher},
    }, GossipSender
};
use anyhow::Result;
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct TopicId {
    _raw: String,
    hash: [u8; 32], // sha512( raw )[..32]
}

impl Into<RecordTopic> for TopicId {
    fn into(self) -> RecordTopic {
        RecordTopic::from_bytes(&self.hash)
    }
}

impl TopicId {
    pub fn new(raw: String) -> Self {
        let mut raw_hash = sha2::Sha512::new();
        raw_hash.update(raw.as_bytes());

        Self {
            _raw: raw,
            hash: raw_hash.finalize()[..32]
                .try_into()
                .expect("hashing 'raw' failed"),
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    #[allow(dead_code)]
    pub fn raw(&self) -> &str {
        &self._raw
    }
}

#[derive(Debug, Clone)]
pub struct Topic {
    api: crate::actor::Handle<TopicActor>,
}

#[derive(Debug)]
struct TopicActor {
    rx: tokio::sync::mpsc::Receiver<crate::actor::Action<Self>>,
    bootstrap: Bootstrap,
    publisher: Option<Publisher>,
    bubble_merge: Option<BubbleMerge>,
    message_overlap_merge: Option<MessageOverlapMerge>,
    record_publisher: crate::crypto::RecordPublisher,
}

impl Topic {
    pub async fn new(
        record_publisher: crate::crypto::RecordPublisher,
        gossip: iroh_gossip::net::Gossip,
        async_bootstrap: bool,
    ) -> Result<Self> {
        let (api, rx) = crate::actor::Handle::channel(32);

        let bootstrap = Bootstrap::new(record_publisher.clone(), gossip.clone()).await?;

        tokio::spawn({
            let bootstrap = bootstrap.clone();
            async move {
                let mut actor = TopicActor {
                    rx,
                    bootstrap: bootstrap.clone(),
                    record_publisher,
                    publisher: None,
                    bubble_merge: None,
                    message_overlap_merge: None,
                };
                let _ = actor.run().await;
            }
        });

        let bootstrap_done = bootstrap.bootstrap().await?;
        if !async_bootstrap {
            bootstrap_done.await?;
        }

        // Spawn publisher after bootstrap
        let _ = api
            .call(move |actor| Box::pin(actor.start_publishing()))
            .await;

        let _ = api
            .call(move |actor| Box::pin(actor.start_bubble_merge()))
            .await;

        let _ = api
            .call(move |actor| Box::pin(actor.start_message_overlap_merge()))
            .await;

        Ok(Self { api })
    }

    pub async fn split(&self) -> Result<(GossipSender, crate::gossip::receiver::GossipReceiver)> {
        Ok((self.gossip_sender().await?, self.gossip_receiver().await?))
    }

    pub async fn gossip_sender(&self) -> Result<GossipSender> {
        self.api
            .call(move |actor| Box::pin(actor.gossip_sender()))
            .await
    }

    pub async fn gossip_receiver(&self) -> Result<crate::gossip::receiver::GossipReceiver> {
        self.api
            .call(move |actor| Box::pin(actor.gossip_receiver()))
            .await
    }

    pub async fn record_creator(&self) -> Result<crate::crypto::RecordPublisher> {
        self.api
            .call(move |actor| Box::pin(async move { Ok(actor.record_publisher.clone()) }))
            .await
    }
}

impl Actor for TopicActor {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(action) = self.rx.recv() => {
                    let _ = action(self).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl TopicActor {
    pub async fn gossip_receiver(&mut self) -> Result<crate::gossip::receiver::GossipReceiver> {
        self.bootstrap.gossip_receiver().await
    }

    pub async fn gossip_sender(&mut self) -> Result<GossipSender> {
        self.bootstrap.gossip_sender().await
    }

    pub async fn start_publishing(&mut self) -> Result<()> {
        let publisher =
            Publisher::new(self.record_publisher.clone(), self.gossip_receiver().await?)?;
        self.publisher = Some(publisher);
        Ok(())
    }

    pub async fn start_bubble_merge(&mut self) -> Result<()> {
        let bubble_merge = BubbleMerge::new(
            self.record_publisher.clone(),
            self.gossip_sender().await?,
            self.gossip_receiver().await?,
        )?;
        self.bubble_merge = Some(bubble_merge);
        Ok(())
    }

    pub async fn start_message_overlap_merge(&mut self) -> Result<()> {
        let message_overlap_merge = MessageOverlapMerge::new(
            self.record_publisher.clone(),
            self.gossip_sender().await?,
            self.gossip_receiver().await?,
        )?;
        self.message_overlap_merge = Some(message_overlap_merge);
        Ok(())
    }
}
