use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fred::prelude::*;
use fred::types::Message;
use futures::stream::TryStreamExt;
use moka::sync::Cache as MokaCache;
use serde::{Deserialize, Serialize};

use crate::config::DragonflyConfig;
use crate::error::RelayError;

const L1_MAX_BYTES: u64 = 64 * 1024 * 1024;
const L2_POOL_SIZE: usize = 4;

pub const CACHE_NS: &str = "v4";

#[derive(Debug, Serialize, Deserialize)]
pub struct Invalidation {
    pub flush: bool,
    pub scopes: Vec<String>,
}

#[derive(Clone)]
pub struct Cache {
    pool: Pool,
    subscriber: Client,
    l1: L1Cache,
    channel: String,
    healthy: Arc<AtomicBool>,
}

#[derive(Clone)]
struct L1Cache {
    cache: MokaCache<String, Arc<Vec<u8>>>,
}

impl L1Cache {
    fn new(max_bytes: u64) -> Self {
        let cache = MokaCache::builder()
            .weigher(|key: &String, value: &Arc<Vec<u8>>| (key.len() + value.len()) as u32)
            .max_capacity(max_bytes)
            .build();
        Self { cache }
    }

    fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.cache.get(key)
    }

    fn put(&self, key: String, value: Vec<u8>) {
        self.cache.insert(key, Arc::new(value));
    }

    fn invalidate_prefix(&self, prefix: &str) -> usize {
        let keys: Vec<String> = self
            .cache
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.to_string())
            .collect();
        for key in &keys {
            self.cache.invalidate(key);
        }
        keys.len()
    }

    fn flush(&self) {
        self.cache.invalidate_all();
    }
}

fn cache_err(error: fred::error::Error) -> RelayError {
    RelayError::Internal(format!("dragonfly cache: {error}"))
}

impl Cache {
    pub async fn new(config: &DragonflyConfig) -> Result<Self, RelayError> {
        let mut fred_config = Config::from_url(&config.url)
            .map_err(|error| RelayError::ConfigInvalid(format!("dragonfly url: {error}")))?;
        fred_config.fail_fast = false;
        let mut builder = Builder::from_config(fred_config);
        builder.set_policy(ReconnectPolicy::new_exponential(0, 100, 5000, 2));
        let pool = builder
            .build_pool(L2_POOL_SIZE)
            .map_err(|error| RelayError::ConfigInvalid(format!("dragonfly pool: {error}")))?;
        let subscriber = builder
            .build()
            .map_err(|error| RelayError::ConfigInvalid(format!("dragonfly subscriber: {error}")))?;
        pool.init()
            .await
            .map_err(|error| RelayError::ConfigInvalid(format!("dragonfly pool init: {error}")))?;
        subscriber
            .init()
            .await
            .map_err(|error| RelayError::ConfigInvalid(format!("dragonfly subscriber init: {error}")))?;
        Ok(Self {
            pool,
            subscriber,
            l1: L1Cache::new(L1_MAX_BYTES),
            channel: config.channel.clone(),
            healthy: Arc::new(AtomicBool::new(true)),
        })
    }

    fn healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn mark_ok(&self) {
        self.healthy.store(true, Ordering::Relaxed);
    }

    fn mark_err(&self) {
        self.healthy.store(false, Ordering::Relaxed);
    }

    pub async fn start_listener(&self) -> Result<(), RelayError> {
        let reconnect_cache = self.clone();
        let _reconnect = self.subscriber.on_reconnect(move |_| {
            let cache = reconnect_cache.clone();
            async move {
                cache.l1_flush();
                if let Err(error) = cache.l2_flush().await {
                    eprintln!("cache flush on reconnect: {error}");
                } else {
                    cache.mark_ok();
                }
                Ok::<(), fred::error::Error>(())
            }
        });
        let message_l1 = self.l1.clone();
        let message_channel = self.channel.clone();
        let _messages = self.subscriber.on_message(move |message: Message| {
            let l1 = message_l1.clone();
            let channel = message_channel.clone();
            async move {
                if message.channel.as_bytes() == channel.as_bytes() {
                    let payload: &[u8] = match &message.value {
                        Value::Bytes(bytes) => bytes,
                        Value::String(text) => text.as_bytes(),
                        _ => &[],
                    };
                    if !payload.is_empty() {
                        if let Ok(invalidation) = serde_json::from_slice::<Invalidation>(payload) {
                            if invalidation.flush {
                                l1.flush();
                            } else {
                                for scope in &invalidation.scopes {
                                    l1.invalidate_prefix(scope);
                                }
                            }
                        }
                    }
                }
                Ok::<(), fred::error::Error>(())
            }
        });
        let _: () = self
            .subscriber
            .subscribe(self.channel.clone())
            .await
            .map_err(cache_err)?;
        Ok(())
    }

    pub async fn publish(&self, invalidation: &Invalidation) -> Result<(), RelayError> {
        let payload = serde_json::to_vec(invalidation)
            .map_err(|error| RelayError::Internal(format!("invalidation encode: {error}")))?;
        let _: Value = self
            .subscriber
            .publish(self.channel.clone(), payload)
            .await
            .map_err(cache_err)?;
        Ok(())
    }

