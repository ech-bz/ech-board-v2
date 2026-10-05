use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use serde::Serialize;

use ech_db_entity::{
    CollectionId, CollectionKey, Counter, EntityId, EntityMeta, EntityReader, EntityRecord,
};
use forum_model::*;

use crate::cache::{Cache, CACHE_NS};
use crate::content::Content;
use crate::error::RelayError;
use crate::types::{ContentKind, MediaMeta};

const PAGE_THREADS: usize = 20;
const BUMP_CHUNK: u64 = 500;

#[derive(Serialize, Clone)]
pub struct Table {
    pub id: [u8; 32],
    pub size: u64,
}

#[derive(Serialize, Clone)]
pub struct Feed {
    pub id: [u8; 32],
    pub counter: u64,
}

#[derive(Serialize, Clone)]
pub struct Entity {
    pub feed: Feed,
    pub version: u32,
}

#[derive(Serialize, Clone)]
pub struct EntityRoot {
    pub id: [u8; 32],
    pub entity: Entity,
    pub genesis: bool,
}

#[derive(Serialize, Clone)]
pub struct Bans {
    pub level: [u8; 32],
    pub ip32: u64,
    pub ip24: u64,
    pub ip20: u64,
    pub ip16: u64,
}

#[derive(Serialize, Clone)]
pub struct Moderators {
    pub forum_admin: Option<[u8; 32]>,
    pub forum_mods: Vec<[u8; 32]>,
    pub board_mods: Vec<[u8; 32]>,
    pub thread_mods: Vec<[u8; 32]>,
    pub thread_admin: Option<[u8; 32]>,
}

#[derive(Serialize, Clone)]
pub struct ForumObject {
    pub root: EntityRoot,
    pub projection: ForumProjection,
}

