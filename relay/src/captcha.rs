use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::CaptchaConfig;
use crate::error::RelayError;

#[derive(Clone)]
pub enum Captcha {
    Disabled,
    Turnstile {
        client: Client,
        secret: String,
        verify_url: String,
    },
}

#[derive(Serialize)]
struct TurnstileRequest<'a> {
    secret: &'a str,
    response: &'a str,
    remoteip: &'a str,
}

#[derive(Deserialize)]
struct TurnstileResponse {
    success: bool,
}

impl Captcha {
    pub fn new(config: &CaptchaConfig) -> Result<Self, RelayError> {
        match config {
            CaptchaConfig::Disabled => Ok(Self::Disabled),
            CaptchaConfig::Turnstile { secret, verify_url } => {
                let client = Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .map_err(|error| RelayError::ConfigInvalid(format!("captcha client: {error}")))?;
                Ok(Self::Turnstile {
                    client,
                    secret: secret.clone(),
                    verify_url: verify_url.clone(),
                })
            }
        }
    }

    pub fn required(&self) -> bool {
        matches!(self, Self::Turnstile { .. })
    }

    pub async fn verify(&self, token: &str, remote_ip: &str) -> Result<(), RelayError> {
        match self {
            Captcha::Disabled => Ok(()),
            Captcha::Turnstile {
                client,
                secret,
                verify_url,
            } => {
                let request = TurnstileRequest {
                    secret,
                    response: token,
                    remoteip: remote_ip,
                };
                let response = client
                    .post(verify_url)
                    .form(&request)
                    .send()
                    .await
                    .map_err(RelayError::CaptchaRequest)?;
                let parsed = response
                    .json::<TurnstileResponse>()
                    .await
                    .map_err(RelayError::CaptchaDecode)?;
                if parsed.success {
                    Ok(())
                } else {
                    Err(RelayError::CaptchaRejected)
                }
            }
        }
    }
}
