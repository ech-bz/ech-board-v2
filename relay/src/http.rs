use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use ech_db_entity::EntityId;
use ech_db_protocol::values::SignedIntent;

use crate::error::RelayError;
use crate::send::SendInputs;
use crate::state::AppState;
use crate::types::{BanHashes, ContentKind, EntityKind, InfoResponse, NonceInfo};
use crate::views;

const MAX_BODY: usize = 200 * 1024 * 1024;

pub fn router(state: Arc<AppState>) -> (Router, Router) {
    let public = Router::new()
        .route("/healthz", get(healthz))
        .route("/info", get(info))
        .route("/nonce/{author}", get(nonce))
        .route("/forum", get(forum_view))
        .route("/board/{id}", get(board_view))
        .route("/board/{id}/post/{number}", get(resolve_post))
        .route("/board/{id}/thread/{number}", get(resolve_thread))
        .route("/thread/{id}", get(thread_view))
        .route("/post/{id}", get(post_view))
        .route("/reactions", post(reactions))
        .route("/feed/{id}", get(feed))
        .route("/bans/{id}", get(bans))
        .route("/decrypt", post(decrypt))
        .route("/ban/{post}", post(ban_hashes))
        .route("/send", post(send_intents))
        .route("/content/{board}/{thread}/{post}/{kind}/{hash}", get(content))
        .route("/content/{board}/reaction/{hash}", get(reaction_content).put(reaction_put))
        .route("/sse/forum/{id}", get(sse_forum))
        .route("/sse/board/{id}", get(sse_board))
        .route("/sse/thread/{id}", get(sse_thread))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let admin = Router::new()
        .route("/add_moderator", post(add_moderator))
        .route("/del_moderator", post(del_moderator))
        .with_state(state);
    (public, admin)
}

async fn healthz() -> Response {
    StatusCode::OK.into_response()
}

fn parse_id(text: &str) -> Result<[u8; 32], RelayError> {
    let stripped = text.trim_start_matches("0x");
    let bytes = hex::decode(stripped).map_err(|_| RelayError::BadRequest("invalid id".into()))?;
    bytes
        .try_into()
        .map_err(|_| RelayError::BadRequest("invalid id length".into()))
}

async fn info(State(state): State<Arc<AppState>>) -> Result<Response, RelayError> {
    let response = InfoResponse {
        forum: state.forum_id.0,
        program: state.program,
        admin: state.root,
    };
    Ok(bcs_response(&response)?)
}

async fn nonce(
    State(state): State<Arc<AppState>>,
    Path(author): Path<String>,
) -> Result<Response, RelayError> {
    let author = parse_id(&author)?;
    let nonce = state.db.nonce(&author).await?;
    Ok(bcs_response(&NonceInfo { nonce })?)
}

async fn forum_view(State(state): State<Arc<AppState>>) -> Result<Response, RelayError> {
    let forum_id = state.forum_id;
    let content = &state.content;
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| async move {
            let forum = views::load_forum(&read, forum_id).await?;
            views::forum_view(&read, content, cache, &forum).await
        })
        .await?;
    Ok(octet_response(bytes))
}

async fn board_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<CursorQuery>,
) -> Result<Response, RelayError> {
    let board_id = EntityId(parse_id(&id)?);
    let forum_id = state.forum_id;
    let content = &state.content;
    let cache = &state.cache;
    let cursor = query.cursor;
    let bytes = state
        .db
        .read(move |read| async move {
            let forum = views::load_forum(&read, forum_id).await?;
            views::board_view(&read, content, cache, &forum, board_id, cursor).await
        })
        .await?;
    Ok(octet_response(bytes))
}

async fn resolve_post(
    State(state): State<Arc<AppState>>,
    Path((id, number)): Path<(String, u64)>,
) -> Result<Response, RelayError> {
    let board_id = EntityId(parse_id(&id)?);
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| async move { views::resolve_post(&read, cache, board_id, number).await })
        .await?;
    Ok(octet_response(bytes))
}

