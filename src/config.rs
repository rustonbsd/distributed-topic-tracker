use std::{num::NonZeroU32, time::Duration};

/// Timeout settings for gossip operations.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    join_peer_timeout: Duration,
    broadcast_timeout: Duration,
    broadcast_neighbors_timeout: Duration,
}

impl TimeoutConfig {
    /// Create a new `TimeoutConfigBuilder` with default values.
    pub fn builder() -> TimeoutConfigBuilder {
        TimeoutConfigBuilder {
            timeouts: TimeoutConfig::default(),
        }
    }

    /// How long to wait when joining a peer.
    ///
    /// Default: 5s.
    pub fn join_peer_timeout(&self) -> Duration {
        self.join_peer_timeout
    }

    /// How long to wait when broadcasting messages.
    ///
    /// Default: 5s.
    pub fn broadcast_timeout(&self) -> Duration {
        self.broadcast_timeout
    }

    /// How long to wait when broadcasting to neighbors.
    ///
    /// Default: 5s.
    pub fn broadcast_neighbors_timeout(&self) -> Duration {
        self.broadcast_neighbors_timeout
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            join_peer_timeout: Duration::from_secs(5),
            broadcast_timeout: Duration::from_secs(5),
            broadcast_neighbors_timeout: Duration::from_secs(5),
        }
    }
}

/// Builder for `TimeoutConfig`.
#[derive(Debug)]
pub struct TimeoutConfigBuilder {
    timeouts: TimeoutConfig,
}

impl TimeoutConfigBuilder {
    /// How long to wait when joining a peer.
    ///
    /// Default: 5s.
    pub fn join_peer_timeout(mut self, timeout: Duration) -> Self {
        self.timeouts.join_peer_timeout = timeout;
        self
    }

    /// How long to wait when broadcasting messages.
    ///
    /// Default: 5s.
    pub fn broadcast_timeout(mut self, timeout: Duration) -> Self {
        self.timeouts.broadcast_timeout = timeout;
        self
    }

    /// How long to wait when broadcasting to neighbors.
    ///
    /// Default: 5s.
    pub fn broadcast_neighbors_timeout(mut self, timeout: Duration) -> Self {
        self.timeouts.broadcast_neighbors_timeout = timeout;
        self
    }

    /// Build the `TimeoutConfig`.
    pub fn build(self) -> TimeoutConfig {
        self.timeouts
    }
}

/// DHT operation settings including retry logic and timeouts.
#[derive(Debug, Clone)]
pub struct DhtConfig {
    retries: usize,
    base_retry_interval: Duration,
    max_retry_jitter: Duration,
    put_timeout: Duration,
    get_timeout: Duration,
}

/// Builder for `DhtConfig`.
#[derive(Debug, Clone)]
pub struct DhtConfigBuilder {
    config: DhtConfig,
}

impl DhtConfigBuilder {
    /// Number of retries after the initial attempt.
    ///
    /// Total attempts = 1 + retries.
    ///
    /// Default: 3.
    pub fn retries(mut self, retries: usize) -> Self {
        self.config.retries = retries;
        self
    }

    /// Base delay between retries. No-op if `interval` is `Duration::ZERO`.
    ///
    /// If `base_retry_interval` is called only once with `Duration::ZERO`, default value prevails.
    /// If `base_retry_interval` is first called with a > `Duration::ZERO`, and then again with `Duration::ZERO`, the first set value is kept.
    ///
    /// Default: 5s.
    pub fn base_retry_interval(mut self, interval: Duration) -> Self {
        if interval > Duration::ZERO {
            self.config.base_retry_interval = interval;
        }
        self
    }

    /// Max random jitter added to retry interval.
    ///
    /// Default: 10s.
    pub fn max_retry_jitter(mut self, jitter: Duration) -> Self {
        self.config.max_retry_jitter = jitter;
        self
    }

    /// Timeout for DHT put operations.
    ///
    /// Default: 10s.
    pub fn put_timeout(mut self, timeout: Duration) -> Self {
        self.config.put_timeout = timeout;
        self
    }

    /// Timeout for DHT get operations.
    ///
    /// Default: 10s.
    pub fn get_timeout(mut self, timeout: Duration) -> Self {
        self.config.get_timeout = timeout;
        self
    }

    /// Build the `DhtConfig`.
    pub fn build(self) -> DhtConfig {
        self.config
    }
}

impl DhtConfig {
    /// Create a new `DhtConfigBuilder` with default values.
    pub fn builder() -> DhtConfigBuilder {
        DhtConfigBuilder {
            config: DhtConfig::default(),
        }
    }

