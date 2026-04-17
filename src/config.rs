use std::time::Duration;

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

    /// How long to wait when joining a peer. Default: 5s.
    pub fn join_peer_timeout(&self) -> Duration {
        self.join_peer_timeout
    }

    /// How long to wait when broadcasting messages. Default: 5s.
    pub fn broadcast_timeout(&self) -> Duration {
        self.broadcast_timeout
    }

    /// How long to wait when broadcasting to neighbors. Default: 5s.
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
    /// How long to wait when joining a peer. Default: 5s.
    pub fn join_peer_timeout(mut self, timeout: Duration) -> Self {
        self.timeouts.join_peer_timeout = timeout;
        self
    }

    /// How long to wait when broadcasting messages. Default: 5s.
    pub fn broadcast_timeout(mut self, timeout: Duration) -> Self {
        self.timeouts.broadcast_timeout = timeout;
        self
    }

    /// How long to wait when broadcasting to neighbors. Default: 5s.
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
    /// Number of DHT operation retry attempts (first attempt + retries). Default: 3.
    pub fn retries(mut self, retries: usize) -> Self {
        self.config.retries = retries;
        self
    }

    /// Base delay between retries. Default: 5s.
    pub fn base_retry_interval(mut self, interval: Duration) -> Self {
        self.config.base_retry_interval = interval;
        self
    }

    /// Max random jitter added to retry interval. Default: 10s.
    pub fn max_retry_jitter(mut self, jitter: Duration) -> Self {
        self.config.max_retry_jitter = jitter;
        self
    }

    /// Timeout for DHT put operations. Default: 10s.
    pub fn put_timeout(mut self, timeout: Duration) -> Self {
        self.config.put_timeout = timeout;
        self
    }

    /// Timeout for DHT get operations. Default: 10s.
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
    
    /// Number of DHT operation retry attempts (first attempt + retries). Default: 3.
    pub fn retries(&self) -> usize {
        self.retries
    }

    /// Base delay between retries. Default: 5s.
    pub fn base_retry_interval(&self) -> Duration {
        self.base_retry_interval
    }

    /// Max random jitter added to retry interval. Default: 10s.
    pub fn max_retry_jitter(&self) -> Duration {
        self.max_retry_jitter
    }

    /// Timeout for DHT put operations. Default: 10s.
    pub fn put_timeout(&self) -> Duration {
        self.put_timeout
    }

    /// Timeout for DHT get operations. Default: 10s.
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

/// Effective interval is `(base_interval + jitter).max(1000ms)`, where `jitter` is sampled in `[0, max_jitter)`.
/// base_interval is minimum 1s, max_jitter is minimum 0s. Default: enabled, 60s base, 120s jitter, 4 min neighbors.
#[derive(Debug, Clone)]
pub enum BubbleMergeConfig {
    Enabled {
        base_interval: Duration,
        max_jitter: Duration,
        min_neighbors: usize,
    },
    Disabled,
}

/// Effective interval is `(base_interval + jitter).max(1000ms)`, where `jitter` is sampled in `[0, max_jitter)`.
/// base_interval is minimum 1s, max_jitter is minimum 0s. Default: enabled, 60s base, 120s jitter.
#[derive(Debug, Clone)]
pub enum MessageOverlapMergeConfig {
    Enabled {
        base_interval: Duration,
        max_jitter: Duration,
    },
    Disabled,
}

/// Effective interval is `(base_interval + jitter).max(1000ms)`, where `jitter` is sampled in `[0, max_jitter)`.
/// base_interval is minimum 1s, max_jitter is minimum 0s. Default: enabled, 10s initial delay, 10s base interval, 50s jitter.
#[derive(Debug, Clone)]
pub enum PublisherConfig {
    Enabled {
        initial_delay: Duration,
        base_interval: Duration,
        max_jitter: Duration,
    },
    Disabled,
}

/// Effective interval is `(base_interval + jitter).max(1000ms)`, where `jitter` is sampled in `[0, max_jitter)`.
/// base_interval is minimum 1s, max_jitter is minimum 0s.
#[derive(Debug, Clone)]
pub struct MergeConfig {
    bubble_merge: BubbleMergeConfig,
    message_overlap_merge: MessageOverlapMergeConfig,
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self::Enabled {
            initial_delay: Duration::from_secs(10),
            base_interval: Duration::from_secs(10),
            max_jitter: Duration::from_secs(50),
        }
    }
}