async fn resolve_thread(
    State(state): State<Arc<AppState>>,
    Path((id, number)): Path<(String, u64)>,
) -> Result<Response, RelayError> {
    let board_id = EntityId(parse_id(&id)?);
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| async move { views::resolve_thread(&read, cache, board_id, number).await })
        .await?;
    Ok(octet_response(bytes))
}

async fn thread_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, RelayError> {
    let thread_id = EntityId(parse_id(&id)?);
    let forum_id = state.forum_id;
    let content = &state.content;
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| async move {
            let forum = views::load_forum(&read, forum_id).await?;
            views::thread_view(&read, content, cache, &forum, thread_id).await
        })
        .await?;
    Ok(octet_response(bytes))
}

async fn post_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, RelayError> {
    let post_id = EntityId(parse_id(&id)?);
    let forum_id = state.forum_id;
    let content = &state.content;
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| async move {
            let forum = views::load_forum(&read, forum_id).await?;
            views::post_view(&read, content, cache, &forum, post_id).await
        })
        .await?;
    Ok(octet_response(bytes))
}

async fn reactions(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<Response, RelayError> {
    let queries: Vec<([u8; 32], [u8; 32])> = bcs::from_bytes(&body)
        .map_err(|error| RelayError::BadRequest(format!("reactions batch decode: {error}")))?;
    let cache = &state.cache;
    let bytes = state
        .db
        .read(move |read| {
            let queries = queries.clone();
            async move { views::reactions_view(&read, cache, queries).await }
        })
        .await?;
    Ok(octet_response(bytes))
}

#[derive(Deserialize)]
struct CursorQuery {
    cursor: Option<u64>,
    counter: Option<u64>,
}

async fn feed(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<CursorQuery>,
) -> Result<Response, RelayError> {
    let feed_id = parse_id(&id)?;
    let counter = query
        .counter
        .ok_or_else(|| RelayError::BadRequest("counter is required".into()))?;
    let collection = ech_db_entity::CollectionId::from_array(feed_id);
    let cache = &state.cache;
    let cursor = query.cursor;
    let bytes = state
        .db
        .read(move |read| async move { views::feed_view(&read, cache, collection, counter, cursor).await })
        .await?;
    Ok(octet_response(bytes))
}

async fn bans(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<CursorQuery>,
) -> Result<Response, RelayError> {
    let level = EntityId(parse_id(&id)?);
    let content = &state.content;
    let cache = &state.cache;
    let cursor = query.cursor;
    let bytes = state
        .db
        .read(move |read| async move { views::bans_view(&read, content, cache, level, cursor).await })
        .await?;
    Ok(octet_response(bytes))
}

#[derive(Deserialize)]
struct BanHashesRequest {
    pk: String,
    signature: Vec<u8>,
}

async fn ban_hashes(
    State(state): State<Arc<AppState>>,
    Path(post): Path<String>,
    Json(request): Json<BanHashesRequest>,
) -> Result<Response, RelayError> {
    let post_id = parse_id(&post)?;
    let pk = parse_id(&request.pk)?;

    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&pk)
        .map_err(|error| RelayError::BadRequest(format!("invalid pk: {error}")))?;
    let signature = ed25519_dalek::Signature::from_slice(&request.signature)
        .map_err(|error| RelayError::BadRequest(format!("invalid signature: {error}")))?;
    use ed25519_dalek::Verifier;
    verifying
        .verify(&post_id, &signature)
        .map_err(|error| RelayError::Unauthorized(format!("signature verification: {error}")))?;

    let root = state.root;
    let forum_id = state.forum_id;
    let post_object = state
        .db
        .read(move |read| async move {
            let mut posts = views::load_posts(&read, &[post_id]).await?;
            let post_object = posts.pop().ok_or_else(|| RelayError::NotFound("post".into()))?;
            let thread = EntityId(post_object.projection.thread);
            let thread_object = views::load_thread(&read, thread).await?;
            let board = EntityId(thread_object.projection.board);

            let mut authorized = pk == root;
            if !authorized {
                authorized = views::list_mods(&read, forum_id).await?.contains(&pk);
            }
            if !authorized {
                authorized = views::list_mods(&read, board).await?.contains(&pk);
            }
            if !authorized {
                authorized = thread_object.projection.admin == Some(pk)
                    || views::list_mods(&read, thread).await?.contains(&pk);
            }
            if !authorized {
                return Err(RelayError::Unauthorized("pk not authorized in chain".into()));
            }
            Ok(post_object)
        })
        .await?;

    let sealed = state.secrets.unseal_uid(&post_object.projection.uid)?;
    let mut hashes = Vec::with_capacity(4);
    for (index, mask) in [32u8, 24, 20, 16].iter().enumerate() {
        hashes.push(state.secrets.ban_digest(*mask, &sealed[index]));
    }
    Ok(bcs_response(&BanHashes { hashes })?)
}