    /// Number of retries after the initial attempt.
    ///
    /// Total attempts = 1 + retries.
    ///
    /// Default: 3.
    pub fn retries(&self) -> usize {
        self.retries
    }

    /// Base delay between retries.
    ///
    /// Default: 5s.
    pub fn base_retry_interval(&self) -> Duration {
        self.base_retry_interval
    }

    /// Max random jitter added to retry interval.
    ///
    /// Default: 10s.
    pub fn max_retry_jitter(&self) -> Duration {
        self.max_retry_jitter
    }

    /// Timeout for DHT put operations.
    ///
    /// Default: 10s.
    pub fn put_timeout(&self) -> Duration {
        self.put_timeout
    }

    /// Timeout for DHT get operations.
    ///
    /// Default: 10s.
    pub fn get_timeout(&self) -> Duration {
        self.get_timeout
    }
}

impl Default for DhtConfig {
    fn default() -> Self {
        Self {
            retries: 3,
            base_retry_interval: Duration::from_secs(5),
            max_retry_jitter: Duration::from_secs(10),
            put_timeout: Duration::from_secs(10),
            get_timeout: Duration::from_secs(10),
        }
    }
}

/// Bubble merge strategy config for detecting and healing split-brain scenarios by joining small clusters with peers advertised in DHT that are not our neighbors.
#[derive(Debug, Clone)]
pub enum BubbleMergeConfig {
    Enabled(BubbleMergeConfigInner),
    Disabled,
}

#[derive(Debug, Clone)]
pub struct BubbleMergeConfigInner {
    base_interval: Duration,
    max_jitter: Duration,
    min_neighbors: usize,
    fail_topic_creation_on_merge_startup_failure: bool,
}

#[derive(Debug, Clone)]
pub struct BubbleMergeConfigBuilder {
    config: BubbleMergeConfigInner,
}

impl Default for BubbleMergeConfig {
    fn default() -> Self {
        Self::Enabled(BubbleMergeConfigInner::default())
    }
}

impl Default for BubbleMergeConfigInner {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_secs(60),
            max_jitter: Duration::from_secs(120),
            min_neighbors: 4,
            fail_topic_creation_on_merge_startup_failure: true,
        }
    }
}

impl BubbleMergeConfig {
    /// Create a new `BubbleMergeConfigBuilder` with default values.
    pub fn builder() -> BubbleMergeConfigBuilder {
        BubbleMergeConfigBuilder {
            config: BubbleMergeConfigInner::default(),
        }
    }
}

impl BubbleMergeConfigInner {
    /// Base interval for bubble merge attempts.
    ///
    /// Default: 60s.
    pub fn base_interval(&self) -> Duration {
        self.base_interval
    }

    /// Max random jitter added to bubble merge interval.
    ///
    /// Default: 120s.
    pub fn max_jitter(&self) -> Duration {
        self.max_jitter
    }

    /// Minimum number of neighbors required to attempt a bubble merge.
    ///
    /// Default: 4.
    pub fn min_neighbors(&self) -> usize {
        self.min_neighbors
    }

    /// Whether to fail topic creation
    ///
    /// If a bubble merge startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_merge_startup_failure(&self) -> bool {
        self.fail_topic_creation_on_merge_startup_failure
    }
}

impl BubbleMergeConfigBuilder {
    /// Base interval for bubble merge attempts. No-op if `interval` is `Duration::ZERO`.
    ///
    /// If `base_interval` is called only once with `Duration::ZERO`, default value prevails.
    /// If `base_interval` is first called with a > `Duration::ZERO`, and then again with `Duration::ZERO`, the first set value is kept.
    ///
    /// Default: 60s.
    pub fn base_interval(mut self, interval: Duration) -> Self {
        if interval > Duration::ZERO {
            self.config.base_interval = interval;
        }
        self
    }

    /// Max random jitter added to bubble merge interval.
    ///
    /// Default: 120s
    pub fn max_jitter(mut self, jitter: Duration) -> Self {
        self.config.max_jitter = jitter;
        self
    }

    /// Minimum number of neighbors required to attempt a bubble merge.
    ///
    /// Default: 4.
    pub fn min_neighbors(mut self, min_neighbors: usize) -> Self {
        self.config.min_neighbors = min_neighbors;
        self
    }