#[derive(Serialize, Clone)]
pub struct ForumProjection {
    pub admin: [u8; 32],
    pub mods: Table,
    pub bans: Bans,
    pub boards: Table,
    pub timestamp_precision_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct BoardObject {
    pub root: EntityRoot,
    pub projection: BoardProjection,
}

#[derive(Serialize, Clone)]
pub struct BoardProjection {
    pub slug: String,
    pub description_hash: Option<[u8; 32]>,
    pub max_media: u64,
    pub bump_limit: u64,
    pub closed: bool,
    pub deleted: bool,
    pub pinned: Vec<[u8; 32]>,
    pub ignore_forum_bans: bool,
    pub mods: Table,
    pub bans: Bans,
    pub reactions: Vec<[u8; 32]>,
    pub threads: Table,
    pub posts: Table,
    pub bumps: Feed,
}

#[derive(Serialize, Clone)]
pub struct ThreadObject {
    pub root: EntityRoot,
    pub projection: ThreadProjection,
}

#[derive(Serialize, Clone)]
pub struct ThreadProjection {
    pub board: [u8; 32],
    pub number: u64,
    pub topic_hash: Option<[u8; 32]>,
    pub op: [u8; 32],
    pub closed: bool,
    pub deleted: bool,
    pub admin: Option<[u8; 32]>,
    pub mods: Table,
    pub bans: Bans,
    pub posts: Feed,
    pub posts_deleted: u64,
    pub last_3: Vec<[u8; 32]>,
}

#[derive(Serialize, Clone)]
pub struct Sender {
    pub pk: [u8; 32],
    pub tweak: [u8; 32],
}

#[derive(Serialize, Clone)]
pub struct PostObject {
    pub root: EntityRoot,
    pub projection: PostProjection,
}

#[derive(Serialize, Clone)]
pub struct PostProjection {
    pub sender: Sender,
    pub thread: [u8; 32],
    pub number: u64,
    pub uid: Vec<u8>,
    pub timestamp_ms: u64,
    pub deleted: bool,
    pub banned: Option<BanKey>,
    pub text_hash: Option<[u8; 32]>,
    pub media_hashes: Vec<[u8; 32]>,
    pub banned_media: Vec<[u8; 32]>,
    pub reactions: Vec<([u8; 32], u64)>,
    pub votes: Vec<([u8; 32], u64)>,
    pub multi_vote: bool,
    pub name_hash: Option<[u8; 32]>,
    pub trip: Option<Tripcode>,
    pub geo: Option<u32>,
    pub mod_note: Option<[u8; 32]>,
}

#[derive(Serialize)]
pub struct ForumView {
    pub forum: ForumObject,
    pub boards: Vec<BoardObject>,
    pub plain_text: HashMap<[u8; 32], Vec<u8>>,
    pub moderators: Moderators,
}

#[derive(Serialize)]
pub struct BoardView {
    pub board: BoardObject,
    pub threads: Vec<ThreadObject>,
    pub op_posts: HashMap<[u8; 32], PostObject>,
    pub last_3: HashMap<[u8; 32], Vec<PostObject>>,
    pub text: HashMap<[u8; 32], Vec<u8>>,
    pub plain_text: HashMap<[u8; 32], Vec<u8>>,
    pub media_meta: HashMap<[u8; 32], MediaMeta>,
    pub next_cursor: Option<u64>,
    pub moderators: Moderators,
}

#[derive(Serialize)]
pub struct ThreadView {
    pub thread: ThreadObject,
    pub posts: Vec<PostObject>,
    pub text: HashMap<[u8; 32], Vec<u8>>,
    pub plain_text: HashMap<[u8; 32], Vec<u8>>,
    pub media_meta: HashMap<[u8; 32], MediaMeta>,
    pub moderators: Moderators,
}

#[derive(Serialize)]
pub struct PostView {
    pub post: PostObject,
    pub thread: ThreadObject,
    pub board: BoardObject,
    pub text: HashMap<[u8; 32], Vec<u8>>,
    pub plain_text: HashMap<[u8; 32], Vec<u8>>,
    pub media_meta: HashMap<[u8; 32], MediaMeta>,
    pub moderators: Moderators,
}

#[derive(Serialize)]
pub struct BansView {
    pub level: [u8; 32],
    pub bans: Vec<BanEntryView>,
    pub next_cursor: Option<u64>,
}

#[derive(Serialize)]
pub struct BanEntryView {
    pub mask: u8,
    pub ip_hash: [u8; 32],
    pub reason_hash: [u8; 32],
    pub reason: Option<String>,
    pub expires: u64,
}

#[derive(Serialize)]
pub struct FeedView {
    pub items: Vec<Vec<u8>>,
    pub next_cursor: Option<u64>,
}

#[derive(Serialize, serde::Deserialize)]
pub struct PostReactionsView {
    pub reaction: Option<[u8; 32]>,
}

fn entity_root(events: CollectionId, version: u32, counter: u64, id: EntityId) -> EntityRoot {
    EntityRoot {
        id: id.0,
        entity: Entity {
            feed: Feed {
                id: events.as_array().to_owned(),
                counter,
            },
            version,
        },
        genesis: false,
    }
}

fn entity_error(error: ech_db_entity::Error) -> RelayError {
    RelayError::Internal(error.to_string())
}

pub async fn load_forum(db: &EntityReader, id: EntityId) -> Result<ForumObject, RelayError> {
    let record = db
        .record::<ForumEntity>(id, ForumEntity::TYPE_NAME)
        .await?
        .ok_or_else(|| RelayError::NotFound("forum".into()))?;
    let state = record.state;
    let version = state.version();

    let genesis = db
        .feed_entry::<ForumEvent>(&id.events().map_err(entity_error)?, 1)
        .await?
        .ok_or_else(|| RelayError::Internal("forum genesis missing".into()))?;
    let admin = match genesis {
        ForumEvent::Genesis { admin } => admin,
        _ => return Err(RelayError::Internal("forum genesis event mismatch".into())),
    };

    let mods_id = id.collection("mods").map_err(entity_error)?;
    let bans_id = id.collection("bans").map_err(entity_error)?;
    let boards_id = id.collection("boards").map_err(entity_error)?;
    let timestamp_precision = *state.timestamp_precision().map_err(entity_error)?;

    let mods_count = db.counter(&mods_id, Counter::Count).await?;
    let boards_count = db.counter(&boards_id, Counter::Count).await?;
    let bans = count_bans(db, &bans_id, id.0).await?;
    let events = db
        .counter(&id.events().map_err(entity_error)?, Counter::Length)
        .await?;

    Ok(ForumObject {
        root: entity_root(id.events().map_err(entity_error)?, version, events, id),
        projection: ForumProjection {
            admin,
            mods: Table {
                id: mods_id.as_array().to_owned(),
                size: mods_count,
            },
            bans,
            boards: Table {
                id: boards_id.as_array().to_owned(),
                size: boards_count,
            },
            timestamp_precision_ms: timestamp_precision,
        },
    })
}

async fn count_bans(db: &EntityReader, bans_id: &CollectionId, level: [u8; 32]) -> Result<Bans, RelayError> {
    let entries = db.map_entries::<BanKey, BanValue>(bans_id).await?;
    let mut counts = [0u64; 4];
    for (key, _) in &entries {
        match key.mask {
            32 => counts[0] += 1,
            24 => counts[1] += 1,
            20 => counts[2] += 1,
            16 => counts[3] += 1,
            _ => {}
        }
    }
    Ok(Bans {
        level,
        ip32: counts[0],
        ip24: counts[1],
        ip20: counts[2],
        ip16: counts[3],
    })
}

pub async fn load_board(db: &EntityReader, id: EntityId) -> Result<BoardObject, RelayError> {
    let record = db
        .record::<BoardEntity>(id, BoardEntity::TYPE_NAME)
        .await?
        .ok_or_else(|| RelayError::NotFound("board".into()))?;
    let state = record.state;
    let version = state.version();

    let mods_id = id.collection("mods").map_err(entity_error)?;
    let bans_id = id.collection("bans").map_err(entity_error)?;
    let threads_id = id.collection("threads").map_err(entity_error)?;
    let posts_id = id.collection("posts").map_err(entity_error)?;
    let bumps_id = id.collection("bumps").map_err(entity_error)?;

    let slug = state.slug().map_err(entity_error)?.clone();
    let description_hash = *state.desc_hash().map_err(entity_error)?;
    let max_media = *state.max_media().map_err(entity_error)?;
    let bump_limit = *state.bump_limit().map_err(entity_error)?;
    let closed = *state.closed().map_err(entity_error)?;
    let deleted = *state.deleted().map_err(entity_error)?;
    let pinned = state
        .pinned()
        .map_err(entity_error)?
        .iter()
        .map(|id| id.0)
        .collect();
    let ignore_forum_bans = *state.ignore_forum_bans().map_err(entity_error)?;
    let reactions = state.reactions().map_err(entity_error)?.clone();

    let mods_count = db.counter(&mods_id, Counter::Count).await?;
    let threads_count = db.counter(&threads_id, Counter::Count).await?;
    let posts_count = db.counter(&posts_id, Counter::Count).await?;
    let bans = count_bans(db, &bans_id, id.0).await?;
    let bumps = db.counter(&bumps_id, Counter::Length).await?;
    let events = db
        .counter(&id.events().map_err(entity_error)?, Counter::Length)
        .await?;

    Ok(BoardObject {
        root: entity_root(id.events().map_err(entity_error)?, version, events, id),
        projection: BoardProjection {
            slug: String::from_utf8_lossy(&slug).into_owned(),
            description_hash,
            max_media,
            bump_limit,
            closed,
            deleted,
            pinned,
            ignore_forum_bans,
            mods: Table {
                id: mods_id.as_array().to_owned(),
                size: mods_count,
            },
            bans,
            reactions,
            threads: Table {
                id: threads_id.as_array().to_owned(),
                size: threads_count,
            },
            posts: Table {
                id: posts_id.as_array().to_owned(),
                size: posts_count,
            },
            bumps: Feed {
                id: bumps_id.as_array().to_owned(),
                counter: bumps,
            },
        },
    })
}

pub async fn load_thread(db: &EntityReader, id: EntityId) -> Result<ThreadObject, RelayError> {
    let record = db
        .record::<ThreadEntity>(id, ThreadEntity::TYPE_NAME)
        .await?
        .ok_or_else(|| RelayError::NotFound("thread".into()))?;
    let state = record.state;
    let version = state.version();

    let mods_id = id.collection("mods").map_err(entity_error)?;
    let bans_id = id.collection("bans").map_err(entity_error)?;
    let posts_id = id.collection("posts").map_err(entity_error)?;

    let board = state.board().map_err(entity_error)?.0;
    let number = *state.number().map_err(entity_error)?;
    let topic_hash = *state.topic_hash().map_err(entity_error)?;
    let closed = *state.closed().map_err(entity_error)?;
    let deleted = *state.deleted().map_err(entity_error)?;
    let admin = *state.admin().map_err(entity_error)?;
    let posts_deleted = *state.posts_deleted().map_err(entity_error)?;

    let mods_count = db.counter(&mods_id, Counter::Count).await?;
    let bans = count_bans(db, &bans_id, id.0).await?;
    let counter = db.counter(&posts_id, Counter::Length).await?;
    let events = db
        .counter(&id.events().map_err(entity_error)?, Counter::Length)
        .await?;

    let op = match db.feed_entry::<EntityId>(&posts_id, 1).await? {
        Some(op) => op.0,
        None => [0u8; 32],
    };

    let mut last_3 = Vec::new();
    if counter > 1 {
        let from = if counter > 4 { counter - 3 + 1 } else { 2 };
        let raw = db.feed_range(&posts_id, from, counter + 1).await?;
        for value in raw.into_iter().flatten() {
            if let Ok(id) = bcs::from_bytes::<EntityId>(&value) {
                last_3.push(id.0);
            }
        }
    }

    Ok(ThreadObject {
        root: entity_root(id.events().map_err(entity_error)?, version, events, id),
        projection: ThreadProjection {
            board,
            number,
            topic_hash,
            op,
            closed,
            deleted,
            admin,
            mods: Table {
                id: mods_id.as_array().to_owned(),
                size: mods_count,
            },
            bans,
            posts: Feed {
                id: posts_id.as_array().to_owned(),
                counter,
            },
            posts_deleted,
            last_3,
        },
    })
}

pub async fn load_posts(db: &EntityReader, ids: &[[u8; 32]]) -> Result<Vec<PostObject>, RelayError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let entity_ids: Vec<EntityId> = ids.iter().map(|id| EntityId(*id)).collect();
    let mut keys: Vec<Vec<u8>> = Vec::with_capacity(entity_ids.len() * 4);
    let events: Vec<CollectionId> = entity_ids
        .iter()
        .map(|id| id.events().map_err(entity_error))
        .collect::<Result<_, _>>()?;
    for id in &entity_ids {
        keys.push(ech_db_entity::EntityKey::record(id));
    }
    for cid in &events {
        keys.push(CollectionKey::source(cid, 1));
    }
    for cid in &events {
        keys.push(CollectionKey::feed_entry(cid, 1));
    }
    for cid in &events {
        keys.push(CollectionKey::length(cid));
    }
    let values = db.state_get(keys).await?;
    let count = entity_ids.len();