impl MergeConfig {
    /// Defaults: bubble_merge=Enabled(60s base, 120s jitter, 4 min neighbors), message_overlap_merge=Enabled(60s base, 120s jitter).
    /// Merge strategies run periodically in the background and attempt to merge split clusters by joining peers in DHT records and message hashes for bubble detection and merging, and by joining peers in DHT records with overlapping message hashes for message overlap detection and merging.
    /// base_interval is minimum 1s, max_jitter is minimum 0s.
    pub fn new(
        bubble_merge: BubbleMergeConfig,
        message_overlap_merge: MessageOverlapMergeConfig,
    ) -> Self {
        Self {
            bubble_merge,
            message_overlap_merge,
        }
    }

    /// Bubble merge strategy config. Default: enabled, 60s base, 120s jitter, 4 min neighbors.
    /// base_interval is minimum 1s, max_jitter is minimum 0s.
    pub fn bubble_merge(&self) -> &BubbleMergeConfig {
        &self.bubble_merge
    }

    /// Message overlap merge strategy config. Default: enabled, 60s base, 120s jitter.
    /// base_interval is minimum 1s, max_jitter is minimum 0s.
    pub fn message_overlap_merge(&self) -> &MessageOverlapMergeConfig {
        &self.message_overlap_merge
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
    check_last_minute_record_first_on_startup: bool,
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
            check_last_minute_record_first_on_startup: false,
        }
    }
}

/// Builder for `BootstrapConfig`.
#[derive(Debug)]
pub struct BootstrapConfigBuilder {
    config: BootstrapConfig,
}

impl BootstrapConfigBuilder {
    /// Max bootstrap records per topic per minute slot. If zero, we don't publish (PublisherConfig will be set to Disabled). Default: 5.
    pub fn max_bootstrap_records(mut self, max_records: usize) -> Self {
        self.config.max_bootstrap_records = max_records;
        self
    }

    /// How long to wait when no peers are found before retrying. Default: 1500ms.
    pub fn no_peers_retry_interval(mut self, interval_ms: Duration) -> Self {
        self.config.no_peers_retry_interval = interval_ms;
        self
    }

    /// How long to wait after joining a peer before attempting to join another. Default: 100ms.
    pub fn per_peer_join_settle_time(mut self, interval_ms: Duration) -> Self {
        self.config.per_peer_join_settle_time = interval_ms;
        self
    }

    /// How long to wait after joining a peer before checking if joined successfully. Default: 500ms.
    pub fn join_confirmation_wait_time(mut self, interval_ms: Duration) -> Self {
        self.config.join_confirmation_wait_time = interval_ms;
        self
    }

    /// How long to wait between DHT discovery attempts. Default: 2000ms.
    pub fn discovery_poll_interval(mut self, interval_ms: Duration) -> Self {
        self.config.discovery_poll_interval = interval_ms;
        self
    }

    /// Whether to publish a bootstrap record unconditionally on startup before dht get. Default: true.
    pub fn publish_record_on_startup(mut self, publish: bool) -> Self {
        self.config.publish_record_on_startup = publish;
        self
    }