    /// Whether to fail topic creation
    ///
    /// If a bubble merge startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_merge_startup_failure(mut self, fail: bool) -> Self {
        self.config.fail_topic_creation_on_merge_startup_failure = fail;
        self
    }

    /// Build the `BubbleMergeConfig`.
    pub fn build(self) -> BubbleMergeConfig {
        BubbleMergeConfig::Enabled(self.config)
    }
}

/// Message overlap merge strategy config for detecting and healing split-brain scenarios by checking for overlapping message hashes with other cluster peers.
#[derive(Debug, Clone)]
pub enum MessageOverlapMergeConfig {
    Enabled(MessageOverlapMergeConfigInner),
    Disabled,
}

#[derive(Debug, Clone)]
pub struct MessageOverlapMergeConfigInner {
    base_interval: Duration,
    max_jitter: Duration,
    fail_topic_creation_on_merge_startup_failure: bool,
}

#[derive(Debug, Clone)]
pub struct MessageOverlapMergeConfigBuilder {
    config: MessageOverlapMergeConfigInner,
}

impl Default for MessageOverlapMergeConfigInner {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_secs(60),
            max_jitter: Duration::from_secs(120),
            fail_topic_creation_on_merge_startup_failure: true,
        }
    }
}

impl Default for MessageOverlapMergeConfig {
    fn default() -> Self {
        Self::Enabled(MessageOverlapMergeConfigInner::default())
    }
}

impl MessageOverlapMergeConfig {
    /// Create a new `MessageOverlapMergeConfigBuilder` with default values.
    pub fn builder() -> MessageOverlapMergeConfigBuilder {
        MessageOverlapMergeConfigBuilder {
            config: MessageOverlapMergeConfigInner::default(),
        }
    }
}

impl MessageOverlapMergeConfigInner {
    /// Base interval for message overlap merge attempts.
    ///
    /// Default: 60s.
    pub fn base_interval(&self) -> Duration {
        self.base_interval
    }

    /// Max random jitter added to message overlap merge interval.
    ///
    /// Default: 120s.
    pub fn max_jitter(&self) -> Duration {
        self.max_jitter
    }

    /// Whether to fail topic creation
    ///
    /// If a message overlap merge startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_merge_startup_failure(&self) -> bool {
        self.fail_topic_creation_on_merge_startup_failure
    }
}

impl MessageOverlapMergeConfigBuilder {
    /// Base interval for message overlap merge attempts. No-op if `interval` is `Duration::ZERO`.
    ///
    /// If `base_interval` is called only once with `Duration::ZERO`, default value prevails.
    /// If `base_interval` is first called with a > `Duration::ZERO`, and then again with `Duration::ZERO`, the first set value is kept.
    ///
    /// Default: 60s.
    pub fn base_interval(mut self, interval: Duration) -> Self {
        if interval > Duration::ZERO {
            self.config.base_interval = interval;
        }
        self
    }

    /// Max random jitter added to message overlap merge interval.
    ///
    /// Default: 120s. Minimum is 0s.
    pub fn max_jitter(mut self, jitter: Duration) -> Self {
        self.config.max_jitter = jitter;
        self
    }

    /// Whether to fail topic creation
    ///
    /// If a message overlap merge startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_merge_startup_failure(mut self, fail: bool) -> Self {
        self.config.fail_topic_creation_on_merge_startup_failure = fail;
        self
    }

    /// Build the `MessageOverlapMergeConfig`.
    pub fn build(self) -> MessageOverlapMergeConfig {
        MessageOverlapMergeConfig::Enabled(self.config)
    }
}

/// Publisher strategy config for publishing bootstrap records to DHT for peer discovery.
#[derive(Debug, Clone)]
pub enum PublisherConfig {
    Enabled(PublisherConfigInner),
    Disabled,
}

#[derive(Debug, Clone)]
pub struct PublisherConfigInner {
    initial_delay: Duration,
    base_interval: Duration,
    max_jitter: Duration,
    fail_topic_creation_on_publishing_startup_failure: bool,
}

#[derive(Debug, Clone)]
pub struct PublisherConfigBuilder {
    config: PublisherConfigInner,
}

impl Default for PublisherConfigInner {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(10),
            base_interval: Duration::from_secs(10),
            max_jitter: Duration::from_secs(50),
            fail_topic_creation_on_publishing_startup_failure: true,
        }
    }
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self::Enabled(PublisherConfigInner::default())
    }
}

impl PublisherConfig {
    /// Create a new `PublisherConfigBuilder` with default values.
    pub fn builder() -> PublisherConfigBuilder {
        PublisherConfigBuilder {
            config: PublisherConfigInner::default(),
        }
    }
}