    let mut refs: Vec<Option<ech_db_protocol::values::IntentRef>> = Vec::with_capacity(count);
    for index in 0..count {
        let source = values[count + index].clone();
        refs.push(match source {
            None => None,
            Some(bytes) => bcs::from_bytes(&bytes).ok(),
        });
    }
    let mut intent_refs: Vec<([u8; 32], u64)> = Vec::new();
    for reference in refs.iter().flatten() {
        let key = (reference.author, reference.seq);
        if !intent_refs.contains(&key) {
            intent_refs.push(key);
        }
    }
    let intents = db.intents_for(&intent_refs).await?;
    let mut intent_map: HashMap<([u8; 32], u64), &ech_db_protocol::values::AttestedIntent> =
        HashMap::new();
    for (index, key) in intent_refs.iter().enumerate() {
        if let Some(attested) = intents[index].as_ref() {
            intent_map.insert(*key, attested);
        }
    }

    let mut posts = Vec::with_capacity(count);
    for index in 0..count {
        let record_bytes = values[index]
            .as_ref()
            .ok_or_else(|| RelayError::NotFound(format!("post {}", hex::encode(entity_ids[index].0))))?;
        let record = EntityRecord::<PostEntity>::decode(record_bytes, PostEntity::TYPE_NAME)
            .map_err(|error| RelayError::Internal(error.to_string()))?;
        let event = values[2 * count + index]
            .as_ref()
            .and_then(|bytes| bcs::from_bytes::<PostEvent>(bytes).ok())
            .ok_or_else(|| RelayError::Internal("post genesis decode".into()))?;
        let counter = match values[3 * count + index].as_ref() {
            None => 0,
            Some(bytes) => bcs::from_bytes::<u64>(bytes)
                .map_err(|_| RelayError::Internal("event counter decode".into()))?,
        };
        let reference = refs[index].as_ref();
        posts.push(build_post(
            entity_ids[index],
            record,
            event,
            reference,
            &intent_map,
            counter,
        )?);
    }
    Ok(posts)
}

