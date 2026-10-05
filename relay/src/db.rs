use ed25519_dalek::SigningKey;
use serde::Serialize;
use tokio::sync::Mutex;

use ech_db_client::Client;
use ech_db_entity::{EntityId, EntityMeta, EntityReader, EntityRecord};
use ech_db_protocol::ids::Stream;
use ech_db_protocol::keys::ReadKey;
use ech_db_protocol::values::{IntentBody, Invocation, ProgramBundle, SignedIntent};
use ech_db_protocol::wire::AppendResult;
use forum_model::{ForumEntity, Responses};

use crate::config::DbConfig;
use crate::error::RelayError;

pub struct Db {
    client: Client,
    root: [u8; 32],
    root_key: SigningKey,
    program: [u8; 32],
    endpoint: String,
    root_lock: Mutex<()>,
}

impl Db {
    pub async fn connect(config: &DbConfig) -> Result<Self, RelayError> {
        let seed = read_key(&config.root_key_path)?;
        let root_key = SigningKey::from_bytes(&seed);
        let root = root_key.verifying_key().to_bytes();
        let client = Client::new(seed).with_endpoint(config.endpoint.clone());
        let wasm = std::fs::read(&config.program_path).map_err(|error| {
            RelayError::ConfigInvalid(format!("program {}: {error}", config.program_path))
        })?;
        let program = client
            .upload_program(ProgramBundle {
                format_version: 1,
                execution_profile: 1,
                wasm,
            })
            .await
            .map_err(|error| RelayError::ConfigInvalid(format!("program upload: {error}")))?;
        Ok(Self {
            client,
            root,
            root_key,
            program,
            endpoint: config.endpoint.clone(),
            root_lock: Mutex::new(()),
        })
    }

    pub fn root(&self) -> [u8; 32] {
        self.root
    }

    pub fn program(&self) -> [u8; 32] {
        self.program
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn root_id(&self) -> EntityId {
        EntityId::root(&self.root)
    }

    pub async fn ensure_forum(&self) -> Result<bool, RelayError> {
        if self.forum_record().await?.is_some() {
            return Ok(false);
        }
        {
            let _guard = self.root_lock.lock().await;
            if self.forum_record().await?.is_some() {
                return Ok(false);
            }
        }
        self.submit_root_serialized("bootstrap", &()).await?;
        Ok(true)
    }

    pub async fn forum_record(&self) -> Result<Option<EntityRecord<ForumEntity>>, RelayError> {
        let id = self.root_id();
        self.read(move |read| async move { Ok(read.record(id, ForumEntity::TYPE_NAME).await?) })
            .await
    }

    pub async fn append_user(
        &self,
        signed: &SignedIntent,
        responses: Vec<u8>,
    ) -> Result<AppendResult, RelayError> {
        self.client
            .append_raw(signed, responses)
            .await
            .map_err(Into::into)
    }

    pub async fn submit_root(
        &self,
        function: &str,
        args: Vec<u8>,
        responses: Vec<u8>,
    ) -> Result<AppendResult, RelayError> {
        let _guard = self.root_lock.lock().await;
        let author = self.root_key.verifying_key().to_bytes();
        let seq = self.seq(&author).await?;
        let body = IntentBody {
            version: IntentBody::FORMAT_VERSION,
            root: self.root,
            author,
            tweak: [0u8; 32],
            expected_seq: seq,
            call: Invocation {
                program_hash: self.program,
                function: function.to_string(),
                args,
            },
        };
        let signed = SignedIntent::sign(body, &self.root_key)
            .map_err(|error| RelayError::Internal(format!("sign root intent: {error}")))?;
        self.client
            .append_raw(&signed, responses)
            .await
            .map_err(Into::into)
    }

    pub async fn submit_root_serialized<A: Serialize + ?Sized>(
        &self,
        function: &str,
        args: &A,
    ) -> Result<AppendResult, RelayError> {
        let encoded = bcs::to_bytes(args).map_err(|error| RelayError::Internal(error.to_string()))?;
        self.submit_root(function, encoded, default_responses()?)
            .await
    }

    pub async fn nonce(&self, author: &[u8; 32]) -> Result<u64, RelayError> {
        self.seq(author).await
    }

    pub async fn seq(&self, author: &[u8; 32]) -> Result<u64, RelayError> {
        let stream = Stream::intent(*author).id();
        self.read(move |read| async move {
            let values = read.raw_get(vec![ReadKey::log_seq(&stream)]).await?;
            match values.into_iter().next().flatten() {
                None => Ok(1),
                Some(bytes) => bcs::from_bytes::<u64>(&bytes)
                    .map(|value| value + 1)
                    .map_err(|_| RelayError::Internal("seq decode".into())),
            }
        })
        .await
    }

    pub async fn read<T, F, Fut>(&self, operation: F) -> Result<T, RelayError>
    where
        F: Fn(EntityReader) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, RelayError>> + Send,
    {
        let result = self
            .client
            .read(move |session| {
                let future = operation(EntityReader::new(session));
                async move { Ok(future.await) }
            })
            .await?;
        result
    }
}

pub fn default_responses() -> Result<Vec<u8>, RelayError> {
    bcs::to_bytes(&Responses {
        uid: None,
        ip32: None,
        tripcode: None,
        geo: None,
    })
    .map_err(|error| RelayError::Internal(error.to_string()))
}

fn read_key(path: &str) -> Result<[u8; 32], RelayError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| RelayError::ConfigInvalid(format!("root key {path}: {error}")))?;
    let bytes = hex::decode(text.trim())
        .map_err(|error| RelayError::ConfigInvalid(format!("root key {path}: {error}")))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| RelayError::ConfigInvalid(format!("root key {path}: expected 32 bytes")))?;
    Ok(seed)
}