impl PublisherConfigInner {
    /// Initial delay before starting publisher.
    ///
    /// Default: 10s.
    pub fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    /// Base interval for publisher attempts.
    ///
    /// Default: 10s.
    pub fn base_interval(&self) -> Duration {
        self.base_interval
    }

    /// Max random jitter added to publisher interval.
    ///
    /// Default: 50s.
    pub fn max_jitter(&self) -> Duration {
        self.max_jitter
    }

    /// Whether to fail topic creation
    ///
    /// If a publisher startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_publishing_startup_failure(&self) -> bool {
        self.fail_topic_creation_on_publishing_startup_failure
    }
}

impl PublisherConfigBuilder {
    /// Initial delay before starting publisher.
    ///
    /// Default: 10s.
    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.config.initial_delay = delay;
        self
    }

    /// Base interval for publisher attempts. No-op if `interval` is `Duration::ZERO`.
    ///
    /// If `base_interval` is called only once with `Duration::ZERO`, default value prevails.
    /// If `base_interval` is first called with a > `Duration::ZERO`, and then again with `Duration::ZERO`, the first set value is kept.
    ///
    /// Default: 10s.
    pub fn base_interval(mut self, interval: Duration) -> Self {
        if interval > Duration::ZERO {
            self.config.base_interval = interval;
        }
        self
    }

    /// Max random jitter added to publisher interval.
    ///
    /// Default: 50s.
    pub fn max_jitter(mut self, jitter: Duration) -> Self {
        self.config.max_jitter = jitter;
        self
    }

    /// Whether to fail topic creation
    ///
    /// If a publisher startup check fails (ret Err()) or just log and run topic without.
    ///
    /// Default: true.
    pub fn fail_topic_creation_on_publishing_startup_failure(mut self, fail: bool) -> Self {
        self.config
            .fail_topic_creation_on_publishing_startup_failure = fail;
        self
    }

    /// Build the `PublisherConfig`.
    pub fn build(self) -> PublisherConfig {
        PublisherConfig::Enabled(self.config)
    }
}

/// Merge strategies run periodically in the background and attempt to merge split clusters by joining peers in DHT records
/// and message hashes for bubble detection and merging, and by joining peers in DHT records with overlapping message hashes
/// for message overlap detection and merging.
#[derive(Debug, Clone, Default)]
pub struct MergeConfig {
    bubble_merge: BubbleMergeConfig,
    message_overlap_merge: MessageOverlapMergeConfig,
}

/// Builder for `MergeConfig`.
#[derive(Debug, Clone)]
pub struct MergeConfigBuilder {
    config: MergeConfig,
}

impl MergeConfig {
    /// Create a new `MergeConfigBuilder` with default values.
    pub fn builder() -> MergeConfigBuilder {
        MergeConfigBuilder {
            config: MergeConfig::default(),
        }
    }

    /// Bubble merge strategy config.
    ///
    /// Default: BubbleMergeConfig::default()
    pub fn bubble_merge(&self) -> &BubbleMergeConfig {
        &self.bubble_merge
    }

    /// Message overlap merge strategy config.
    ///
    /// Default: MessageOverlapMergeConfig::default()
    pub fn message_overlap_merge(&self) -> &MessageOverlapMergeConfig {
        &self.message_overlap_merge
    }
}

impl MergeConfigBuilder {
    /// Bubble merge strategy config.
    ///
    /// Default: BubbleMergeConfig::default()
    pub fn bubble_merge(mut self, bubble_merge: BubbleMergeConfig) -> Self {
        self.config.bubble_merge = bubble_merge;
        self
    }

    /// Message overlap merge strategy config.
    ///
    /// Default: MessageOverlapMergeConfig::default()
    pub fn message_overlap_merge(
        mut self,
        message_overlap_merge: MessageOverlapMergeConfig,
    ) -> Self {
        self.config.message_overlap_merge = message_overlap_merge;
        self
    }

    /// Build the `MergeConfig`.
    pub fn build(self) -> MergeConfig {
        self.config
    }
}

/// Bootstrap process settings for peer discovery.
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    max_bootstrap_records: usize,
    no_peers_retry_interval: Duration,
    per_peer_join_settle_time: Duration,
    join_confirmation_wait_time: Duration,
    discovery_poll_interval: Duration,
    publish_record_on_startup: bool,
    check_older_records_first_on_startup: bool,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            max_bootstrap_records: 5,
            no_peers_retry_interval: Duration::from_millis(1500),
            per_peer_join_settle_time: Duration::from_millis(100),
            join_confirmation_wait_time: Duration::from_millis(500),
            discovery_poll_interval: Duration::from_millis(2000),
            publish_record_on_startup: true,
            check_older_records_first_on_startup: false,
        }
    }
}