fn build_post(
    id: EntityId,
    record: EntityRecord<PostEntity>,
    event: PostEvent,
    reference: Option<&ech_db_protocol::values::IntentRef>,
    intents: &HashMap<([u8; 32], u64), &ech_db_protocol::values::AttestedIntent>,
    counter: u64,
) -> Result<PostObject, RelayError> {
    let state = record.state;
    let version = state.version();
    let (thread, number, timestamp_ms, name_hash, media_hashes, multi_vote) = match event {
        PostEvent::Genesis {
            thread,
            number,
            timestamp_ms,
            name_hash,
            media_hashes,
            multi_vote,
            ..
        } => (thread, number, timestamp_ms, name_hash, media_hashes, multi_vote),
        _ => return Err(RelayError::Internal("post genesis event mismatch".into())),
    };
    let reference = reference.ok_or_else(|| RelayError::Internal("post source missing".into()))?;
    let attested = intents
        .get(&(reference.author, reference.seq))
        .ok_or_else(|| RelayError::Internal("post intent missing".into()))?;
    let responses: Responses = bcs::from_bytes(&attested.attestation.body.responses)
        .map_err(|_| RelayError::Internal("post responses decode".into()))?;

    let deleted = *state.deleted().map_err(entity_error)?;
    let banned = state.banned().map_err(entity_error)?.clone();
    let text_hash = *state.text_hash().map_err(entity_error)?;
    let banned_media = state.banned_media().map_err(entity_error)?.clone();
    let reactions = state.reactions().map_err(entity_error)?.clone();
    let votes = state.votes().map_err(entity_error)?.clone();
    let mod_note = *state.mod_note().map_err(entity_error)?;

    Ok(PostObject {
        root: entity_root(id.events().map_err(entity_error)?, version, counter, id),
        projection: PostProjection {
            sender: Sender {
                pk: reference.author,
                tweak: attested.intent.body.tweak,
            },
            thread: thread.0,
            number,
            uid: responses.uid.unwrap_or_default(),
            timestamp_ms,
            deleted,
            banned,
            text_hash,
            media_hashes,
            banned_media,
            reactions,
            votes,
            multi_vote,
            name_hash,
            trip: responses.tripcode,
            geo: responses.geo,
            mod_note,
        },
    })
}

