#![cfg(target_arch = "wasm32")]

use ech_db_entity::{bootstrap_root, export_entity_program, AttestedIntent, EntityId, Error};
use forum_model::*;
use serde::de::DeserializeOwned;
use serde::Serialize;

export_entity_program!(handle);

fn args<T: DeserializeOwned>(attested: &AttestedIntent) -> std::result::Result<T, CommandError> {
    bcs::from_bytes(&attested.intent.body.call.args).map_err(|error| CommandError::Args {
        message: error.to_string(),
    })
}

fn out<T: Serialize>(value: &T) -> std::result::Result<Vec<u8>, CommandError> {
    bcs::to_bytes(value).map_err(|error| CommandError::Encode {
        message: error.to_string(),
    })
}

fn handle(attested: AttestedIntent) -> std::result::Result<Vec<u8>, Vec<u8>> {
    match run(&attested) {
        Ok(bytes) => Ok(bytes),
        Err(error) => match bcs::to_bytes(&error) {
            Ok(bytes) => Err(bytes),
            Err(_) => Err(b"encode".to_vec()),
        },
    }
}

fn ip32_of(attested: &AttestedIntent) -> std::result::Result<Hash, CommandError> {
    let responses: Responses = bcs::from_bytes(&attested.attestation.body.responses).map_err(
        |error| CommandError::Args {
            message: error.to_string(),
        },
    )?;
    responses.ip32.ok_or(CommandError::Args {
        message: "ip32 missing".to_string(),
    })
}

fn creator_of(post: &PostEntity) -> std::result::Result<Pubkey, CommandError> {
    match post.events()?.source(1)? {
        Some(source) => Ok(source.author),
        None => Err(CommandError::entity(&Error::Missing(post.id()?))),
    }
}

fn create_post(
    forum: &ForumEntity,
    board: &mut BoardEntity,
    thread: &mut ThreadEntity,
    by: &Pubkey,
    now_ms: u64,
    requires_media: bool,
    text_hash: Option<Hash>,
    media_hashes: Vec<Hash>,
    name_hash: Option<Hash>,
    vote_keys: Vec<Hash>,
    multi_vote: bool,
) -> ForumResult<EntityId> {
    let max_media = *board.max_media()?;
    if requires_media && max_media != 0 && media_hashes.is_empty() {
        return Err(AppError::PostRequiresMedia.into());
    }
    if media_hashes.len() as u64 > max_media {
        return Err(AppError::MediaLimitExceeded.into());
    }
    if media_hashes.is_empty() && text_hash.is_none() {
        return Err(AppError::PostEmpty.into());
    }
    if vote_keys.len() > VOTE_OPTIONS_LIMIT {
        return Err(AppError::VoteOptionsLimit.into());
    }
    let moderator =
        forum.admin()? == *by || forum.mods()?.has(by)? || board.mods()?.has(by)?;
    if *board.closed()? && !moderator {
        return Err(AppError::BoardClosed.into());
    }
    if *thread.closed()? && !(moderator || *thread.admin()? == Some(*by)) {
        return Err(AppError::ThreadClosed.into());
    }
    let number = board.posts()?.count()? + 1;
    let precision = *forum.timestamp_precision()?;
    let timestamp_ms = if precision > 0 {
        now_ms - now_ms % precision
    } else {
        now_ms
    };
    let post = thread.spawn::<PostEntity>(&PostEvent::Genesis {
        thread: thread.id()?,
        number,
        timestamp_ms,
        name_hash,
        text_hash,
        media_hashes,
        vote_keys,
        multi_vote,
    })?;
    let post_id = post.id()?;
    drop(post);
    board.register_post(number, post_id)?;
    let thread_id = thread.id()?;
    let live = thread.posts()?.len()? + 1 - *thread.posts_deleted()?;
    if live <= *board.bump_limit()? && !board.pinned()?.contains(&thread_id) {
        board.bump(thread_id)?;
    }
    thread.add_post(post_id)?;
    Ok(post_id)
}

