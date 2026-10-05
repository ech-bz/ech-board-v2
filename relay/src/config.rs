use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct RelayConfig {
    pub server: ServerConfig,
    pub db: DbConfig,
    pub s3: ContentS3Config,
    pub content: ContentConfig,
    pub dragonfly: DragonflyConfig,
    #[serde(default)]
    pub captcha: CaptchaConfig,
    pub secrets: SecretsConfig,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Deserialize, Clone)]
pub struct ServerConfig {
    pub bind: String,
    pub admin_bind: String,
}

#[derive(Deserialize, Clone)]
pub struct DbConfig {
    pub endpoint: String,
    pub root_key_path: String,
    pub program_path: String,
}

#[derive(Deserialize, Clone)]
pub struct ContentS3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Deserialize, Clone)]
pub struct ContentConfig {
    pub media_base_url: String,
}

#[derive(Deserialize, Clone)]
pub struct DragonflyConfig {
    pub url: String,
    pub channel: String,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CaptchaConfig {
    Disabled,
    Turnstile {
        secret: String,
        verify_url: String,
    },
}

impl Default for CaptchaConfig {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Deserialize, Clone)]
pub struct SecretsConfig {
    pub hmac_key: String,
    pub uid_key: String,
    pub tripcode_key: String,
    pub geoip_path: String,
}

#[derive(Deserialize, Clone)]
pub struct Limits {
    pub max_text_size: usize,
    pub max_upload_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_text_size: 65536,
            max_upload_bytes: 200 * 1024 * 1024,
        }
    }
}

pub fn load(path: &str) -> Result<RelayConfig, crate::error::RelayError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| crate::error::RelayError::ConfigInvalid(format!("read {path}: {error}")))?;
    toml::from_str(&text)
        .map_err(|error| crate::error::RelayError::ConfigInvalid(format!("parse {path}: {error}")))
}