pub async fn fetch_content(
    content: &Content,
    kind: ContentKind,
    hashes: HashSet<[u8; 32]>,
) -> HashMap<[u8; 32], Vec<u8>> {
    let results = futures::stream::iter(hashes.into_iter().map(|hash| {
        let content = content.clone();
        async move {
            for attempt in 0..3 {
                match content.get(kind, &hash).await {
                    Ok(Some(data)) => return Some((hash, data)),
                    Ok(None) => return None,
                    Err(_) if attempt < 2 => {
                        tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
                    }
                    Err(_) => return None,
                }
            }
            None
        }
    }))
    .buffer_unordered(32)
    .collect::<Vec<_>>()
    .await;
    results.into_iter().flatten().collect()
}

pub async fn fetch_media_meta(
    content: &Content,
    hashes: HashSet<[u8; 32]>,
) -> HashMap<[u8; 32], MediaMeta> {
    fetch_content(content, ContentKind::MediaMeta, hashes)
        .await
        .into_iter()
        .filter_map(|(hash, data)| {
            bcs::from_bytes::<MediaMeta>(&data)
                .ok()
                .map(|meta| (hash, meta))
        })
        .collect()
}

pub async fn list_mods(db: &EntityReader, id: EntityId) -> Result<Vec<[u8; 32]>, RelayError> {
    let collection = id.collection("mods").map_err(entity_error)?;
    Ok(db.set_members::<[u8; 32]>(&collection).await?)
}

pub async fn moderators(
    db: &EntityReader,
    forum_id: EntityId,
    forum_admin: [u8; 32],
    board: Option<&BoardObject>,
    thread: Option<&ThreadObject>,
) -> Result<Moderators, RelayError> {
    let forum_mods = list_mods(db, forum_id).await?;
    let board_mods = match board {
        Some(board) => list_mods(db, EntityId(board.root.id)).await?,
        None => Vec::new(),
    };
    let thread_mods = match thread {
        Some(thread) => list_mods(db, EntityId(thread.root.id)).await?,
        None => Vec::new(),
    };
    Ok(Moderators {
        forum_admin: Some(forum_admin),
        forum_mods,
        board_mods,
        thread_mods,
        thread_admin: thread.and_then(|thread| thread.projection.admin),
    })
}