    async fn l2_get(&self, key: &str) -> Result<Option<Vec<u8>>, RelayError> {
        let value: Option<Vec<u8>> = self.pool.get(key).await.map_err(cache_err)?;
        Ok(value)
    }

    async fn l2_put(&self, key: &str, value: &[u8]) -> Result<(), RelayError> {
        let _: () = self
            .pool
            .set(key, value.to_vec(), None, None, false)
            .await
            .map_err(cache_err)?;
        Ok(())
    }

    async fn l2_del(&self, keys: &[String]) -> Result<(), RelayError> {
        if keys.is_empty() {
            return Ok(());
        }
        let _: () = self.pool.del(keys.to_vec()).await.map_err(cache_err)?;
        Ok(())
    }

    async fn l2_incr(&self, key: &str) -> Result<u64, RelayError> {
        let value: u64 = self.pool.incr(key).await.map_err(cache_err)?;
        Ok(value)
    }

    async fn l2_pattern_del(&self, pattern: &str) -> Result<(), RelayError> {
        let keys: Vec<String> = self
            .pool
            .next()
            .scan_buffered(pattern, Some(100), None)
            .map_ok(|key| key.as_str_lossy().into_owned())
            .try_collect()
            .await
            .map_err(cache_err)?;
        self.l2_del(&keys).await
    }

    async fn l2_flush(&self) -> Result<(), RelayError> {
        self.l2_pattern_del(&format!("{CACHE_NS}:*")).await?;
        self.l2_pattern_del("gen:*").await?;
        Ok(())
    }

    pub async fn gen_get(&self, key: &str) -> u64 {
        match self.pool.get::<Option<u64>, _>(key).await {
            Ok(value) => {
                self.mark_ok();
                value.unwrap_or(0)
            }
            Err(error) => {
                self.mark_err();
                eprintln!("cache gen {key}: {error}");
                0
            }
        }
    }

    pub async fn peek(&self, key: &str) -> Option<Vec<u8>> {
        if self.healthy() {
            if let Some(value) = self.l1.get(key) {
                return Some(value.to_vec());
            }
        }
        match self.l2_get(key).await {
            Ok(Some(value)) => {
                self.mark_ok();
                self.l1.put(key.to_string(), value.clone());
                Some(value)
            }
            Ok(None) => {
                self.mark_ok();
                None
            }
            Err(error) => {
                self.mark_err();
                eprintln!("cache get {key}: {error}");
                None
            }
        }
    }

    pub async fn store(&self, key: String, value: &[u8]) {
        if !self.healthy() {
            return;
        }
        self.l1.put(key.clone(), value.to_vec());
        match self.l2_put(&key, value).await {
            Ok(()) => self.mark_ok(),
            Err(error) => {
                self.mark_err();
                eprintln!("cache put {key}: {error}");
            }
        }
    }

    pub async fn get_or_build<F>(&self, key: String, build: F) -> Result<Vec<u8>, RelayError>
    where
        F: std::future::Future<Output = Result<Vec<u8>, RelayError>> + Send,
    {
        if let Some(value) = self.l1.get(&key) {
            return Ok(value.to_vec());
        }
        match self.l2_get(&key).await {
            Ok(Some(value)) => {
                self.l1.put(key.clone(), value.clone());
                Ok(value)
            }
            Ok(None) => {
                let value = build.await?;
                self.l1.put(key.clone(), value.clone());
                if let Err(error) = self.l2_put(&key, &value).await {
                    eprintln!("cache put {key}: {error}");
                }
                Ok(value)
            }
            Err(error) => {
                eprintln!("cache get {key}: {error}");
                build.await
            }
        }
    }

    pub async fn incr_gen(&self, key: &str) -> Result<u64, RelayError> {
        self.l2_incr(key).await
    }

    pub async fn delete_keys(&self, keys: &[String]) -> Result<(), RelayError> {
        self.l2_del(keys).await
    }

    pub async fn delete_pattern(&self, pattern: &str) -> Result<(), RelayError> {
        self.l2_pattern_del(pattern).await
    }

    pub async fn flush_all(&self) -> Result<(), RelayError> {
        self.l2_flush().await
    }

    pub fn apply_local(&self, invalidation: &Invalidation) {
        if invalidation.flush {
            self.l1_flush();
        } else {
            for scope in &invalidation.scopes {
                self.l1_invalidate_prefix(scope);
            }
        }
    }

    pub fn l1_invalidate_prefix(&self, prefix: &str) -> usize {
        self.l1.invalidate_prefix(prefix)
    }

    pub fn l1_flush(&self) {
        self.l1.flush();
    }
    pub async fn invalidate_all(&self) {
        if let Err(error) = self.flush_all().await {
            eprintln!("cache flush: {error}");
        }
        let invalidation = Invalidation {
            flush: true,
            scopes: Vec::new(),
        };
        self.apply_local(&invalidation);
        if let Err(error) = self.publish(&invalidation).await {
            eprintln!("cache publish: {error}");
        }
    }}