/// Builder for `BootstrapConfig`.
#[derive(Debug)]
pub struct BootstrapConfigBuilder {
    config: BootstrapConfig,
}

impl BootstrapConfigBuilder {
    /// Max bootstrap records per topic per minute slot.
    ///
    /// If zero, we don't publish (PublisherConfig will be set to Disabled).
    ///
    /// Default: 5.
    pub fn max_bootstrap_records(mut self, max_records: usize) -> Self {
        self.config.max_bootstrap_records = max_records;
        self
    }

    /// How long to wait when no peers are found before retrying.
    ///
    /// Default: 1500ms.
    pub fn no_peers_retry_interval(mut self, interval: Duration) -> Self {
        self.config.no_peers_retry_interval = interval;
        self
    }

    /// How long to wait after joining a peer before attempting to join another.
    ///
    /// Default: 100ms.
    pub fn per_peer_join_settle_time(mut self, interval: Duration) -> Self {
        self.config.per_peer_join_settle_time = interval;
        self
    }

    /// How long to wait after joining a peer before checking if joined successfully.
    ///
    /// Default: 500ms.
    pub fn join_confirmation_wait_time(mut self, interval: Duration) -> Self {
        self.config.join_confirmation_wait_time = interval;
        self
    }

    /// How long to wait between DHT discovery attempts.
    ///
    /// Default: 2000ms.
    pub fn discovery_poll_interval(mut self, interval: Duration) -> Self {
        self.config.discovery_poll_interval = interval;
        self
    }

    /// Whether to publish a bootstrap record unconditionally on startup before dht get.
    ///
    /// Default: true.
    pub fn publish_record_on_startup(mut self, publish: bool) -> Self {
        self.config.publish_record_on_startup = publish;
        self
    }

    /// Whether to check `unix_minute` and `unix_minute-1` or `unix_minute-1` and `unix_minute-2` on startup.
    ///
    /// If this is enabled, we first fetch `unix_minute-1` and `unix_minute-2`.
    ///  
    /// If joining longer running, existing topics is priority, set to true.
    /// If minimizing bootstrap time for cluster cold starts (2+ nodes starting roughly
    /// at the same time into a topic without peers), set to false.
    ///
    /// Default: false.
    pub fn check_older_records_first_on_startup(mut self, check: bool) -> Self {
        self.config.check_older_records_first_on_startup = check;
        self
    }

    /// Build the `BootstrapConfig`.
    pub fn build(self) -> BootstrapConfig {
        self.config
    }
}

impl BootstrapConfig {
    /// Create a new `BootstrapConfigBuilder` with default values.
    pub fn builder() -> BootstrapConfigBuilder {
        BootstrapConfigBuilder {
            config: BootstrapConfig::default(),
        }
    }

    /// Max bootstrap records per topic per minute slot.
    ///
    /// If zero, we don't publish (PublisherConfig will be set to Disabled).
    ///
    /// Default: 5.
    pub fn max_bootstrap_records(&self) -> usize {
        self.max_bootstrap_records
    }

    /// How long to wait when no peers are found before retrying.
    ///
    /// Default: 1500ms.
    pub fn no_peers_retry_interval(&self) -> Duration {
        self.no_peers_retry_interval
    }

    /// How long to wait after joining a peer before attempting to join another.
    ///
    /// Default: 100ms.
    pub fn per_peer_join_settle_time(&self) -> Duration {
        self.per_peer_join_settle_time
    }

    /// How long to wait after joining a peer before checking if joined successfully.
    ///
    /// Default: 500ms.
    pub fn join_confirmation_wait_time(&self) -> Duration {
        self.join_confirmation_wait_time
    }

    /// How long to wait between DHT discovery attempts.
    ///
    /// Default: 2000ms.
    pub fn discovery_poll_interval(&self) -> Duration {
        self.discovery_poll_interval
    }

    /// Whether to publish a bootstrap record unconditionally on startup before dht get.
    ///
    /// Default: true.
    pub fn publish_record_on_startup(&self) -> bool {
        self.publish_record_on_startup
    }