pub async fn forum_view(
    db: &EntityReader,
    content: &Content,
    cache: &Cache,
    forum: &ForumObject,
) -> Result<Vec<u8>, RelayError> {
    let key = format!("{CACHE_NS}:forum");
    cache
        .get_or_build(key, async {
            let forum_id = EntityId(forum.root.id);
            let boards_id = forum_id.collection("boards").map_err(entity_error)?;
            let entries = db.map_entries::<Vec<u8>, EntityId>(&boards_id).await?;

            let mut boards = Vec::with_capacity(entries.len());
            for (_, id) in entries {
                let board = load_board(db, id).await?;
                if board.projection.deleted {
                    continue;
                }
                boards.push(board);
            }

            let mut plain_text_hashes = HashSet::new();
            for board in &boards {
                if let Some(hash) = board.projection.description_hash {
                    plain_text_hashes.insert(hash);
                }
            }
            let plain_text = fetch_content(content, ContentKind::PlainText, plain_text_hashes).await;
            let moderators = moderators(db, forum_id, forum.projection.admin, None, None).await?;

            let response = ForumView {
                forum: forum.clone(),
                boards,
                plain_text,
                moderators,
            };
            bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .await
}

pub async fn board_view(
    db: &EntityReader,
    content: &Content,
    cache: &Cache,
    forum: &ForumObject,
    board_id: EntityId,
    cursor: Option<u64>,
) -> Result<Vec<u8>, RelayError> {
    let board = load_board(db, board_id).await?;
    if board.projection.deleted {
        return Err(RelayError::NotFound("board deleted".into()));
    }
    let posts_counter = board.projection.posts.size;
    let generation = cache
        .gen_get(&format!("gen:board:{}", hex::encode(board.root.id)))
        .await;
    let key = format!(
        "{CACHE_NS}:board:{}:{posts_counter}:{generation}:{}",
        hex::encode(board.root.id),
        cursor.unwrap_or(0)
    );
    if let Some(cached) = cache.peek(&key).await {
        return Ok(cached);
    }

    let bumps_id = board_id.collection("bumps").map_err(entity_error)?;
    let counter = board.projection.bumps.counter;
    let mut end = cursor.unwrap_or(counter + 1);
    if end > counter + 1 {
        end = counter + 1;
    }

    let mut seen = HashSet::new();
    let mut thread_ids: Vec<EntityId> = Vec::new();
    for pinned in &board.projection.pinned {
        let id = EntityId(*pinned);
        if seen.insert(id.0) {
            thread_ids.push(id);
        }
    }

    let mut index = end;
    while thread_ids.len() < PAGE_THREADS && index > 1 {
        let chunk_start = if index > BUMP_CHUNK { index - BUMP_CHUNK } else { 1 };
        let raw = db.feed_range(&bumps_id, chunk_start, index).await?;
        let bumps: Vec<EntityId> = raw
            .into_iter()
            .flatten()
            .filter_map(|value| bcs::from_bytes::<EntityId>(&value).ok())
            .collect();
        let mut stop_at = None;
        for (offset, id) in bumps.into_iter().enumerate().rev() {
            if seen.insert(id.0) {
                thread_ids.push(id);
                if thread_ids.len() >= PAGE_THREADS {
                    stop_at = Some(chunk_start + offset as u64);
                    break;
                }
            }
        }
        index = stop_at.unwrap_or(chunk_start);
    }

    let mut threads = Vec::with_capacity(thread_ids.len());
    let mut op_ids = Vec::new();
    let mut reply_ids: Vec<([u8; 32], Vec<[u8; 32]>)> = Vec::new();
    for id in &thread_ids {
        let thread = match load_thread(db, *id).await {
            Ok(thread) => thread,
            Err(RelayError::NotFound(_)) => continue,
            Err(error) => return Err(error),
        };
        if thread.projection.deleted {
            continue;
        }
        op_ids.push(thread.projection.op);
        reply_ids.push((thread.root.id, thread.projection.last_3.clone()));
        threads.push(thread);
    }

    let mut all_posts: Vec<[u8; 32]> = op_ids.clone();
    for (_, replies) in &reply_ids {
        all_posts.extend(replies.iter().copied());
    }
    let loaded = load_posts(db, &all_posts).await?;
    let by_id: HashMap<[u8; 32], PostObject> =
        loaded.into_iter().map(|post| (post.root.id, post)).collect();

    let mut op_posts = HashMap::with_capacity(op_ids.len());
    for (thread, op) in threads.iter().zip(op_ids.iter()) {
        if let Some(post) = by_id.get(op) {
            op_posts.insert(thread.root.id, post.clone());
        }
    }
    let mut last_3 = HashMap::with_capacity(reply_ids.len());
    for (thread, replies) in &reply_ids {
        let mut posts = Vec::with_capacity(replies.len());
        for id in replies {
            if let Some(post) = by_id.get(id) {
                posts.push(post.clone());
            }
        }
        last_3.insert(*thread, posts);
    }

    let preview: Vec<&PostObject> = op_posts
        .values()
        .chain(last_3.values().flat_map(|posts| posts.iter()))
        .collect();
    let text_hashes: HashSet<[u8; 32]> = preview
        .iter()
        .filter(|post| !post.projection.deleted)
        .filter_map(|post| post.projection.text_hash)
        .collect();
    let text = fetch_content(content, ContentKind::Text, text_hashes).await;

    let mut plain_text_hashes = HashSet::new();
    if let Some(hash) = board.projection.description_hash {
        plain_text_hashes.insert(hash);
    }
    for thread in &threads {
        if thread.projection.deleted {
            continue;
        }
        if let Some(hash) = thread.projection.topic_hash {
            plain_text_hashes.insert(hash);
        }
    }
    for post in preview.iter().filter(|post| !post.projection.deleted) {
        if let Some(hash) = post.projection.name_hash {
            plain_text_hashes.insert(hash);
        }
    }
    let plain_text = fetch_content(content, ContentKind::PlainText, plain_text_hashes).await;

    let media_hashes: HashSet<[u8; 32]> = preview
        .iter()
        .filter(|post| !post.projection.deleted)
        .flat_map(|post| post.projection.media_hashes.iter().copied())
        .collect();
    let media_meta = fetch_media_meta(content, media_hashes).await;

    let next_cursor = if index > 1 { Some(index) } else { None };

    let moderators = moderators(
        db,
        EntityId(forum.root.id),
        forum.projection.admin,
        Some(&board),
        None,
    )
    .await?;

    let response = BoardView {
        board,
        threads,
        op_posts,
        last_3,
        text,
        plain_text,
        media_meta,
        next_cursor,
        moderators,
    };
    let bytes = bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))?;
    cache.store(key, &bytes).await;
    Ok(bytes)
}

