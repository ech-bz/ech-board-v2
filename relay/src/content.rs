use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

use crate::config::ContentS3Config;
use crate::error::RelayError;
use crate::types::ContentKind;

#[derive(Clone)]
pub struct Content {
    client: Client,
    bucket: String,
    media_base_url: String,
}

impl Content {
    pub fn new(config: &ContentS3Config, media_base_url: &str) -> Self {
        let credentials = Credentials::new(
            config.access_key.clone(),
            config.secret_key.clone(),
            None,
            None,
            "ech-db-relay",
        );
        let s3_config = aws_sdk_s3::Config::builder()
            .endpoint_url(&config.endpoint)
            .region(Region::new(config.region.clone()))
            .credentials_provider(credentials)
            .force_path_style(true)
            .behavior_version(BehaviorVersion::latest())
            .build();
        Self {
            client: Client::from_conf(s3_config),
            bucket: config.bucket.clone(),
            media_base_url: media_base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn ensure_bucket(&self) -> Result<(), RelayError> {
        match self.client.create_bucket().bucket(&self.bucket).send().await {
            Ok(_) => Ok(()),
            Err(error) => {
                let existed = error
                    .raw_response()
                    .map(|response| response.status().as_u16() == 409)
                    .unwrap_or(false);
                if existed {
                    Ok(())
                } else {
                    Err(RelayError::Internal(format!("create bucket: {error}")))
                }
            }
        }
    }

    pub fn key(kind: ContentKind, hash: &[u8; 32]) -> String {
        let hex = hex::encode(hash);
        format!("{}/{}/{}/{}", kind.directory(), &hex[0..2], &hex[2..4], hex)
    }

    pub fn public_url(&self, kind: ContentKind, hash: &[u8; 32]) -> String {
        format!(
            "{}/{}/{}",
            self.media_base_url,
            self.bucket,
            Self::key(kind, hash)
        )
    }

    pub async fn put_bytes(
        &self,
        kind: ContentKind,
        hash: &[u8; 32],
        data: Vec<u8>,
    ) -> Result<(), RelayError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::key(kind, hash))
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|error| RelayError::Internal(format!("s3 put: {error}")))?;
        Ok(())
    }

    pub async fn get(
        &self,
        kind: ContentKind,
        hash: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, RelayError> {
        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::key(kind, hash))
            .send()
            .await
        {
            Ok(output) => {
                let body = output
                    .body
                    .collect()
                    .await
                    .map_err(|error| RelayError::Internal(format!("s3 body: {error}")))?;
                Ok(Some(body.into_bytes().to_vec()))
            }
            Err(error) => {
                if is_missing(&error) {
                    Ok(None)
                } else {
                    Err(RelayError::Internal(format!("s3 get: {error}")))
                }
            }
        }
    }
}

fn is_missing(error: &aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>) -> bool {
    match error {
        aws_sdk_s3::error::SdkError::ServiceError(service) => {
            service.err().is_no_such_key()
                || service.raw().status().as_u16() == 404
        }
        _ => false,
    }
}