#[derive(Deserialize)]
struct DecryptRequest {
    uid: Vec<u8>,
    path: Vec<String>,
    pk: String,
    signature: Vec<u8>,
}

async fn decrypt(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DecryptRequest>,
) -> Result<Response, RelayError> {
    if request.path.is_empty() || request.path.len() > 4 {
        return Err(RelayError::BadRequest("path must have 1-4 ids".into()));
    }
    let mut path = Vec::with_capacity(request.path.len());
    for item in &request.path {
        path.push(parse_id(item)?);
    }
    let pk = parse_id(&request.pk)?;

    let mut message = Vec::new();
    message.extend_from_slice(&request.uid);
    for item in &path {
        message.extend_from_slice(item);
    }
    message.extend_from_slice(&pk);
    let digest = blake2b_simd::Params::new().hash_length(32).hash(&message);

    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&pk)
        .map_err(|error| RelayError::BadRequest(format!("invalid pk: {error}")))?;
    let signature = ed25519_dalek::Signature::from_slice(&request.signature)
        .map_err(|error| RelayError::BadRequest(format!("invalid signature: {error}")))?;
    use ed25519_dalek::Verifier;
    verifying
        .verify(digest.as_bytes(), &signature)
        .map_err(|error| RelayError::Unauthorized(format!("signature verification: {error}")))?;

    let root = state.root;
    let forum = state.forum_id;
    let uid = &request.uid;
    let path = &path;
    state
        .db
        .read(move |read| async move {
            if path[0] != forum.0 {
                return Err(RelayError::BadRequest("path[0] is not the forum".into()));
            }
            let mut authorized = pk == root;
            if !authorized {
                authorized = views::list_mods(&read, forum).await?.contains(&pk);
            }
            let mut scope = if authorized { 0usize } else { usize::MAX };
            if !authorized && path.len() >= 2 {
                let board = EntityId(path[1]);
                if views::list_mods(&read, board).await?.contains(&pk) {
                    authorized = true;
                    scope = 1;
                }
            }
            if !authorized && path.len() >= 3 {
                let thread = EntityId(path[2]);
                let thread_object = views::load_thread(&read, thread).await?;
                if thread_object.projection.admin == Some(pk)
                    || views::list_mods(&read, thread).await?.contains(&pk)
                {
                    authorized = true;
                    scope = 2;
                }
            }
            if !authorized {
                return Err(RelayError::Unauthorized("pk not authorized in chain".into()));
            }

            if scope == 2 {
                if path.len() < 4 {
                    return Err(RelayError::BadRequest(
                        "thread-scoped decrypt requires the post in path".into(),
                    ));
                }
                let post_id = EntityId(path[3]);
                let posts = views::load_posts(&read, &[post_id.0]).await?;
                let post = posts
                    .first()
                    .ok_or_else(|| RelayError::NotFound("post".into()))?;
                if post.projection.thread != path[2] {
                    return Err(RelayError::BadRequest("post does not belong to this thread".into()));
                }
                if post.projection.uid != *uid {
                    return Err(RelayError::BadRequest("uid is not the uid of that post".into()));
                }
            }
            Ok(())
        })
        .await?;

    let chunks = state.secrets.unseal_uid(&request.uid)?;
    let masks: [u8; 4] = [32, 24, 20, 16];
    let mut output = Vec::with_capacity(128);
    for (index, chunk) in chunks.iter().enumerate() {
        output.extend_from_slice(&state.secrets.ban_digest(masks[index], chunk));
    }
    Ok(octet_response(output))
}

