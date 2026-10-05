use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use blake2b_simd::Params;
use bytes::Bytes;
use serde::de::DeserializeOwned;

use ech_db_entity::{CollectionKey, EntityId, EntityReader};
use ech_db_protocol::values::{IntentBody, SignedIntent};
use forum_model::*;

use crate::error::RelayError;
use crate::state::AppState;
use crate::types::{AppliedEvent, Batch, ContentKind, EntityKind, SendResponse};
use crate::views;

pub struct SendInputs {
    pub intents: Vec<SignedIntent>,
    pub text: Option<Bytes>,
    pub media: Vec<PathBuf>,
    pub description: Option<String>,
    pub topic: Option<String>,
    pub reason: Option<String>,
    pub name: Option<String>,
    pub tripcode: Option<String>,
    pub captcha: Option<String>,
    pub remote_ip: Ipv4Addr,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentUse {
    None,
    Post,
    Text,
    Topic,
    Description,
    Reason,
}

struct Plan {
    signed: SignedIntent,
    board: Option<EntityId>,
    thread: Option<EntityId>,
    post: Option<EntityId>,
    content: ContentUse,
    captcha: bool,
}

type Pending = Vec<(ContentKind, [u8; 32], Vec<u8>)>;

fn decode2<A: DeserializeOwned, B: DeserializeOwned>(body: &IntentBody) -> Result<(A, B), RelayError> {
    bcs::from_bytes(&body.call.args).map_err(|error| RelayError::BadRequest(format!("intent args: {error}")))
}

fn decode3<A: DeserializeOwned, B: DeserializeOwned, C: DeserializeOwned>(
    body: &IntentBody,
) -> Result<(A, B, C), RelayError> {
    bcs::from_bytes(&body.call.args).map_err(|error| RelayError::BadRequest(format!("intent args: {error}")))
}

fn decode4<A: DeserializeOwned, B: DeserializeOwned, C: DeserializeOwned, D: DeserializeOwned>(
    body: &IntentBody,
) -> Result<(A, B, C, D), RelayError> {
    bcs::from_bytes(&body.call.args).map_err(|error| RelayError::BadRequest(format!("intent args: {error}")))
}

fn decode5<
    A: DeserializeOwned,
    B: DeserializeOwned,
    C: DeserializeOwned,
    D: DeserializeOwned,
    E: DeserializeOwned,
>(
    body: &IntentBody,
) -> Result<(A, B, C, D, E), RelayError> {
    bcs::from_bytes(&body.call.args).map_err(|error| RelayError::BadRequest(format!("intent args: {error}")))
}

fn decode_args<T: DeserializeOwned>(body: &IntentBody) -> Result<T, RelayError> {
    bcs::from_bytes(&body.call.args).map_err(|error| RelayError::BadRequest(format!("intent args: {error}")))
}

fn plan(signed: SignedIntent, program: [u8; 32], root: [u8; 32]) -> Result<Plan, RelayError> {
    if signed.body.root != root {
        return Err(RelayError::Unauthorized("intent root mismatch".into()));
    }
    if signed.body.call.program_hash != program {
        return Err(RelayError::BadRequest("unknown program".into()));
    }
    if !signed.verify() {
        return Err(RelayError::Unauthorized("intent signature invalid".into()));
    }
    let body = &signed.body;
    let mut plan = Plan {
        signed: signed.clone(),
        board: None,
        thread: None,
        post: None,
        content: ContentUse::None,
        captcha: false,
    };
    match body.call.function.as_str() {
        "bootstrap" | "read_forum" | "find_board" | "read_board" | "read_thread" | "read_post"
        | "upgrade_forum" | "upgrade_board" | "upgrade_thread" | "upgrade_post"
        | "forum_add_moderator" | "forum_del_moderator" | "forum_set_timestamp_precision" => {}
        "forum_ban" | "forum_unban" => {
            plan.content = ContentUse::Reason;
        }
        "board_ban" => {
            let (board, _, _): (EntityId, BanKey, BanValue) = decode3(body)?;
            plan.board = Some(board);
            plan.content = ContentUse::Reason;
        }
        "board_unban" => {
            let (board, _): (EntityId, BanKey) = decode2(body)?;
            plan.board = Some(board);
        }
        "thread_ban" => {
            let (board, thread, _, _): (EntityId, EntityId, BanKey, BanValue) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.content = ContentUse::Reason;
        }
        "thread_unban" => {
            let (board, thread, _): (EntityId, EntityId, BanKey) = decode3(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
        }
        "forum_new_board" => {
            plan.content = ContentUse::Description;
        }
        "forum_ban_post" | "board_ban_post" | "thread_ban_post" => {
            let (board, thread, post, _, _): (EntityId, EntityId, EntityId, BanKey, BanValue) =
                decode5(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
            plan.content = ContentUse::Reason;
        }
        "forum_unban_post" | "board_unban_post" | "thread_unban_post" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, BanKey) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        "board_add_moderator" | "board_del_moderator" => {
            let (board, _): (EntityId, Pubkey) = decode2(body)?;
            plan.board = Some(board);
        }
        "board_set_max_media" | "board_set_bump_limit" => {
            let (board, _): (EntityId, u64) = decode2(body)?;
            plan.board = Some(board);
        }
        "board_set_closed" | "board_set_deleted" | "board_set_ignore_forum_bans" => {
            let (board, _): (EntityId, bool) = decode2(body)?;
            plan.board = Some(board);
        }
        "board_set_description" => {
            let (board, _): (EntityId, Option<Hash>) = decode2(body)?;
            plan.board = Some(board);
            plan.content = ContentUse::Description;
        }
        "board_set_reactions" => {
            let (board, _): (EntityId, Vec<Hash>) = decode2(body)?;
            plan.board = Some(board);
        }
        "board_set_pinned" => {
            let (board, _): (EntityId, Vec<EntityId>) = decode2(body)?;
            plan.board = Some(board);
        }
        "new_thread" => {
            let args: NewThreadArgs = decode_args(body)?;
            plan.board = Some(args.board);
            plan.content = ContentUse::Post;
            plan.captcha = true;
        }
        "new_post" => {
            let args: NewPostArgs = decode_args(body)?;
            plan.board = Some(args.board);
            plan.thread = Some(args.thread);
            plan.content = ContentUse::Post;
            plan.captcha = true;
        }
        "thread_add_moderator" | "thread_del_moderator" => {
            let (board, thread, _): (EntityId, EntityId, Pubkey) = decode3(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
        }
        "thread_set_closed" | "thread_set_deleted" => {
            let (board, thread, _): (EntityId, EntityId, bool) = decode3(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
        }
        "thread_set_admin" => {
            let (board, thread, _): (EntityId, EntityId, Option<Pubkey>) = decode3(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
        }
        "thread_set_topic" => {
            let (board, thread, _): (EntityId, EntityId, Option<Hash>) = decode3(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.content = ContentUse::Topic;
        }
        "set_post_deleted" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, bool) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        "set_post_text" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, Option<Hash>) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
            plan.content = ContentUse::Text;
        }
        "ban_media" | "unban_media" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, Vec<Hash>) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        "set_reaction_v2" => {
            let (board, thread, post, _, _): (EntityId, EntityId, EntityId, Option<Hash>, Hash) =
                decode5(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        "vote_v2" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, Vec<Hash>) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        "set_mod_note" => {
            let (board, thread, post, _): (EntityId, EntityId, EntityId, Option<Hash>) = decode4(body)?;
            plan.board = Some(board);
            plan.thread = Some(thread);
            plan.post = Some(post);
        }
        other => return Err(RelayError::BadRequest(format!("unsupported function {other}"))),
    }
    Ok(plan)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn send(state: &AppState, inputs: SendInputs) -> Result<Vec<u8>, RelayError> {
    let SendInputs {
        intents,
        text,
        media,
        description,
        topic,
        reason,
        name,
        tripcode,
        captcha,
        remote_ip,
    } = inputs;
    let mut plans = Vec::with_capacity(intents.len());
    for signed in intents {
        plans.push(plan(signed, state.program, state.root)?);
    }
    if plans.is_empty() {
        return Err(RelayError::BadRequest("no intents provided".into()));
    }

    if plans.iter().any(|plan| plan.captcha) && state.captcha.required() {
        let token = captcha
            .as_deref()
            .ok_or_else(|| RelayError::BadRequest("captcha requested but not provided".into()))?;
        state.captcha.verify(token, &remote_ip.to_string()).await?;
    }

    let ip = u32::from(remote_ip);
    let mask_hashes = state.secrets.mask_hashes(ip);
    let uid = state.secrets.uid(&mask_hashes)?;
    let geo = state.geoip.country_code(IpAddr::V4(remote_ip));
    let trip = match tripcode {
        Some(ref raw) => Some(crate::tripcode::resolve(raw, &state.tripcode_key)?),
        None => None,
    };

    let plans_ref = &plans;
    let mask_ref = &mask_hashes;
    let text_ref = &text;
    let media_ref = &media;
    let description_ref = description.as_deref();
    let topic_ref = topic.as_deref();
    let reason_ref = reason.as_deref();
    let name_ref = name.as_deref();
    let pending: Pending = state
        .db
        .read(move |read| async move {
            check_bans(state, &read, plans_ref, mask_ref).await?;
            let mut pending = Vec::new();
            for plan in plans_ref {
                verify_content(
                    state,
                    &read,
                    plan,
                    text_ref,
                    media_ref,
                    description_ref,
                    topic_ref,
                    reason_ref,
                    name_ref,
                    &mut pending,
                )
                .await?;
            }
            Ok(pending)
        })
        .await?;

    let mut touched: Vec<(EntityKind, [u8; 32])> = Vec::new();
    add_touched(EntityKind::Forum, state.forum_id, &mut touched);
    for plan in &plans {
        if let Some(board) = plan.board {
            add_touched(EntityKind::Board, board, &mut touched);
        }
        if let Some(thread) = plan.thread {
            add_touched(EntityKind::Thread, thread, &mut touched);
        }
        if let Some(post) = plan.post {
            add_touched(EntityKind::Post, post, &mut touched);
        }
    }
    let mut writes = Vec::new();
    let mut created: Vec<[u8; 32]> = Vec::new();
    for plan in &plans {
        let function = plan.signed.body.call.function.clone();
        let responses = responses_for(state, &plan.signed, &uid, &trip, geo, ip)?;
        let outcome = state.db.append_user(&plan.signed, responses).await?;
        writes.extend(outcome.writes);
        let output = outcome.output;
        if function == "new_thread" {
            let result: NewThreadResult = bcs::from_bytes(&output)
                .map_err(|error| RelayError::Internal(format!("new_thread output: {error}")))?;
            add_touched(EntityKind::Thread, result.thread, &mut touched);
            add_touched(EntityKind::Post, result.post, &mut touched);
            created.push(result.thread.0);
            created.push(result.post.0);
        } else if function == "new_post" {
            let id: EntityId = bcs::from_bytes(&output)
                .map_err(|error| RelayError::Internal(format!("new_post output: {error}")))?;
            add_touched(EntityKind::Post, id, &mut touched);
            created.push(id.0);
        } else if function == "forum_new_board" {
            let id: EntityId = bcs::from_bytes(&output)
                .map_err(|error| RelayError::Internal(format!("forum_new_board output: {error}")))?;
            add_touched(EntityKind::Board, id, &mut touched);
            created.push(id.0);
        }
    }

    for (kind, hash, data) in pending {
        state.content.put_bytes(kind, &hash, data).await?;
    }

    let mut events = Vec::new();
    for (kind, id) in &touched {
        events.extend(AppliedEvent::for_entity(*kind, *id, &writes)?);
    }
    invalidate(state, &plans).await;
    state.realtime.publish(Batch {
        forum: Some(state.forum_id.0),
        board: plans.iter().find_map(|plan| plan.board.map(|id| id.0)),
        thread: plans.iter().find_map(|plan| plan.thread.map(|id| id.0)),
        post: plans.iter().find_map(|plan| plan.post.map(|id| id.0)),
        events: events.clone(),
    });

    let encoded = plans
        .iter()
        .map(|plan| {
            bcs::to_bytes(&plan.signed).map_err(|error| RelayError::Internal(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let digest = Params::new()
        .hash_length(32)
        .hash(&encoded.concat());
    let response = SendResponse {
        accepted_by: vec![state.db.endpoint().to_string()],
        digest: hex::encode(digest.as_bytes()),
        events,
        created,
    };
    bcs::to_bytes(&response).map_err(|error| RelayError::Internal(error.to_string()))
}

fn add_touched(kind: EntityKind, id: EntityId, touched: &mut Vec<(EntityKind, [u8; 32])>) {
    if !touched.iter().any(|(_, known)| *known == id.0) {
        touched.push((kind, id.0));
    }
}

fn responses_for(
    state: &AppState,
    signed: &SignedIntent,
    uid: &[u8],
    trip: &Option<Tripcode>,
    geo: Option<u32>,
    ip: u32,
) -> Result<Vec<u8>, RelayError> {
    let ip32 = match signed.body.call.function.as_str() {
        "set_reaction_v2" => {
            let (_, _, post, _, _): (EntityId, EntityId, EntityId, Option<Hash>, Hash) =
                decode5(&signed.body)?;
            Some(state.secrets.ip32(ip, Some(&post.0)))
        }
        "vote_v2" => {
            let (_, _, post, _): (EntityId, EntityId, EntityId, Vec<Hash>) = decode4(&signed.body)?;
            Some(state.secrets.ip32(ip, Some(&post.0)))
        }
        _ => None,
    };
    let responses = Responses {
        uid: Some(uid.to_vec()),
        ip32,
        tripcode: trip.clone(),
        geo,
    };
    bcs::to_bytes(&responses).map_err(|error| RelayError::Internal(error.to_string()))
}

async fn check_bans(
    state: &AppState,
    read: &EntityReader,
    plans: &[Plan],
    mask_hashes: &[[u8; 32]; 4],
) -> Result<(), RelayError> {
    let now = now_ms();
    let mut levels: Vec<EntityId> = Vec::new();
    let mut ignore_forum = false;
    for plan in plans {
        if let Some(board) = plan.board {
            let board_object = views::load_board(read, board).await?;
            if board_object.projection.ignore_forum_bans {
                ignore_forum = true;
            }
            if !levels.iter().any(|known| *known == board) {
                levels.push(board);
            }
        }
        if let Some(thread) = plan.thread {
            if !levels.iter().any(|known| *known == thread) {
                levels.push(thread);
            }
        }
    }
    if !ignore_forum {
        levels.insert(0, state.forum_id);
    }

    let mut keys = Vec::new();
    for level in &levels {
        let bans = level.collection("bans")?;
        for (index, mask) in [32u8, 24, 20, 16].iter().enumerate() {
            let ip_hash = state.secrets.ban_digest(*mask, &mask_hashes[index]);
            keys.push(CollectionKey::entry(
                &bans,
                &BanKey {
                    level: *level,
                    mask: *mask,
                    ip_hash,
                },
            )?);
        }
    }
    let values = read.state_get(keys).await?;
    for value in values.iter().flatten() {
        let ban: BanValue =
            bcs::from_bytes(value).map_err(|_| RelayError::Internal("ban decode".into()))?;
        if ban.expires > now {
            return Err(RelayError::Banned);
        }
    }
    Ok(())
}

async fn verify_content(
    state: &AppState,
    read: &EntityReader,
    plan: &Plan,
    text: &Option<Bytes>,
    media: &[PathBuf],
    description: Option<&str>,
    topic: Option<&str>,
    reason: Option<&str>,
    name: Option<&str>,
    pending: &mut Pending,
) -> Result<(), RelayError> {
    match plan.content {
        ContentUse::None => Ok(()),
        ContentUse::Post => {
            let body = &plan.signed.body;
            let (text_hash, media_hashes, name_hash, topic_hash) = match body.call.function.as_str() {
                "new_thread" => {
                    let args: NewThreadArgs = decode_args(body)?;
                    (args.text_hash, args.media_hashes, args.name_hash, args.topic_hash)
                }
                "new_post" => {
                    let args: NewPostArgs = decode_args(body)?;
                    (args.text_hash, args.media_hashes, args.name_hash, None)
                }
                other => return Err(RelayError::Internal(format!("unexpected post function {other}"))),
            };
            if let Some(board) = plan.board {
                let board_object = views::load_board(read, board).await?;
                if media_hashes.len() as u64 > board_object.projection.max_media {
                    return Err(RelayError::BadRequest(format!(
                        "media count {} exceeds board max_media {}",
                        media_hashes.len(),
                        board_object.projection.max_media
                    )));
                }
            }
            verify_text(state, text, &text_hash, pending)?;
            verify_media(state, media, &media_hashes, pending).await?;
            verify_topic(topic, &topic_hash)?;
            verify_plaintext(&name_hash, name, pending)
        }
        ContentUse::Text => {
            let (_, _, _, text_hash): (EntityId, EntityId, EntityId, Option<Hash>) =
                decode4(&plan.signed.body)?;
            verify_text(state, text, &text_hash, pending)
        }
        ContentUse::Topic => {
            let (_, _, topic_hash): (EntityId, EntityId, Option<Hash>) = decode3(&plan.signed.body)?;
            verify_topic(topic, &topic_hash)
        }
        ContentUse::Description => {
            let description_hash = match plan.signed.body.call.function.as_str() {
                "forum_new_board" => decode_args::<NewBoardArgs>(&plan.signed.body)?.desc_hash,
                _ => {
                    let (_, hash): (EntityId, Option<Hash>) = decode2(&plan.signed.body)?;
                    hash
                }
            };
            verify_plaintext(&description_hash, description, pending)
        }
        ContentUse::Reason => {
            let Some(reason) = reason else {
                return Ok(());
            };
            let reason_hash = declared_reason(&plan.signed.body)?;
            verify_plaintext(&Some(reason_hash), Some(reason), pending)
        }
    }
}

fn declared_reason(body: &IntentBody) -> Result<[u8; 32], RelayError> {
    match body.call.function.as_str() {
        "forum_ban" | "board_ban" | "thread_ban" => {
            let (_, value): (BanKey, BanValue) = decode2(body)?;
            Ok(value.reason_hash)
        }
        "forum_ban_post" | "board_ban_post" | "thread_ban_post" => {
            let (_, _, _, _, value): (EntityId, EntityId, EntityId, BanKey, BanValue) =
                decode5(body)?;
            Ok(value.reason_hash)
        }
        other => Err(RelayError::Internal(format!("unexpected ban function {other}"))),
    }
}

fn verify_text(
    state: &AppState,
    text: &Option<Bytes>,
    declared: &Option<Hash>,
    pending: &mut Pending,
) -> Result<(), RelayError> {
    match (declared, text) {
        (None, None) => Ok(()),
        (Some(hash), Some(blob)) => {
            if blob.len() > state.limits.max_text_size {
                return Err(RelayError::BadRequest(format!(
                    "text size {} exceeds max {}",
                    blob.len(),
                    state.limits.max_text_size
                )));
            }
            let _parts: Vec<crate::postparts::PostPart> = bcs::from_bytes(blob)
                .map_err(|error| RelayError::BadRequest(format!("invalid PostPart bcs: {error}")))?;
            verify_hash(hash, blob)?;
            pending.push((ContentKind::Text, *hash, blob.to_vec()));
            Ok(())
        }
        (Some(_), None) => Err(RelayError::BadRequest(
            "text_hash present but no text content provided".into(),
        )),
        (None, Some(_)) => Err(RelayError::BadRequest(
            "text provided but intent text_hash is None".into(),
        )),
    }
}

fn verify_topic(topic: Option<&str>, declared: &Option<Hash>) -> Result<(), RelayError> {
    match (declared, topic) {
        (None, None) => Ok(()),
        (Some(_), Some(value)) => {
            if value.len() > 150 {
                return Err(RelayError::BadRequest("topic exceeds 150 chars".into()));
            }
            Ok(())
        }
        (Some(_), None) => Err(RelayError::BadRequest(
            "topic hash present but no content provided".into(),
        )),
        (None, Some(_)) => Err(RelayError::BadRequest(
            "topic provided but intent hash is None".into(),
        )),
    }
}

async fn verify_media(
    state: &AppState,
    media: &[PathBuf],
    declared: &[Hash],
    pending: &mut Pending,
) -> Result<(), RelayError> {
    if media.len() != declared.len() {
        return Err(RelayError::BadRequest(format!(
            "media count mismatch: {} blobs vs {} hashes",
            media.len(),
            declared.len()
        )));
    }
    for (path, hash) in media.iter().zip(declared.iter()) {
        let metadata = std::fs::metadata(path)
            .map_err(|error| RelayError::BadRequest(format!("media: {error}")))?;
        if metadata.len() > state.limits.max_upload_bytes {
            return Err(RelayError::BadRequest("media file too large".into()));
        }
        let data = tokio::fs::read(path)
            .await
            .map_err(|error| RelayError::BadRequest(format!("media: {error}")))?;
        verify_hash(hash, &data)?;
        let file_type = crate::thumbnails::validate(&data)?;
        let meta = crate::thumbnails::compute_meta(&data, path)?;
        let meta_bytes =
            bcs::to_bytes(&meta).map_err(|error| RelayError::Internal(error.to_string()))?;
        if file_type.supports_thumbnail() {
            let thumb = crate::thumbnails::generate(&data, path)?;
            pending.push((ContentKind::Thumbnail, *hash, thumb));
        }
        pending.push((ContentKind::Media, *hash, data));
        pending.push((ContentKind::MediaMeta, *hash, meta_bytes));
    }
    Ok(())
}

fn verify_plaintext(
    declared: &Option<Hash>,
    value: Option<&str>,
    pending: &mut Pending,
) -> Result<(), RelayError> {
    match (declared, value) {
        (Some(hash), Some(text)) => {
            let data = text.as_bytes();
            verify_hash(hash, data)?;
            pending.push((ContentKind::PlainText, *hash, data.to_vec()));
            Ok(())
        }
        (None, None) => Ok(()),
        (Some(_), None) => Err(RelayError::BadRequest(
            "plaintext hash present but no content provided".into(),
        )),
        (None, Some(_)) => Err(RelayError::BadRequest(
            "plaintext provided but intent hash is None".into(),
        )),
    }
}

fn verify_hash(expected: &Hash, blob: &[u8]) -> Result<(), RelayError> {
    if Params::new().hash_length(32).hash(blob).as_bytes() != expected.as_slice() {
        return Err(RelayError::BadRequest("content hash mismatch".into()));
    }
    Ok(())
}

async fn invalidate(state: &AppState, plans: &[Plan]) {
    let mut flush = false;
    let mut gens: Vec<String> = Vec::new();
    let mut deletes: Vec<String> = Vec::new();
    let mut patterns: Vec<String> = Vec::new();
    let mut scopes: Vec<String> = Vec::new();

    for plan in plans {
        let function = plan.signed.body.call.function.as_str();
        let author = hex::encode(plan.signed.body.author);
        match function {
            "new_thread" | "new_post" => {
                deletes.push("v4:forum".to_string());
                scopes.push("v4:forum".to_string());
            }
            "set_reaction_v2" | "vote_v2" => {
                if let (Some(board), Some(thread), Some(post)) = (plan.board, plan.thread, plan.post)
                {
                    gens.push(format!("gen:thread:{}", hex::encode(thread.0)));
                    gens.push(format!("gen:board:{}", hex::encode(board.0)));
                    deletes.push(format!("v4:post:{}", hex::encode(post.0)));
                    deletes.push(format!("v4:reactions:{}:{author}", hex::encode(post.0)));
                    scopes.push(format!("v4:post:{}", hex::encode(post.0)));
                    scopes.push(format!("v4:reactions:{}:{author}", hex::encode(post.0)));
                }
            }
            "thread_set_topic" => {
                if let Some(thread) = plan.thread {
                    patterns.push(format!("v4:thread:{}:*", hex::encode(thread.0)));
                    scopes.push(format!("v4:thread:{}", hex::encode(thread.0)));
                }
                if let Some(board) = plan.board {
                    patterns.push(format!("v4:board:{}:*", hex::encode(board.0)));
                    scopes.push(format!("v4:board:{}", hex::encode(board.0)));
                }
            }
            "set_post_text" | "set_post_deleted" | "ban_media" | "unban_media" | "set_mod_note" => {
                if let (Some(board), Some(thread), Some(post)) = (plan.board, plan.thread, plan.post)
                {
                    deletes.push(format!("v4:post:{}", hex::encode(post.0)));
                    patterns.push(format!("v4:thread:{}:*", hex::encode(thread.0)));
                    patterns.push(format!("v4:board:{}:*", hex::encode(board.0)));
                    scopes.push(format!("v4:post:{}", hex::encode(post.0)));
                    scopes.push(format!("v4:thread:{}", hex::encode(thread.0)));
                    scopes.push(format!("v4:board:{}", hex::encode(board.0)));
                }
            }
            "forum_ban" | "forum_unban" | "forum_ban_post" | "forum_unban_post" => {
                patterns.push(format!("v4:bans:{}:*", hex::encode(state.forum_id.0)));
                scopes.push(format!("v4:bans:{}", hex::encode(state.forum_id.0)));
            }
            "board_ban" | "board_unban" | "board_ban_post" | "board_unban_post" => {
                if let Some(board) = plan.board {
                    patterns.push(format!("v4:bans:{}:*", hex::encode(board.0)));
                    scopes.push(format!("v4:bans:{}", hex::encode(board.0)));
                }
            }
            "thread_ban" | "thread_unban" | "thread_ban_post" | "thread_unban_post" => {
                if let Some(thread) = plan.thread {
                    patterns.push(format!("v4:bans:{}:*", hex::encode(thread.0)));
                    scopes.push(format!("v4:bans:{}", hex::encode(thread.0)));
                }
            }
            "board_set_closed" | "board_set_deleted" | "board_set_description"
            | "board_set_pinned" | "board_set_reactions" | "board_set_max_media"
            | "board_set_bump_limit" | "board_set_ignore_forum_bans" => {
                if let Some(board) = plan.board {
                    patterns.push(format!("v4:board:{}:*", hex::encode(board.0)));
                    scopes.push(format!("v4:board:{}", hex::encode(board.0)));
                }
            }
            "thread_set_closed" | "thread_set_deleted" | "thread_set_admin" => {
                if let Some(thread) = plan.thread {
                    patterns.push(format!("v4:thread:{}:*", hex::encode(thread.0)));
                    scopes.push(format!("v4:thread:{}", hex::encode(thread.0)));
                }
            }
            _ => {
                flush = true;
            }
        }
    }

    let invalidation = crate::cache::Invalidation {
        flush,
        scopes: if flush { Vec::new() } else { scopes.clone() },
    };

    if !flush {
        let mut futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> =
            Vec::new();
        for key in &gens {
            let key = key.clone();
            let cache = state.cache.clone();
            futures.push(Box::pin(async move {
                if let Err(error) = cache.incr_gen(&key).await {
                    eprintln!("cache incr {key}: {error}");
                }
            }));
        }
        if !deletes.is_empty() {
            let keys = deletes.clone();
            let cache = state.cache.clone();
            futures.push(Box::pin(async move {
                if let Err(error) = cache.delete_keys(&keys).await {
                    eprintln!("cache del {keys:?}: {error}");
                }
            }));
        }
        for pattern in &patterns {
            let pattern = pattern.clone();
            let cache = state.cache.clone();
            futures.push(Box::pin(async move {
                if let Err(error) = cache.delete_pattern(&pattern).await {
                    eprintln!("cache pattern del {pattern}: {error}");
                }
            }));
        }
        futures::future::join_all(futures).await;
    } else if let Err(error) = state.cache.flush_all().await {
        eprintln!("cache flush: {error}");
    }

    state.cache.apply_local(&invalidation);
    if let Err(error) = state.cache.publish(&invalidation).await {
        eprintln!("cache publish: {error}");
    }
}