fn run(attested: &AttestedIntent) -> std::result::Result<Vec<u8>, CommandError> {
    let function = attested.intent.body.call.function.as_str();
    let by = attested.intent.body.author;
    let now_ms = attested.attestation.body.timestamp_ms;
    match function {
        "bootstrap" => {
            let root = bootstrap_root::<ForumEntity>(&ForumEvent::Genesis { admin: by })?;
            out(&root.id()?)
        }
        "read_forum" => {
            let forum = ForumEntity::load_root()?;
            out(&ForumView::of(&forum)?)
        }
        "find_board" => {
            let slug: Vec<u8> = args(attested)?;
            let forum = ForumEntity::load_root()?;
            out(&forum.board_of(&slug)?)
        }
        "read_board" => {
            let board_id: EntityId = args(attested)?;
            let board = BoardEntity::load(board_id)?;
            out(&BoardView::of(&board)?)
        }
        "read_thread" => {
            let thread_id: EntityId = args(attested)?;
            let thread = ThreadEntity::load(thread_id)?;
            out(&ThreadView::of(&thread)?)
        }
        "read_post" => {
            let post_id: EntityId = args(attested)?;
            let post = PostEntity::load(post_id)?;
            let creator = creator_of(&post)?;
            out(&PostView::of(&post, creator)?)
        }
        "upgrade_forum" => {
            let mut forum = ForumEntity::load_root()?;
            forum.upgrade()?;
            out(&())
        }
        "upgrade_board" => {
            let board_id: EntityId = args(attested)?;
            let mut board = BoardEntity::load(board_id)?;
            board.upgrade()?;
            out(&())
        }
        "upgrade_thread" => {
            let thread_id: EntityId = args(attested)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.upgrade()?;
            out(&())
        }
        "upgrade_post" => {
            let post_id: EntityId = args(attested)?;
            let mut post = PostEntity::load(post_id)?;
            post.upgrade()?;
            out(&())
        }
        "forum_add_moderator" => {
            let moderator: Pubkey = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.add_moderator(&by, moderator)?;
            out(&())
        }
        "forum_del_moderator" => {
            let moderator: Pubkey = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.del_moderator(&by, moderator)?;
            out(&())
        }
        "forum_set_timestamp_precision" => {
            let precision: u64 = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.set_timestamp_precision(&by, precision)?;
            out(&())
        }
        "forum_ban" => {
            let (key, value): (BanKey, BanValue) = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.ban(&by, key, value)?;
            out(&())
        }
        "forum_unban" => {
            let key: BanKey = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.unban(&by, key)?;
            out(&())
        }
        "forum_new_board" => {
            let board_args: NewBoardArgs = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            forum.ensure_new_board(&by, &board_args.slug)?;
            let board = forum.spawn::<BoardEntity>(&BoardEvent::Genesis {
                slug: board_args.slug.clone(),
            })?;
            let board_id = board.id()?;
            let mut board = board;
            forum.register_board(board_args.slug, board_id)?;
            board.set_max_media(&forum, &by, board_args.max_media)?;
            board.set_bump_limit(&forum, &by, board_args.bump_limit)?;
            board.set_description(&forum, &by, board_args.desc_hash)?;
            out(&board_id)
        }
        "forum_ban_post" => {
            let (board_id, thread_id, post_id, key, value): (
                EntityId,
                EntityId,
                EntityId,
                BanKey,
                BanValue,
            ) = args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            forum.ban(&by, key, value)?;
            post.set_banned(&forum, &board, &thread, &by, Some(key))?;
            out(&())
        }
        "forum_unban_post" => {
            let (board_id, thread_id, post_id, key): (EntityId, EntityId, EntityId, BanKey) =
                args(attested)?;
            let mut forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.ensure_banned(&key)?;
            forum.unban(&by, key)?;
            post.set_banned(&forum, &board, &thread, &by, None)?;
            out(&())
        }
        "board_add_moderator" => {
            let (board_id, moderator): (EntityId, Pubkey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.add_moderator(&forum, &by, moderator)?;
            out(&())
        }
        "board_del_moderator" => {
            let (board_id, moderator): (EntityId, Pubkey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.del_moderator(&forum, &by, moderator)?;
            out(&())
        }
        "board_set_max_media" => {
            let (board_id, max_media): (EntityId, u64) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_max_media(&forum, &by, max_media)?;
            out(&())
        }
        "board_set_bump_limit" => {
            let (board_id, bump_limit): (EntityId, u64) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_bump_limit(&forum, &by, bump_limit)?;
            out(&())
        }
        "board_set_closed" => {
            let (board_id, closed): (EntityId, bool) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_closed(&forum, &by, closed)?;
            out(&())
        }
        "board_set_deleted" => {
            let (board_id, deleted): (EntityId, bool) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_deleted(&forum, &by, deleted)?;
            out(&())
        }
        "board_set_description" => {
            let (board_id, desc_hash): (EntityId, Option<Hash>) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_description(&forum, &by, desc_hash)?;
            out(&())
        }
        "board_set_ignore_forum_bans" => {
            let (board_id, ignore): (EntityId, bool) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_ignore_forum_bans(&forum, &by, ignore)?;
            out(&())
        }
        "board_set_reactions" => {
            let (board_id, reactions): (EntityId, Vec<Hash>) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_reactions(&forum, &by, reactions)?;
            out(&())
        }
        "board_set_pinned" => {
            let (board_id, pinned): (EntityId, Vec<EntityId>) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.set_pinned(&forum, &by, pinned)?;
            out(&())
        }
        "board_ban" => {
            let (board_id, key, value): (EntityId, BanKey, BanValue) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.ban(&forum, &by, key, value)?;
            out(&())
        }
        "board_unban" => {
            let (board_id, key): (EntityId, BanKey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            board.unban(&forum, &by, key)?;
            out(&())
        }
        "board_ban_post" => {
            let (board_id, thread_id, post_id, key, value): (
                EntityId,
                EntityId,
                EntityId,
                BanKey,
                BanValue,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            board.ban(&forum, &by, key, value)?;
            post.set_banned(&forum, &board, &thread, &by, Some(key))?;
            out(&())
        }
        "board_unban_post" => {
            let (board_id, thread_id, post_id, key): (EntityId, EntityId, EntityId, BanKey) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.ensure_banned(&key)?;
            board.unban(&forum, &by, key)?;
            post.set_banned(&forum, &board, &thread, &by, None)?;
            out(&())
        }
        "new_thread" => {
            let thread_args: NewThreadArgs = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(thread_args.board)?;
            let number = board.posts()?.count()? + 1;
            let thread = board.spawn::<ThreadEntity>(&ThreadEvent::Genesis {
                board: thread_args.board,
                number,
                topic_hash: thread_args.topic_hash,
            })?;
            let thread_id = thread.id()?;
            board.register_thread(number, thread_id)?;
            let mut thread = thread;
            let post_id = create_post(
                &forum,
                &mut board,
                &mut thread,
                &by,
                now_ms,
                true,
                thread_args.text_hash,
                thread_args.media_hashes,
                thread_args.name_hash,
                thread_args.vote_keys,
                thread_args.multi_vote,
            )?;
            out(&NewThreadResult {
                thread: thread_id,
                post: post_id,
            })
        }
        "new_post" => {
            let post_args: NewPostArgs = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let mut board = BoardEntity::load(post_args.board)?;
            let mut thread = ThreadEntity::load(post_args.thread)?;
            let post_id = create_post(
                &forum,
                &mut board,
                &mut thread,
                &by,
                now_ms,
                false,
                post_args.text_hash,
                post_args.media_hashes,
                post_args.name_hash,
                post_args.vote_keys,
                post_args.multi_vote,
            )?;
            out(&post_id)
        }
        "thread_add_moderator" => {
            let (board_id, thread_id, moderator): (EntityId, EntityId, Pubkey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.add_moderator(&forum, &board, &by, moderator)?;
            out(&())
        }
        "thread_del_moderator" => {
            let (board_id, thread_id, moderator): (EntityId, EntityId, Pubkey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.del_moderator(&forum, &board, &by, moderator)?;
            out(&())
        }
        "thread_set_closed" => {
            let (board_id, thread_id, closed): (EntityId, EntityId, bool) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.set_closed(&forum, &board, &by, closed)?;
            out(&())
        }
        "thread_set_deleted" => {
            let (board_id, thread_id, deleted): (EntityId, EntityId, bool) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.set_deleted(&forum, &board, &by, deleted)?;
            out(&())
        }
        "thread_set_topic" => {
            let (board_id, thread_id, topic_hash): (EntityId, EntityId, Option<Hash>) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.set_topic(&forum, &board, &by, topic_hash)?;
            out(&())
        }
        "thread_set_admin" => {
            let (board_id, thread_id, admin): (EntityId, EntityId, Option<Pubkey>) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.set_admin(&forum, &board, &by, admin)?;
            out(&())
        }
        "thread_ban" => {
            let (board_id, thread_id, key, value): (EntityId, EntityId, BanKey, BanValue) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.ban(&forum, &board, &by, key, value)?;
            out(&())
        }
        "thread_unban" => {
            let (board_id, thread_id, key): (EntityId, EntityId, BanKey) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            thread.unban(&forum, &board, &by, key)?;
            out(&())
        }
        "thread_ban_post" => {
            let (board_id, thread_id, post_id, key, value): (
                EntityId,
                EntityId,
                EntityId,
                BanKey,
                BanValue,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            thread.ban(&forum, &board, &by, key, value)?;
            post.set_banned(&forum, &board, &thread, &by, Some(key))?;
            out(&())
        }
        "thread_unban_post" => {
            let (board_id, thread_id, post_id, key): (EntityId, EntityId, EntityId, BanKey) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.ensure_banned(&key)?;
            thread.unban(&forum, &board, &by, key)?;
            post.set_banned(&forum, &board, &thread, &by, None)?;
            out(&())
        }
        "set_post_deleted" => {
            let (board_id, thread_id, post_id, deleted): (EntityId, EntityId, EntityId, bool) =
                args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            let creator = creator_of(&post)?;
            post.set_deleted(&forum, &board, &thread, &by, &creator, now_ms, deleted)?;
            thread.post_deleted(deleted)?;
            out(&())
        }
        "set_post_text" => {
            let (board_id, thread_id, post_id, text_hash): (
                EntityId,
                EntityId,
                EntityId,
                Option<Hash>,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let mut thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            let creator = creator_of(&post)?;
            post.set_text(&forum, &board, &thread, &by, &creator, now_ms, text_hash)?;
            if post.creation()?.media_hashes.is_empty()
                && post.text_hash()?.is_none()
                && !*post.deleted()?
            {
                post.set_deleted(&forum, &board, &thread, &by, &creator, now_ms, true)?;
                thread.post_deleted(true)?;
            }
            out(&())
        }
        "ban_media" => {
            let (board_id, thread_id, post_id, hashes): (
                EntityId,
                EntityId,
                EntityId,
                Vec<Hash>,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            let creator = creator_of(&post)?;
            post.ban_media(&forum, &board, &thread, &by, &creator, now_ms, hashes)?;
            out(&())
        }
        "unban_media" => {
            let (board_id, thread_id, post_id, hashes): (
                EntityId,
                EntityId,
                EntityId,
                Vec<Hash>,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.unban_media(&forum, &board, &thread, &by, hashes)?;
            out(&())
        }
        "set_reaction_v2" => {
            let (board_id, thread_id, post_id, old, reaction_hash): (
                EntityId,
                EntityId,
                EntityId,
                Option<Hash>,
                Hash,
            ) = args(attested)?;
            let ip32 = ip32_of(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.set_reaction(&forum, &board, &thread, &by, &ip32, old, reaction_hash)?;
            out(&())
        }
        "vote_v2" => {
            let (board_id, thread_id, post_id, options): (
                EntityId,
                EntityId,
                EntityId,
                Vec<Hash>,
            ) = args(attested)?;
            let ip32 = ip32_of(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.vote(&forum, &board, &thread, &by, &ip32, options)?;
            out(&())
        }
        "set_mod_note" => {
            let (board_id, thread_id, post_id, mod_note): (
                EntityId,
                EntityId,
                EntityId,
                Option<Hash>,
            ) = args(attested)?;
            let forum = ForumEntity::load_root()?;
            let board = BoardEntity::load(board_id)?;
            let thread = ThreadEntity::load(thread_id)?;
            let mut post = PostEntity::load(post_id)?;
            post.set_mod_note(&forum, &board, &thread, &by, mod_note)?;
            out(&())
        }
        _ => Err(CommandError::Args {
            message: format!("unknown function {function}"),
        }),
    }
}