async fn send_intents(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Result<Response, RelayError> {
    let remote_ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .ok_or_else(|| RelayError::BadRequest("invalid ipv4".into()))?,
    };
    let form = read_form(multipart).await?;
    if form.intents.is_empty() {
        return Err(RelayError::BadRequest("no intents provided".into()));
    }
    let mut intents = Vec::with_capacity(form.intents.len());
    for blob in &form.intents {
        let intent: SignedIntent = bcs::from_bytes(blob)
            .map_err(|error| RelayError::BadRequest(format!("failed to decode intent: {error}")))?;
        intents.push(intent);
    }
    let bytes = crate::send::send(
        &state,
        SendInputs {
            intents,
            text: form.text,
            media: form.media,
            description: form.description,
            topic: form.topic,
            reason: form.reason,
            name: form.name,
            tripcode: form.tripcode,
            captcha: form.captcha,
            remote_ip,
        },
    )
    .await?;
    Ok(bcs_response_raw(bytes))
}

struct SendForm {
    intents: Vec<Bytes>,
    text: Option<Bytes>,
    media: Vec<PathBuf>,
    description: Option<String>,
    topic: Option<String>,
    reason: Option<String>,
    name: Option<String>,
    tripcode: Option<String>,
    captcha: Option<String>,
    _files: Vec<tempfile::NamedTempFile>,
}

async fn read_form(mut multipart: Multipart) -> Result<SendForm, RelayError> {
    let mut form = SendForm {
        intents: Vec::new(),
        text: None,
        media: Vec::new(),
        description: None,
        topic: None,
        reason: None,
        name: None,
        tripcode: None,
        captcha: None,
        _files: Vec::new(),
    };
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| RelayError::BadRequest(format!("multipart: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "intent" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|error| RelayError::BadRequest(format!("multipart: {error}")))?;
                form.intents.push(data);
            }
            "captcha" | "description" | "topic" | "reason" | "name" | "tripcode" => {
                let value = field
                    .text()
                    .await
                    .map_err(|error| RelayError::BadRequest(format!("multipart: {error}")))?;
                let slot = match name.as_str() {
                    "captcha" => &mut form.captcha,
                    "description" => &mut form.description,
                    "topic" => &mut form.topic,
                    "reason" => &mut form.reason,
                    "name" => &mut form.name,
                    _ => &mut form.tripcode,
                };
                if slot.is_some() {
                    return Err(RelayError::BadRequest(format!("duplicate field {name}")));
                }
                *slot = Some(value);
            }
            "text" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|error| RelayError::BadRequest(format!("multipart: {error}")))?;
                if form.text.is_some() {
                    return Err(RelayError::BadRequest("duplicate field text".into()));
                }
                form.text = Some(data);
            }
            "media" => {
                let mut file = tempfile::NamedTempFile::new()
                    .map_err(|error| RelayError::Internal(format!("tempfile: {error}")))?;
                let mut field = field;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|error| RelayError::BadRequest(format!("multipart: {error}")))?
                {
                    std::io::Write::write_all(&mut file, &chunk)
                        .map_err(|error| RelayError::Internal(format!("tempfile: {error}")))?;
                }
                form.media.push(file.path().to_path_buf());
                form._files.push(file);
            }
            _ => {}
        }
    }
    Ok(form)
}

