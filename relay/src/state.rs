use std::sync::Arc;

use ech_db_entity::EntityId;

use crate::cache::Cache;
use crate::captcha::Captcha;
use crate::config::{Limits, RelayConfig};
use crate::content::Content;
use crate::db::Db;
use crate::error::RelayError;
use crate::geoip::GeoIp;
use crate::realtime::Realtime;
use crate::secrets::Secrets;

pub struct AppState {
    pub db: Arc<Db>,
    pub content: Content,
    pub secrets: Secrets,
    pub geoip: GeoIp,
    pub captcha: Captcha,
    pub cache: Cache,
    pub realtime: Realtime,
    pub forum_id: EntityId,
    pub root: [u8; 32],
    pub program: [u8; 32],
    pub tripcode_key: String,
    pub limits: Limits,
}

impl AppState {
    pub async fn build(config: RelayConfig) -> Result<Self, RelayError> {
        eprintln!("relay: connecting to database at {}", config.db.endpoint);
        let db = Arc::new(Db::connect(&config.db).await?);
        eprintln!("relay: program {}", hex::encode(db.program()));
        let created = db.ensure_forum().await?;
        if created {
            eprintln!("relay: forum bootstrapped at {}", hex::encode(db.root()));
        }
        eprintln!("relay: connecting to content storage at {}", config.s3.endpoint);
        let content = Content::new(&config.s3, &config.content.media_base_url);
        content.ensure_bucket().await?;
        let secrets = Secrets::new(&config.secrets.hmac_key, &config.secrets.uid_key)?;
        let geoip = GeoIp::load(&config.secrets.geoip_path)?;
        let captcha = Captcha::new(&config.captcha)?;
        eprintln!("relay: connecting to cache at {}", config.dragonfly.url);
        let cache = Cache::new(&config.dragonfly).await?;
        cache.start_listener().await?;
        eprintln!("relay: ready");
        Ok(Self {
            forum_id: db.root_id(),
            root: db.root(),
            program: db.program(),
            db,
            content,
            secrets,
            geoip,
            captcha,
            cache,
            realtime: Realtime::new(),
            tripcode_key: config.secrets.tripcode_key,
            limits: config.limits,
        })
    }
}