    /// Whether to check the last minute record unix_minute-1 before the current time window unix_minute on startup (in this impl unix_minute and unix_minute-1 are both always fetched, if this is enabled than we first fetch unix_minute-2 and unix_minute-1).  Default: false.
    /// 
    /// If joining longer running, existing topics is priority, set to true.
    /// If minimizing bootstrap time for cluster cold starts (2+ nodes starting roughly at the same time into a topic without peers), set to false.
    pub fn check_last_minute_record_first_on_startup(mut self, check: bool) -> Self {
        self.config.check_last_minute_record_first_on_startup = check;
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

    /// Max bootstrap records per topic per minute slot. If zero, we don't publish (PublisherConfig will be set to Disabled, make sure to use config builder to enforce). Default: 5.
    pub fn max_bootstrap_records(&self) -> usize {
        self.max_bootstrap_records
    }

    /// How long to wait when no peers are found before retrying. Default: 1500ms.
    pub fn no_peers_retry_interval(&self) -> Duration {
        self.no_peers_retry_interval
    }

    /// How long to wait after joining a peer before attempting to join another. Default: 100ms.
    pub fn per_peer_join_settle_time(&self) -> Duration {
        self.per_peer_join_settle_time
    }

    /// How long to wait after joining a peer before checking if joined successfully. Default: 500ms.
    pub fn join_confirmation_wait_time(&self) -> Duration {
        self.join_confirmation_wait_time
    }

    /// How long to wait between DHT discovery attempts. Default: 2000ms.
    pub fn discovery_poll_interval(&self) -> Duration {
        self.discovery_poll_interval
    }

    /// Whether to publish a bootstrap record unconditionally on startup before dht get. Default: true.
    pub fn publish_record_on_startup(&self) -> bool {
        self.publish_record_on_startup
    }

    /// Whether to check the last minute record unix_minute-1 before the current time window unix_minute on startup (in this impl unix_minute and unix_minute-1 are both always fetched, if this is enabled than we first fetch unix_minute-2 and unix_minute-1).  Default: false.
    /// 
    /// If joining longer running, existing topics is priority, set to true.
    /// If minimizing bootstrap time for cluster cold starts (2+ nodes starting roughly at the same time into a topic without peers), set to false.
    pub fn check_last_minute_record_first_on_startup(&self) -> bool {
        self.check_last_minute_record_first_on_startup
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

    /// Publisher strategy config. Default: enabled, 10s initial delay, 10s base interval, 50s jitter.
    /// base_interval is minimum 1s, max_jitter is minimum 0s.
    pub fn publisher_config(&self) -> &PublisherConfig {
        &self.publisher_config
    }

    /// DHT operation settings. Default: DhtConfig::default().
    pub fn dht_config(&self) -> &DhtConfig {
        &self.dht_config
    }

    /// Bootstrap strategy settings. Default: max 5 bootstrap records per topic per minute, 1500ms no peers retry interval, 100ms per peer join settle time, 500ms join confirmation wait time, 2000ms discovery poll interval.
    pub fn bootstrap_config(&self) -> &BootstrapConfig {
        &self.bootstrap_config
    }

    /// Max peers to join simultaneously (min 1). Default: 4.
    pub fn max_join_peer_count(&self) -> usize {
        self.max_join_peer_count
    }

    /// Timeout settings. Default: TimeoutConfig::default().
    pub fn timeouts(&self) -> &TimeoutConfig {
        &self.timeouts
    }

    /// Merge strategy settings. Default: bubble and overlap merges enabled.
    pub fn merge_config(&self) -> &MergeConfig {
        &self.merge_config
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            merge_config: MergeConfig {
                bubble_merge: BubbleMergeConfig::Enabled {
                    base_interval: Duration::from_secs(60),
                    max_jitter: Duration::from_secs(120),
                    min_neighbors: 4,
                },
                message_overlap_merge: MessageOverlapMergeConfig::Enabled {
                    base_interval: Duration::from_secs(60),
                    max_jitter: Duration::from_secs(120),
                },
            },
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
    /// Merge strategy settings. Default: bubble and overlap merges enabled.
    pub fn merge_config(mut self, merge_config: MergeConfig) -> Self {
        self.config.merge_config = merge_config;
        self
    }

    /// Publisher strategy config. Default: enabled, 10s initial delay, 10s base interval, 50s jitter.
    /// base_interval and initial_delay minimum is 1s, max_jitter is minimum 0s.
    pub fn publisher_config(mut self, publisher_config: PublisherConfig) -> Self {
        self.config.publisher_config = publisher_config;
        self
    }

    /// DHT operation settings. Default: DhtConfig::default().
    pub fn dht_config(mut self, dht_config: DhtConfig) -> Self {
        self.config.dht_config = dht_config;
        self
    }

    /// Bootstrap strategy settings. Default: max 5 bootstrap records per topic per minute, 1500ms no peers retry interval, 100ms per peer join settle time, 500ms join confirmation wait time, 2000ms discovery poll interval.
    pub fn bootstrap_config(mut self, bootstrap_config: BootstrapConfig) -> Self {
        self.config.bootstrap_config = bootstrap_config;
        self
    }

    /// Max peers to join simultaneously (min 1). Default: 4.
    pub fn max_join_peer_count(mut self, max_peers: usize) -> Self {
        self.config.max_join_peer_count = max_peers.max(1);
        self
    }

    /// Timeout settings. Default: TimeoutConfig::default().
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.config.timeouts = timeouts;
        self
    }

    /// Build the `Config`. If `max_bootstrap_records` is zero, `PublisherConfig` is set to `Disabled`.
    pub fn build(self) -> Config {
        let mut config = self.config;
        if config.bootstrap_config.max_bootstrap_records == 0 {
            // if max_bootstrap_records is zero, we don't publish, so disable publisher to avoid confusion
            config.publisher_config = PublisherConfig::Disabled;
        }

        config
    }
}
