use serde::{Deserialize, Serialize};

use ech_db_entity::EntityId;
use ech_db_entity::CollectionKey;
use ech_db_protocol::wire::StateWrite;

use crate::error::RelayError;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Media,
    Thumbnail,
    PlainText,
    MediaMeta,
    Reaction,
}

impl ContentKind {
    pub fn directory(self) -> &'static str {
        match self {
            ContentKind::Text => "text",
            ContentKind::Media => "media",
            ContentKind::Thumbnail => "thumb",
            ContentKind::PlainText => "plaintext",
            ContentKind::MediaMeta => "media_meta",
            ContentKind::Reaction => "reaction",
        }
    }

    pub fn redirects(self) -> bool {
        matches!(
            self,
            ContentKind::Media | ContentKind::Thumbnail | ContentKind::Reaction
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Jpeg,
    Png,
    WebP,
    Gif,
    Mp4,
    WebM,
    Mp3,
    Ogg,
    Pdf,
}

impl FileType {
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0xff, 0xd8, ..] => Some(Self::Jpeg),
            [0x89, 0x50, 0x4e, 0x47, ..] => Some(Self::Png),
            [0x52, 0x49, 0x46, 0x46, _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some(Self::WebP),
            [b'G', b'I', b'F', b'8', b'7', b'a', ..]
            | [b'G', b'I', b'F', b'8', b'9', b'a', ..] => Some(Self::Gif),
            [_, _, _, _, b'f', b't', b'y', b'p', ..] => Some(Self::Mp4),
            [0x1a, 0x45, 0xdf, 0xa3, ..] => Some(Self::WebM),
            [b'I', b'D', b'3', ..] => Some(Self::Mp3),
            [0xff, b, ..] if *b & 0xE0 == 0xE0 => Some(Self::Mp3),
            [b'O', b'g', b'g', b'S', ..] => Some(Self::Ogg),
            [0x25, b'P', b'D', b'F', ..] => Some(Self::Pdf),
            _ => None,
        }
    }

    pub fn supports_thumbnail(&self) -> bool {
        matches!(
            self,
            Self::Jpeg | Self::Png | Self::WebP | Self::Gif | Self::Mp4 | Self::WebM
        )
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Gif => "image/gif",
            Self::Mp4 => "video/mp4",
            Self::WebM => "video/webm",
            Self::Mp3 => "audio/mpeg",
            Self::Ogg => "audio/ogg",
            Self::Pdf => "application/pdf",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MediaMeta {
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    Forum,
    Board,
    Thread,
    Post,
}

#[derive(Serialize, Clone)]
pub struct AppliedEvent {
    pub kind: EntityKind,
    pub id: [u8; 32],
    pub counter: u64,
    pub event: Vec<u8>,
}

impl AppliedEvent {
    pub fn for_entity(
        kind: EntityKind,
        id: [u8; 32],
        writes: &[StateWrite],
    ) -> Result<Vec<AppliedEvent>, RelayError> {
        let events = EntityId(id)
            .events()
            .map_err(|error| RelayError::Internal(error.to_string()))?;
        let prefix = CollectionKey::entry_prefix(&events);
        let mut applied = Vec::new();
        for write in writes {
            let Some(suffix) = write.key.strip_prefix(prefix.as_slice()) else {
                continue;
            };
            let counter = CollectionKey::decode_feed_seq(suffix)
                .map_err(|error| RelayError::Internal(error.to_string()))?;
            applied.push(AppliedEvent {
                kind,
                id,
                counter,
                event: write.value.clone(),
            });
        }
        applied.sort_by_key(|event| event.counter);
        Ok(applied)
    }
}

#[derive(Clone)]
pub struct Batch {
    pub forum: Option<[u8; 32]>,
    pub board: Option<[u8; 32]>,
    pub thread: Option<[u8; 32]>,
    pub post: Option<[u8; 32]>,
    pub events: Vec<AppliedEvent>,
}

#[derive(Serialize)]
pub struct SendResponse {
    pub accepted_by: Vec<String>,
    pub digest: String,
    pub events: Vec<AppliedEvent>,
    pub created: Vec<[u8; 32]>,
}

#[derive(Serialize, Deserialize)]
pub struct NonceInfo {
    pub nonce: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BanHashes {
    pub hashes: Vec<[u8; 32]>,
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub forum: [u8; 32],
    pub program: [u8; 32],
    pub admin: [u8; 32],
}