async fn content(
    State(state): State<Arc<AppState>>,
    Path((board, thread, post, kind, hash)): Path<(String, String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, RelayError> {
    let board = parse_id(&board)?;
    let thread = parse_id(&thread)?;
    let post = parse_id(&post)?;
    let hash = parse_id(&hash)?;
    let kind = match kind.as_str() {
        "text" => ContentKind::Text,
        "media" => ContentKind::Media,
        "thumbnail" | "thumb" => ContentKind::Thumbnail,
        "plaintext" => ContentKind::PlainText,
        "media_meta" => ContentKind::MediaMeta,
        "reaction" => ContentKind::Reaction,
        _ => return Err(RelayError::NotFound("unknown content kind".into())),
    };

    let (board_object, thread_object, post_object) = state
        .db
        .read(move |read| async move {
            let board_object = views::load_board(&read, EntityId(board)).await?;
            if board_object.projection.deleted {
                return Err(RelayError::NotFound("board deleted".into()));
            }
            let thread_object = views::load_thread(&read, EntityId(thread)).await?;
            if thread_object.projection.deleted {
                return Err(RelayError::NotFound("thread deleted".into()));
            }
            let mut posts = views::load_posts(&read, &[post]).await?;
            let post_object = posts
                .pop()
                .ok_or_else(|| RelayError::NotFound("post".into()))?;
            if post_object.projection.deleted {
                return Err(RelayError::NotFound("post deleted".into()));
            }
            Ok((board_object, thread_object, post_object))
        })
        .await?;

    let etag = format!("\"{}\"", hex::encode(hash));
    if let Some(value) = headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()) {
        if value.split(',').any(|part| part.trim() == etag) {
            return Ok(StatusCode::NOT_MODIFIED.into_response());
        }
    }

    match kind {
        ContentKind::Text => {
            if post_object.projection.text_hash != Some(hash) {
                return Err(RelayError::NotFound("text".into()));
            }
        }
        ContentKind::PlainText => {
            let allowed = post_object.projection.name_hash == Some(hash)
                || thread_object.projection.topic_hash == Some(hash)
                || board_object.projection.description_hash == Some(hash);
            if !allowed {
                return Err(RelayError::NotFound("plaintext".into()));
            }
        }
        _ => {
            if !post_object.projection.media_hashes.contains(&hash)
                || post_object.projection.banned_media.contains(&hash)
            {
                return Err(RelayError::NotFound("media".into()));
            }
        }
    }

    if kind.redirects() {
        return Ok(axum::response::Redirect::temporary(&state.content.public_url(kind, &hash))
            .into_response());
    }

    let Some(data) = state.content.get(kind, &hash).await? else {
        return Err(RelayError::NotFound("content".into()));
    };
    let mime = match kind {
        ContentKind::MediaMeta => "application/octet-stream".to_string(),
        _ => crate::types::FileType::detect(&data)
            .map(|file| file.mime().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string()),
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
            (header::ETAG, etag),
        ],
        data,
    )
        .into_response())
}

async fn reaction_content(
    State(state): State<Arc<AppState>>,
    Path((board, hash)): Path<(String, String)>,
) -> Result<Response, RelayError> {
    let board = parse_id(&board)?;
    let hash = parse_id(&hash)?;
    let board_object = state
        .db
        .read(move |read| async move { views::load_board(&read, EntityId(board)).await })
        .await?;
    if board_object.projection.deleted {
        return Err(RelayError::NotFound("board deleted".into()));
    }
    if !board_object.projection.reactions.contains(&hash) {
        return Err(RelayError::NotFound("reaction".into()));
    }
    Ok(axum::response::Redirect::temporary(
        &state.content.public_url(ContentKind::Reaction, &hash),
    )
    .into_response())
}

async fn reaction_put(
    State(state): State<Arc<AppState>>,
    Path((_board, hash)): Path<(String, String)>,
    body: Bytes,
) -> Result<Response, RelayError> {
    let hash = parse_id(&hash)?;
    if blake2b_simd::Params::new().hash_length(32).hash(&body).as_bytes() != hash.as_slice() {
        return Err(RelayError::BadRequest("content hash mismatch".into()));
    }
    let file_type = crate::types::FileType::detect(&body)
        .ok_or_else(|| RelayError::BadRequest("unsupported file type".into()))?;
    if !matches!(
        file_type,
        crate::types::FileType::Jpeg
            | crate::types::FileType::Png
            | crate::types::FileType::WebP
            | crate::types::FileType::Gif
    ) {
        return Err(RelayError::BadRequest("unsupported file type".into()));
    }
    state
        .content
        .put_bytes(ContentKind::Reaction, &hash, body.to_vec())
        .await?;
    Ok(StatusCode::OK.into_response())
}

#[derive(Serialize)]
struct EventJson {
    kind: &'static str,
    id: String,
    counter: u64,
    event: String,
}