    /// Whether to check `unix_minute` and `unix_minute-1` or `unix_minute-1` and `unix_minute-2` on startup.
    ///
    /// If this is enabled, we first fetch `unix_minute-1` and `unix_minute-2`.  
    ///  
    /// If joining longer running, existing topics is priority, set to true.
    /// If minimizing bootstrap time for cluster cold starts (2+ nodes starting roughly
    /// at the same time into a topic without peers), set to false.
    ///
    /// Default: false.
    pub fn check_older_records_first_on_startup(&self) -> bool {
        self.check_older_records_first_on_startup
    }
}

/// Top-level configuration combining all settings.
#[derive(Debug, Clone)]
pub struct Config {
    bootstrap_config: BootstrapConfig,
    publisher_config: PublisherConfig,
    dht_config: DhtConfig,

    merge_config: MergeConfig,

    max_join_peer_count: usize,
    timeouts: TimeoutConfig,
}

impl Config {
    /// Create a new `ConfigBuilder` with default values.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder {
            config: Config::default(),
        }
    }

    /// Publisher strategy config.
    ///
    /// Default: PublisherConfig::default().
    pub fn publisher_config(&self) -> &PublisherConfig {
        &self.publisher_config
    }

    /// DHT operation settings.
    ///
    /// Default: DhtConfig::default().
    pub fn dht_config(&self) -> &DhtConfig {
        &self.dht_config
    }

    /// Bootstrap strategy settings.
    ///
    /// Default: BootstrapConfig::default().
    pub fn bootstrap_config(&self) -> &BootstrapConfig {
        &self.bootstrap_config
    }

    /// Max peers to join simultaneously.
    ///
    /// Minimum: 1.
    ///
    /// Default: 4.
    pub fn max_join_peer_count(&self) -> usize {
        self.max_join_peer_count
    }

    /// Timeout settings.
    ///
    /// Default: TimeoutConfig::default().
    pub fn timeouts(&self) -> &TimeoutConfig {
        &self.timeouts
    }

    /// Merge strategy settings.
    ///
    /// Default: bubble and overlap merges enabled.
    pub fn merge_config(&self) -> &MergeConfig {
        &self.merge_config
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            merge_config: MergeConfig::default(),
            bootstrap_config: BootstrapConfig::default(),
            publisher_config: PublisherConfig::default(),
            dht_config: DhtConfig::default(),
            max_join_peer_count: 4,
            timeouts: TimeoutConfig::default(),
        }
    }
}

/// Builder for `Config`.
#[derive(Debug)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Merge strategy settings.
    ///
    /// Default: MergeConfig::default().
    pub fn merge_config(mut self, merge_config: MergeConfig) -> Self {
        self.config.merge_config = merge_config;
        self
    }

    /// Publisher strategy config.
    ///
    /// Default: PublisherConfig::default().
    pub fn publisher_config(mut self, publisher_config: PublisherConfig) -> Self {
        self.config.publisher_config = publisher_config;
        self
    }

    /// DHT operation settings.
    ///
    /// Default: DhtConfig::default().
    pub fn dht_config(mut self, dht_config: DhtConfig) -> Self {
        self.config.dht_config = dht_config;
        self
    }

    /// Bootstrap strategy settings.
    ///
    /// Default: BootstrapConfig::default().
    pub fn bootstrap_config(mut self, bootstrap_config: BootstrapConfig) -> Self {
        self.config.bootstrap_config = bootstrap_config;
        self
    }

    /// Max peers to join simultaneously.
    ///
    /// Minimum: 1.
    ///
    /// Default: 4.
    pub fn max_join_peer_count(mut self, max_peers: NonZeroU32) -> Self {
        self.config.max_join_peer_count = max_peers.get() as usize;
        self
    }

    /// Timeout settings.
    ///
    /// Default: TimeoutConfig::default().
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.config.timeouts = timeouts;
        self
    }

    /// Build the `Config`.
    ///
    /// If `max_bootstrap_records` is zero, `PublisherConfig` is set to `Disabled`.
    pub fn build(self) -> Config {
        let mut config = self.config;
        if config.bootstrap_config.max_bootstrap_records == 0
            && matches!(config.publisher_config, PublisherConfig::Enabled(_))
        {
            // if max_bootstrap_records is zero, we don't publish, so disable publisher to avoid confusion
            tracing::warn!(
                "Publisher is enabled via PublisherConfig::Enabled(_) but BootstrapConfig.max_bootstrap_records is set to 0 (we effectively never publish). Overriding PublisherConfig to PublisherConfig::Disabled."
            );
            config.publisher_config = PublisherConfig::Disabled;
        }

        config
    }
}