pub async fn thread_view(
    db: &EntityReader,
    content: &Content,
    cache: &Cache,
    forum: &ForumObject,
    thread_id: EntityId,
) -> Result<Vec<u8>, RelayError> {
    let thread = load_thread(db, thread_id).await?;
    if thread.projection.deleted {
        return Err(RelayError::NotFound("thread deleted".into()));
    }
    let posts_counter = thread.projection.posts.counter;
    let generation = cache
        .gen_get(&format!("gen:thread:{}", hex::encode(thread.root.id)))
        .await;
    let key = format!(
        "{CACHE_NS}:thread:{}:{posts_counter}:{generation}",
        hex::encode(thread.root.id)
    );
    cache
        .get_or_build(key, async {
            let posts_id = thread_id.collection("posts").map_err(entity_error)?;
            let raw = db.feed_range(&posts_id, 1, posts_counter + 1).await?;
            let post_ids: Vec<[u8; 32]> = raw
                .into_iter()
                .flatten()
                .filter_map(|value| bcs::from_bytes::<EntityId>(&value).ok().map(|id| id.0))
                .collect();
            let mut posts = load_posts(db, &post_ids).await?;
            posts.sort_by_key(|post| post.projection.number);

            let board_id = EntityId(thread.projection.board);
            let board = load_board(db, board_id).await?;
            if board.projection.deleted {
                return Err(RelayError::NotFound("board deleted".into()));
            }

            let text_hashes: HashSet<[u8; 32]> = posts
                .iter()
                .filter(|post| !post.projection.deleted)
                .filter_map(|post| post.projection.text_hash)
                .collect();
            let mut plain_text_hashes = HashSet::new();
            if let Some(hash) = thread.projection.topic_hash {
                plain_text_hashes.insert(hash);
            }
            for post in posts.iter().filter(|post| !post.projection.deleted) {
                if let Some(hash) = post.projection.name_hash {
                    plain_text_hashes.insert(hash);
                }
            }
            let media_hashes: HashSet<[u8; 32]> = posts
                .iter()
                .filter(|post| !post.projection.deleted)
                .flat_map(|post| post.projection.media_hashes.iter().copied())
                .collect();

            let (text, plain_text, media_meta) = tokio::join!(
                fetch_content(content, ContentKind::Text, text_hashes),
                fetch_content(content, ContentKind::PlainText, plain_text_hashes),
                fetch_media_meta(content, media_hashes),
            );

            let moderators = moderators(
                db,
                EntityId(forum.root.id),
                forum.projection.admin,
                Some(&board),
                Some(&thread),
            )
            .await?;

            let response = ThreadView {
                thread,
                posts,
                text,
                plain_text,
                media_meta,
                moderators,
            };
            bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .await
}

pub async fn post_view(
    db: &EntityReader,
    content: &Content,
    cache: &Cache,
    forum: &ForumObject,
    post_id: EntityId,
) -> Result<Vec<u8>, RelayError> {
    let key = format!("{CACHE_NS}:post:{}", hex::encode(post_id.0));
    cache
        .get_or_build(key, async {
            let mut posts = load_posts(db, &[post_id.0]).await?;
            let post = posts.pop().ok_or_else(|| RelayError::NotFound("post".into()))?;
            if post.projection.deleted {
                return Err(RelayError::NotFound("post deleted".into()));
            }
            let thread_id = EntityId(post.projection.thread);
            let thread = load_thread(db, thread_id).await?;
            if thread.projection.deleted {
                return Err(RelayError::NotFound("thread deleted".into()));
            }
            let board_id = EntityId(thread.projection.board);
            let board = load_board(db, board_id).await?;
            if board.projection.deleted {
                return Err(RelayError::NotFound("board deleted".into()));
            }

            let mut text_hashes = HashSet::new();
            if let Some(hash) = post.projection.text_hash {
                text_hashes.insert(hash);
            }
            let mut plain_text_hashes = HashSet::new();
            if let Some(hash) = post.projection.name_hash {
                plain_text_hashes.insert(hash);
            }
            if let Some(hash) = thread.projection.topic_hash {
                plain_text_hashes.insert(hash);
            }
            let media_hashes: HashSet<[u8; 32]> =
                post.projection.media_hashes.iter().copied().collect();

            let (text, plain_text, media_meta) = tokio::join!(
                fetch_content(content, ContentKind::Text, text_hashes),
                fetch_content(content, ContentKind::PlainText, plain_text_hashes),
                fetch_media_meta(content, media_hashes),
            );

            let moderators = moderators(
                db,
                EntityId(forum.root.id),
                forum.projection.admin,
                Some(&board),
                Some(&thread),
            )
            .await?;

            let response = PostView {
                post,
                thread,
                board,
                text,
                plain_text,
                media_meta,
                moderators,
            };
            bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .await
}

pub async fn resolve_post(
    db: &EntityReader,
    cache: &Cache,
    board_id: EntityId,
    number: u64,
) -> Result<Vec<u8>, RelayError> {
    let key = format!("{CACHE_NS}:resolvepost:{}:{number}", hex::encode(board_id.0));
    if let Some(cached) = cache.peek(&key).await {
        return Ok(cached);
    }
    let board = load_board(db, board_id).await?;
    if board.projection.deleted {
        return Err(RelayError::NotFound("board deleted".into()));
    }
    let posts_id = board_id.collection("posts").map_err(entity_error)?;
    let post = db.map_get::<u64, EntityId>(&posts_id, &number).await?;
    let id = post.ok_or_else(|| RelayError::NotFound(format!("post {number} not found")))?;
    let bytes = id.0.to_vec();
    cache.store(key, &bytes).await;
    Ok(bytes)
}

pub async fn resolve_thread(
    db: &EntityReader,
    cache: &Cache,
    board_id: EntityId,
    number: u64,
) -> Result<Vec<u8>, RelayError> {
    let key = format!("{CACHE_NS}:resolvethread:{}:{number}", hex::encode(board_id.0));
    if let Some(cached) = cache.peek(&key).await {
        return Ok(cached);
    }
    let board = load_board(db, board_id).await?;
    if board.projection.deleted {
        return Err(RelayError::NotFound("board deleted".into()));
    }
    let threads_id = board_id.collection("threads").map_err(entity_error)?;
    let entry = db.map_get::<u64, EntityId>(&threads_id, &number).await?;
    let id = entry.ok_or_else(|| RelayError::NotFound(format!("thread {number} not found")))?;
    let bytes = id.0.to_vec();
    cache.store(key, &bytes).await;
    Ok(bytes)
}

pub async fn bans_view(
    db: &EntityReader,
    content: &Content,
    cache: &Cache,
    level_id: EntityId,
    cursor: Option<u64>,
) -> Result<Vec<u8>, RelayError> {
    const LIMIT: u64 = 50;
    let key = format!(
        "{CACHE_NS}:bans:{}:{}",
        hex::encode(level_id.0),
        cursor.unwrap_or(0)
    );
    cache
        .get_or_build(key, async {
            let bans_id = level_id.collection("bans").map_err(entity_error)?;
            let entries = db.map_entries::<BanKey, BanValue>(&bans_id).await?;
            let mut all: Vec<BanEntryView> = entries
                .into_iter()
                .map(|(key, value)| BanEntryView {
                    mask: key.mask,
                    ip_hash: key.ip_hash,
                    reason_hash: value.reason_hash,
                    reason: None,
                    expires: value.expires,
                })
                .collect();
            all.sort_by(|left, right| {
                (left.mask, hex::encode(left.ip_hash)).cmp(&(right.mask, hex::encode(right.ip_hash)))
            });

            let reason_hashes: HashSet<[u8; 32]> =
                all.iter().map(|entry| entry.reason_hash).collect();
            let plain = fetch_content(content, ContentKind::PlainText, reason_hashes).await;

            for entry in all.iter_mut() {
                entry.reason = plain
                    .get(&entry.reason_hash)
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            }

            let total = all.len();
            let offset = cursor.unwrap_or(0) as usize;
            let page: Vec<BanEntryView> = all.into_iter().skip(offset).take(LIMIT as usize).collect();
            let next_cursor = if offset + page.len() < total {
                Some((offset + page.len()) as u64)
            } else {
                None
            };
            let response = BansView {
                level: level_id.0,
                bans: page,
                next_cursor,
            };
            bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .await
}

pub async fn feed_view(
    db: &EntityReader,
    cache: &Cache,
    feed_id: CollectionId,
    counter: u64,
    cursor: Option<u64>,
) -> Result<Vec<u8>, RelayError> {
    const LIMIT: u64 = 20;
    let key = format!(
        "{CACHE_NS}:feed:{}:{}:{}",
        hex::encode(feed_id.as_array()),
        counter,
        cursor.unwrap_or(0)
    );
    cache
        .get_or_build(key, async {
            let end = cursor.unwrap_or(counter + 1);
            let start = if end > LIMIT { end - LIMIT } else { 1 };
            let raw = db.feed_range(&feed_id, start, end).await?;
            let mut items: Vec<Vec<u8>> = raw.into_iter().flatten().collect();
            items.reverse();
            let next_cursor = if start > 1 { Some(start) } else { None };
            let response = FeedView { items, next_cursor };
            bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .await
}

pub async fn reactions_view(
    db: &EntityReader,
    cache: &Cache,
    queries: Vec<([u8; 32], [u8; 32])>,
) -> Result<Vec<u8>, RelayError> {
    let mut out = Vec::new();
    for (post_id, pk) in queries {
        let key = format!(
            "{CACHE_NS}:reactions:{}:{}",
            hex::encode(post_id),
            hex::encode(pk)
        );
        if let Some(cached) = cache.peek(&key).await {
            if let Ok(view) = bcs::from_bytes::<PostReactionsView>(&cached) {
                if let Some(reaction) = view.reaction {
                    out.push((post_id, reaction));
                }
                continue;
            }
        }
        let reaction = find_reaction(db, EntityId(post_id), pk).await?;
        let bytes = bcs::to_bytes(&PostReactionsView { reaction })
            .map_err(|error| RelayError::Internal(error.to_string()))?;
        cache.store(key, &bytes).await;
        if let Some(reaction) = reaction {
            out.push((post_id, reaction));
        }
    }
    bcs::to_bytes(&out).map_err(|error| RelayError::Internal(error.to_string()))
}

async fn find_reaction(
    db: &EntityReader,
    post_id: EntityId,
    pk: [u8; 32],
) -> Result<Option<[u8; 32]>, RelayError> {
    let reacted = post_id.collection("reacted").map_err(entity_error)?;
    let entry = db.map_get::<[u8; 32], MarkEntry>(&reacted, &pk).await?;
    Ok(entry.and_then(|entry| entry.options.first().copied()))
}