#[derive(Serialize)]
struct BatchJson {
    forum: Option<String>,
    board: Option<String>,
    thread: Option<String>,
    post: Option<String>,
    events: Vec<EventJson>,
}

fn hex_opt(id: Option<[u8; 32]>) -> Option<String> {
    id.map(|id| format!("0x{}", hex::encode(id)))
}

fn batch_json(batch: &crate::types::Batch) -> String {
    let json = BatchJson {
        forum: hex_opt(batch.forum),
        board: hex_opt(batch.board),
        thread: hex_opt(batch.thread),
        post: hex_opt(batch.post),
        events: batch
            .events
            .iter()
            .map(|event| EventJson {
                kind: match event.kind {
                    EntityKind::Forum => "forum",
                    EntityKind::Board => "board",
                    EntityKind::Thread => "thread",
                    EntityKind::Post => "post",
                },
                id: format!("0x{}", hex::encode(event.id)),
                counter: event.counter,
                event: format!("0x{}", hex::encode(&event.event)),
            })
            .collect(),
    };
    serde_json::to_string(&json).unwrap_or_default()
}

enum Scope {
    Forum,
    Board,
    Thread,
}

fn scoped(batch: &crate::types::Batch, scope: &Scope, id: &str) -> bool {
    match scope {
        Scope::Forum => {
            batch
                .forum
                .map(|value| format!("0x{}", hex::encode(value)))
                .as_deref()
                == Some(id)
                && batch.thread.is_none()
        }
        Scope::Board => {
            batch
                .board
                .map(|value| format!("0x{}", hex::encode(value)))
                .as_deref()
                == Some(id)
        }
        Scope::Thread => {
            batch
                .thread
                .map(|value| format!("0x{}", hex::encode(value)))
                .as_deref()
                == Some(id)
        }
    }
}

async fn sse_scope(
    state: Arc<AppState>,
    scope: Scope,
    id: String,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, RelayError> {
    let receiver = state.realtime.subscribe();
    let stream = futures::stream::unfold((receiver, scope, id), |(mut receiver, scope, id)| async move {
        loop {
            match receiver.recv().await {
                Ok(batch) => {
                    if !scoped(&batch, &scope, &id) {
                        continue;
                    }
                    let event = Event::default().data(batch_json(&batch));
                    return Some((Ok::<Event, std::convert::Infallible>(event), (receiver, scope, id)));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream))
}

async fn sse_forum(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, RelayError> {
    let id = format!("0x{}", hex::encode(parse_id(&id)?));
    Ok(sse_scope(state, Scope::Forum, id).await?.into_response())
}

async fn sse_board(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, RelayError> {
    let id = format!("0x{}", hex::encode(parse_id(&id)?));
    Ok(sse_scope(state, Scope::Board, id).await?.into_response())
}

async fn sse_thread(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, RelayError> {
    let id = format!("0x{}", hex::encode(parse_id(&id)?));
    Ok(sse_scope(state, Scope::Thread, id).await?.into_response())
}

async fn add_moderator(
    State(state): State<Arc<AppState>>,
    Json(moderator): Json<String>,
) -> Result<Response, RelayError> {
    moderator_action(state, moderator, "forum_add_moderator").await
}

async fn del_moderator(
    State(state): State<Arc<AppState>>,
    Json(moderator): Json<String>,
) -> Result<Response, RelayError> {
    moderator_action(state, moderator, "forum_del_moderator").await
}

async fn moderator_action(
    state: Arc<AppState>,
    moderator: String,
    function: &str,
) -> Result<Response, RelayError> {
    let moderator = parse_id(&moderator)?;
    let result = state.db.submit_root_serialized(function, &moderator).await?;
    state.cache.invalidate_all().await;
    Ok(bcs_response_raw(result.output))
}

fn octet_response(bytes: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    )
        .into_response()
}

fn bcs_response<T: Serialize + ?Sized>(value: &T) -> Result<Response, RelayError> {
    let bytes = bcs::to_bytes(value).map_err(|error| RelayError::Internal(error.to_string()))?;
    Ok(bcs_response_raw(bytes))
}

fn bcs_response_raw(bytes: Vec<u8>) -> Response {
    octet_response(bytes)
}
