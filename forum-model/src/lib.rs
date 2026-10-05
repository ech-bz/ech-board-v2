use ech_db_entity::{
    entity, entity_impl, runtime, BcsSchema, EntityId, Error, Feed, Map, Result, Set,
};
use serde::{Deserialize, Serialize};

pub type Hash = [u8; 32];
pub type Pubkey = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum AppError {
    #[error("not authorized")]
    NotAuthorized,
    #[error("board slug invalid")]
    BoardSlugInvalid,
    #[error("board closed")]
    BoardClosed,
    #[error("thread closed")]
    ThreadClosed,
    #[error("media limit exceeded")]
    MediaLimitExceeded,
    #[error("post requires media")]
    PostRequiresMedia,
    #[error("post empty")]
    PostEmpty,
    #[error("vote options limit")]
    VoteOptionsLimit,
    #[error("vote options mismatch")]
    VoteOptionsMismatch,
    #[error("already voted")]
    AlreadyVoted,
    #[error("reaction not allowed")]
    ReactionNotAllowed,
    #[error("media not found")]
    MediaNotFound,
    #[error("cross reference mismatch")]
    CrossReferenceMismatch,
    #[error("intent args mismatch")]
    IntentArgsMismatch,
    #[error("invalid state")]
    InvalidState,
}

pub enum ForumError {
    App(AppError),
    Entity(Error),
}

impl From<Error> for ForumError {
    fn from(error: Error) -> Self {
        Self::Entity(error)
    }
}

impl From<AppError> for ForumError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

pub type ForumResult<T> = std::result::Result<T, ForumError>;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub struct Tripcode {
    pub secured: bool,
    pub trip: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub struct Responses {
    pub uid: Option<Vec<u8>>,
    pub ip32: Option<Hash>,
    pub tripcode: Option<Tripcode>,
    pub geo: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, BcsSchema)]
pub struct BanKey {
    pub level: EntityId,
    pub mask: u8,
    pub ip_hash: Hash,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, BcsSchema)]
pub struct BanValue {
    pub reason_hash: Hash,
    pub expires: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub struct MarkEntry {
    pub ip32: Hash,
    pub options: Vec<Hash>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub struct PostCreation {
    pub thread: EntityId,
    pub number: u64,
    pub timestamp_ms: u64,
    pub name_hash: Option<Hash>,
    pub text_hash: Option<Hash>,
    pub media_hashes: Vec<Hash>,
    pub vote_keys: Vec<Hash>,
    pub multi_vote: bool,
}

const SELF_MODERATE_WINDOW_MS: u64 = 600_000;
pub const VOTE_OPTIONS_LIMIT: usize = 16;
const SLUG_MAX_LEN: usize = 16;
const BAN_MASKS: [u8; 4] = [32, 24, 20, 16];

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub enum ForumEvent {
    Genesis {
        admin: Pubkey,
    },
    BoardRegistered {
        slug: Vec<u8>,
        board: EntityId,
    },
    AddModerator {
        moderator: Pubkey,
    },
    DelModerator {
        moderator: Pubkey,
    },
    SetTimestampPrecision {
        precision: u64,
    },
    Ban {
        key: BanKey,
        value: BanValue,
    },
    Unban {
        key: BanKey,
    },
}

#[entity(event = ForumEvent, key = ())]
pub enum ForumEntity {
    V1 {
        mods: Set<Pubkey>,
        bans: Map<BanKey, BanValue>,
        boards: Map<Vec<u8>, EntityId>,
        timestamp_precision: u64,
    },
}

#[entity_impl]
impl ForumEntity {
    fn genesis(event: &ForumEvent) -> Result<Self> {
        if let ForumEvent::Genesis { .. } = event {
            Ok(Self::V1 {
                mods: Set::default(),
                bans: Map::default(),
                boards: Map::default(),
                timestamp_precision: 0,
            })
        } else {
            Err(Error::Rejected)
        }
    }

    fn apply(&mut self, event: &ForumEvent) -> Result<()> {
        match event {
            ForumEvent::Genesis { .. } => Ok(()),
            ForumEvent::BoardRegistered { slug, board } => self.boards_mut()?.set(slug, board),
            ForumEvent::AddModerator { moderator } => self.mods_mut()?.set(moderator),
            ForumEvent::DelModerator { moderator } => self.mods_mut()?.remove(moderator),
            ForumEvent::SetTimestampPrecision { precision } => {
                *self.timestamp_precision_mut()? = *precision;
                Ok(())
            }
            ForumEvent::Ban { key, value } => self.bans_mut()?.set(key, value),
            ForumEvent::Unban { key } => self.bans_mut()?.remove(key),
        }
    }
}

impl ForumEntity {
    fn record(&mut self, event: &ForumEvent) -> ForumResult<()> {
        runtime::apply(self, event)?;
        Ok(())
    }

    pub fn admin(&self) -> ForumResult<Pubkey> {
        match self.events()?.get(1)? {
            Some(ForumEvent::Genesis { admin }) => Ok(admin),
            _ => Err(AppError::InvalidState.into()),
        }
    }

    fn ensure_admin(&self, by: &Pubkey) -> ForumResult<()> {
        if self.admin()? != *by {
            return Err(AppError::NotAuthorized.into());
        }
        Ok(())
    }

    fn ensure_moderator(&self, by: &Pubkey) -> ForumResult<()> {
        if self.admin()? == *by || self.mods()?.has(by)? {
            return Ok(());
        }
        Err(AppError::NotAuthorized.into())
    }

    pub fn board_of(&self, slug: &[u8]) -> ForumResult<Option<EntityId>> {
        Ok(self.boards()?.get(&slug.to_vec())?)
    }

    pub fn ensure_new_board(&self, by: &Pubkey, slug: &[u8]) -> ForumResult<()> {
        self.ensure_moderator(by)?;
        let valid = !slug.is_empty()
            && slug.len() <= SLUG_MAX_LEN
            && slug
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        if !valid {
            return Err(AppError::BoardSlugInvalid.into());
        }
        if self.boards()?.get(&slug.to_vec())?.is_some() {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    pub fn register_board(&mut self, slug: Vec<u8>, board: EntityId) -> ForumResult<()> {
        self.record(&ForumEvent::BoardRegistered { slug, board })
    }

    pub fn add_moderator(&mut self, by: &Pubkey, moderator: Pubkey) -> ForumResult<()> {
        self.ensure_admin(by)?;
        self.record(&ForumEvent::AddModerator { moderator })
    }

    pub fn del_moderator(&mut self, by: &Pubkey, moderator: Pubkey) -> ForumResult<()> {
        self.ensure_admin(by)?;
        self.record(&ForumEvent::DelModerator { moderator })
    }

    pub fn set_timestamp_precision(&mut self, by: &Pubkey, precision: u64) -> ForumResult<()> {
        self.ensure_moderator(by)?;
        self.record(&ForumEvent::SetTimestampPrecision { precision })
    }

    pub fn ban(&mut self, by: &Pubkey, key: BanKey, value: BanValue) -> ForumResult<()> {
        self.ensure_moderator(by)?;
        self.check_ban_key(&key)?;
        self.record(&ForumEvent::Ban { key, value })
    }

    pub fn unban(&mut self, by: &Pubkey, key: BanKey) -> ForumResult<()> {
        self.ensure_moderator(by)?;
        self.check_ban_key(&key)?;
        self.record(&ForumEvent::Unban { key })
    }

    fn check_ban_key(&self, key: &BanKey) -> ForumResult<()> {
        if key.level != self.id()? || !BAN_MASKS.contains(&key.mask) {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ForumView {
    pub admin: Pubkey,
    pub mods: u64,
    pub boards: u64,
    pub bans: u64,
    pub timestamp_precision: u64,
}

impl ForumView {
    pub fn of(forum: &ForumEntity) -> ForumResult<Self> {
        Ok(Self {
            admin: forum.admin()?,
            mods: forum.mods()?.count()?,
            boards: forum.boards()?.count()?,
            bans: forum.bans()?.count()?,
            timestamp_precision: *forum.timestamp_precision()?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub enum BoardEvent {
    Genesis {
        slug: Vec<u8>,
    },
    SetMaxMedia {
        max_media: u64,
    },
    SetBumpLimit {
        bump_limit: u64,
    },
    SetDescription {
        desc_hash: Option<Hash>,
    },
    SetIgnoreForumBans {
        ignore: bool,
    },
    SetReactions {
        reactions: Vec<Hash>,
    },
    SetPinned {
        pinned: Vec<EntityId>,
    },
    SetClosed {
        closed: bool,
    },
    SetDeleted {
        deleted: bool,
    },
    AddModerator {
        moderator: Pubkey,
    },
    DelModerator {
        moderator: Pubkey,
    },
    ThreadRegistered {
        number: u64,
        thread: EntityId,
    },
    PostSubmitted {
        number: u64,
        post: EntityId,
    },
    ThreadBumped {
        thread: EntityId,
    },
    Ban {
        key: BanKey,
        value: BanValue,
    },
    Unban {
        key: BanKey,
    },
}

#[entity(event = BoardEvent, key = (slug,))]
pub enum BoardEntity {
    V1 {
        slug: Vec<u8>,
        desc_hash: Option<Hash>,
        max_media: u64,
        bump_limit: u64,
        closed: bool,
        deleted: bool,
        pinned: Vec<EntityId>,
        ignore_forum_bans: bool,
        mods: Set<Pubkey>,
        bans: Map<BanKey, BanValue>,
        reactions: Vec<Hash>,
        threads: Map<u64, EntityId>,
        posts: Map<u64, EntityId>,
        bumps: Feed<EntityId>,
    },
}

#[entity_impl]
impl BoardEntity {
    fn genesis(event: &BoardEvent) -> Result<Self> {
        if let BoardEvent::Genesis { slug } = event {
            Ok(Self::V1 {
                slug: slug.clone(),
                desc_hash: None,
                max_media: 0,
                bump_limit: 0,
                closed: false,
                deleted: false,
                pinned: Vec::new(),
                ignore_forum_bans: false,
                mods: Set::default(),
                bans: Map::default(),
                reactions: Vec::new(),
                threads: Map::default(),
                posts: Map::default(),
                bumps: Feed::default(),
            })
        } else {
            Err(Error::Rejected)
        }
    }

    fn apply(&mut self, event: &BoardEvent) -> Result<()> {
        match event {
            BoardEvent::Genesis { .. } => Ok(()),
            BoardEvent::SetMaxMedia { max_media } => {
                *self.max_media_mut()? = *max_media;
                Ok(())
            }
            BoardEvent::SetBumpLimit { bump_limit } => {
                *self.bump_limit_mut()? = *bump_limit;
                Ok(())
            }
            BoardEvent::SetDescription { desc_hash } => {
                *self.desc_hash_mut()? = *desc_hash;
                Ok(())
            }
            BoardEvent::SetIgnoreForumBans { ignore } => {
                *self.ignore_forum_bans_mut()? = *ignore;
                Ok(())
            }
            BoardEvent::SetReactions { reactions } => {
                *self.reactions_mut()? = reactions.clone();
                Ok(())
            }
            BoardEvent::SetPinned { pinned } => {
                *self.pinned_mut()? = pinned.clone();
                Ok(())
            }
            BoardEvent::SetClosed { closed } => {
                *self.closed_mut()? = *closed;
                Ok(())
            }
            BoardEvent::SetDeleted { deleted } => {
                *self.deleted_mut()? = *deleted;
                Ok(())
            }
            BoardEvent::AddModerator { moderator } => self.mods_mut()?.set(moderator),
            BoardEvent::DelModerator { moderator } => self.mods_mut()?.remove(moderator),
            BoardEvent::ThreadRegistered { number, thread } => self.threads_mut()?.set(number, thread),
            BoardEvent::PostSubmitted { number, post } => self.posts_mut()?.set(number, post),
            BoardEvent::ThreadBumped { thread } => {
                self.bumps_mut()?.append(thread)?;
                Ok(())
            }
            BoardEvent::Ban { key, value } => self.bans_mut()?.set(key, value),
            BoardEvent::Unban { key } => self.bans_mut()?.remove(key),
        }
    }
}

impl BoardEntity {
    fn record(&mut self, event: &BoardEvent) -> ForumResult<()> {
        runtime::apply(self, event)?;
        Ok(())
    }

    fn ensure_registered(&self, forum: &ForumEntity) -> ForumResult<()> {
        if forum.boards()?.get(self.slug()?)?.is_none() {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    fn ensure_forum_moderator(&self, forum: &ForumEntity, by: &Pubkey) -> ForumResult<()> {
        if forum.admin()? == *by || forum.mods()?.has(by)? {
            return Ok(());
        }
        Err(AppError::NotAuthorized.into())
    }

    fn ensure_moderator(&self, forum: &ForumEntity, by: &Pubkey) -> ForumResult<()> {
        if forum.admin()? == *by || forum.mods()?.has(by)? || self.mods()?.has(by)? {
            return Ok(());
        }
        Err(AppError::NotAuthorized.into())
    }

    fn check_ban_key(&self, key: &BanKey) -> ForumResult<()> {
        if key.level != self.id()? || !BAN_MASKS.contains(&key.mask) {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    pub fn add_moderator(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        moderator: Pubkey,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        self.record(&BoardEvent::AddModerator { moderator })
    }

    pub fn del_moderator(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        moderator: Pubkey,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        self.record(&BoardEvent::DelModerator { moderator })
    }

    pub fn set_max_media(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        max_media: u64,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        self.record(&BoardEvent::SetMaxMedia { max_media })
    }

    pub fn set_bump_limit(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        bump_limit: u64,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        self.record(&BoardEvent::SetBumpLimit { bump_limit })
    }

    pub fn set_ignore_forum_bans(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        ignore: bool,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        self.record(&BoardEvent::SetIgnoreForumBans { ignore })
    }

    pub fn set_closed(&mut self, forum: &ForumEntity, by: &Pubkey, closed: bool) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        if *self.closed()? == closed {
            return Err(AppError::IntentArgsMismatch.into());
        }
        if *self.deleted()? {
            return Err(AppError::InvalidState.into());
        }
        self.record(&BoardEvent::SetClosed { closed })
    }

    pub fn set_deleted(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        deleted: bool,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_forum_moderator(forum, by)?;
        if *self.deleted()? == deleted {
            return Err(AppError::IntentArgsMismatch.into());
        }
        if !*self.closed()? {
            return Err(AppError::InvalidState.into());
        }
        self.record(&BoardEvent::SetDeleted { deleted })
    }

    pub fn set_description(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        desc_hash: Option<Hash>,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_moderator(forum, by)?;
        self.record(&BoardEvent::SetDescription { desc_hash })
    }

    pub fn set_reactions(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        reactions: Vec<Hash>,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_moderator(forum, by)?;
        self.record(&BoardEvent::SetReactions { reactions })
    }

    pub fn set_pinned(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        pinned: Vec<EntityId>,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_moderator(forum, by)?;
        self.record(&BoardEvent::SetPinned { pinned })
    }

    pub fn ban(
        &mut self,
        forum: &ForumEntity,
        by: &Pubkey,
        key: BanKey,
        value: BanValue,
    ) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_moderator(forum, by)?;
        self.check_ban_key(&key)?;
        self.record(&BoardEvent::Ban { key, value })
    }

    pub fn unban(&mut self, forum: &ForumEntity, by: &Pubkey, key: BanKey) -> ForumResult<()> {
        self.ensure_registered(forum)?;
        self.ensure_moderator(forum, by)?;
        self.check_ban_key(&key)?;
        self.record(&BoardEvent::Unban { key })
    }

    pub fn register_thread(&mut self, number: u64, thread: EntityId) -> ForumResult<()> {
        self.record(&BoardEvent::ThreadRegistered { number, thread })
    }

    pub fn register_post(&mut self, number: u64, post: EntityId) -> ForumResult<()> {
        self.record(&BoardEvent::PostSubmitted { number, post })
    }

    pub fn bump(&mut self, thread: EntityId) -> ForumResult<()> {
        self.record(&BoardEvent::ThreadBumped { thread })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct BoardView {
    pub slug: Vec<u8>,
    pub desc_hash: Option<Hash>,
    pub max_media: u64,
    pub bump_limit: u64,
    pub closed: bool,
    pub deleted: bool,
    pub pinned: Vec<EntityId>,
    pub ignore_forum_bans: bool,
    pub mods: u64,
    pub bans: u64,
    pub reactions: Vec<Hash>,
    pub threads: u64,
    pub posts: u64,
    pub bumps: u64,
}

impl BoardView {
    pub fn of(board: &BoardEntity) -> ForumResult<Self> {
        Ok(Self {
            slug: board.slug()?.clone(),
            desc_hash: *board.desc_hash()?,
            max_media: *board.max_media()?,
            bump_limit: *board.bump_limit()?,
            closed: *board.closed()?,
            deleted: *board.deleted()?,
            pinned: board.pinned()?.clone(),
            ignore_forum_bans: *board.ignore_forum_bans()?,
            mods: board.mods()?.count()?,
            bans: board.bans()?.count()?,
            reactions: board.reactions()?.clone(),
            threads: board.threads()?.count()?,
            posts: board.posts()?.count()?,
            bumps: board.bumps()?.len()?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub enum ThreadEvent {
    Genesis {
        board: EntityId,
        number: u64,
        topic_hash: Option<Hash>,
    },
    AddModerator {
        moderator: Pubkey,
    },
    DelModerator {
        moderator: Pubkey,
    },
    SetClosed {
        closed: bool,
    },
    SetDeleted {
        deleted: bool,
    },
    SetTopic {
        topic_hash: Option<Hash>,
    },
    SetAdmin {
        admin: Option<Pubkey>,
    },
    PostAdded {
        post: EntityId,
    },
    PostDeletedDelta {
        deleted: bool,
    },
    Ban {
        key: BanKey,
        value: BanValue,
    },
    Unban {
        key: BanKey,
    },
}

#[entity(event = ThreadEvent, key = (board, number))]
pub enum ThreadEntity {
    V1 {
        board: EntityId,
        number: u64,
        topic_hash: Option<Hash>,
        closed: bool,
        deleted: bool,
        admin: Option<Pubkey>,
        mods: Set<Pubkey>,
        bans: Map<BanKey, BanValue>,
        posts: Feed<EntityId>,
        posts_deleted: u64,
    },
}

#[entity_impl]
impl ThreadEntity {
    fn genesis(event: &ThreadEvent) -> Result<Self> {
        if let ThreadEvent::Genesis {
            board,
            number,
            topic_hash,
        } = event
        {
            Ok(Self::V1 {
                board: *board,
                number: *number,
                topic_hash: *topic_hash,
                closed: false,
                deleted: false,
                admin: None,
                mods: Set::default(),
                bans: Map::default(),
                posts: Feed::default(),
                posts_deleted: 0,
            })
        } else {
            Err(Error::Rejected)
        }
    }

    fn apply(&mut self, event: &ThreadEvent) -> Result<()> {
        match event {
            ThreadEvent::Genesis { .. } => Ok(()),
            ThreadEvent::AddModerator { moderator } => self.mods_mut()?.set(moderator),
            ThreadEvent::DelModerator { moderator } => self.mods_mut()?.remove(moderator),
            ThreadEvent::SetClosed { closed } => {
                *self.closed_mut()? = *closed;
                Ok(())
            }
            ThreadEvent::SetDeleted { deleted } => {
                *self.deleted_mut()? = *deleted;
                Ok(())
            }
            ThreadEvent::SetTopic { topic_hash } => {
                *self.topic_hash_mut()? = *topic_hash;
                Ok(())
            }
            ThreadEvent::SetAdmin { admin } => {
                *self.admin_mut()? = *admin;
                Ok(())
            }
            ThreadEvent::PostAdded { post } => {
                self.posts_mut()?.append(post)?;
                Ok(())
            }
            ThreadEvent::PostDeletedDelta { deleted } => {
                let count = self.posts_deleted_mut()?;
                if *deleted {
                    *count = count.checked_add(1).ok_or(Error::Overflow)?;
                } else {
                    *count = count.checked_sub(1).ok_or(Error::Overflow)?;
                }
                Ok(())
            }
            ThreadEvent::Ban { key, value } => self.bans_mut()?.set(key, value),
            ThreadEvent::Unban { key } => self.bans_mut()?.remove(key),
        }
    }
}

impl ThreadEntity {
    fn record(&mut self, event: &ThreadEvent) -> ForumResult<()> {
        runtime::apply(self, event)?;
        Ok(())
    }

    fn ensure_linked(&self, forum: &ForumEntity, board: &BoardEntity) -> ForumResult<()> {
        if board.id()? != *self.board()? || forum.boards()?.get(board.slug()?)?.is_none() {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    fn ensure_moderator(
        &self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
    ) -> ForumResult<()> {
        let ok = forum.admin()? == *by
            || forum.mods()?.has(by)?
            || board.mods()?.has(by)?
            || *self.admin()? == Some(*by);
        if ok {
            Ok(())
        } else {
            Err(AppError::NotAuthorized.into())
        }
    }

    fn ensure_moderator_or_self(
        &self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
    ) -> ForumResult<()> {
        if self.mods()?.has(by)? {
            return Ok(());
        }
        self.ensure_moderator(forum, board, by)
    }

    fn check_ban_key(&self, key: &BanKey) -> ForumResult<()> {
        if key.level != self.id()? || !BAN_MASKS.contains(&key.mask) {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    pub fn add_moderator(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        moderator: Pubkey,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator(forum, board, by)?;
        self.record(&ThreadEvent::AddModerator { moderator })
    }

    pub fn del_moderator(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        moderator: Pubkey,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator(forum, board, by)?;
        self.record(&ThreadEvent::DelModerator { moderator })
    }

    pub fn set_closed(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        closed: bool,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator_or_self(forum, board, by)?;
        if *self.closed()? == closed {
            return Err(AppError::IntentArgsMismatch.into());
        }
        if *self.deleted()? {
            return Err(AppError::InvalidState.into());
        }
        self.record(&ThreadEvent::SetClosed { closed })
    }

    pub fn set_deleted(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        deleted: bool,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator_or_self(forum, board, by)?;
        if *self.deleted()? == deleted {
            return Err(AppError::IntentArgsMismatch.into());
        }
        if !*self.closed()? {
            return Err(AppError::InvalidState.into());
        }
        self.record(&ThreadEvent::SetDeleted { deleted })
    }

    pub fn set_topic(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        topic_hash: Option<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator_or_self(forum, board, by)?;
        self.record(&ThreadEvent::SetTopic { topic_hash })
    }

    pub fn set_admin(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        admin: Option<Pubkey>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator(forum, board, by)?;
        self.record(&ThreadEvent::SetAdmin { admin })
    }

    pub fn ban(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        key: BanKey,
        value: BanValue,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator_or_self(forum, board, by)?;
        self.check_ban_key(&key)?;
        self.record(&ThreadEvent::Ban { key, value })
    }

    pub fn unban(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        by: &Pubkey,
        key: BanKey,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board)?;
        self.ensure_moderator_or_self(forum, board, by)?;
        self.check_ban_key(&key)?;
        self.record(&ThreadEvent::Unban { key })
    }

    pub fn add_post(&mut self, post: EntityId) -> ForumResult<()> {
        self.record(&ThreadEvent::PostAdded { post })
    }

    pub fn post_deleted(&mut self, deleted: bool) -> ForumResult<()> {
        self.record(&ThreadEvent::PostDeletedDelta { deleted })
    }

    pub fn op(&self) -> ForumResult<Option<EntityId>> {
        Ok(self.posts()?.get(1)?)
    }

    pub fn last_posts(&self, count: u64) -> ForumResult<Vec<EntityId>> {
        let len = self.posts()?.len()?;
        let mut posts = Vec::new();
        if len < 2 {
            return Ok(posts);
        }
        let from = if len > count + 1 { len - count + 1 } else { 2 };
        let mut seq = from;
        while seq <= len {
            if let Some(post) = self.posts()?.get(seq)? {
                posts.push(post);
            }
            seq += 1;
        }
        Ok(posts)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ThreadView {
    pub board: EntityId,
    pub number: u64,
    pub topic_hash: Option<Hash>,
    pub closed: bool,
    pub deleted: bool,
    pub admin: Option<Pubkey>,
    pub mods: u64,
    pub bans: u64,
    pub posts: u64,
    pub posts_deleted: u64,
    pub op: Option<EntityId>,
    pub last: Vec<EntityId>,
}

impl ThreadView {
    pub fn of(thread: &ThreadEntity) -> ForumResult<Self> {
        Ok(Self {
            board: *thread.board()?,
            number: *thread.number()?,
            topic_hash: *thread.topic_hash()?,
            closed: *thread.closed()?,
            deleted: *thread.deleted()?,
            admin: *thread.admin()?,
            mods: thread.mods()?.count()?,
            bans: thread.bans()?.count()?,
            posts: thread.posts()?.len()?,
            posts_deleted: *thread.posts_deleted()?,
            op: thread.op()?,
            last: thread.last_posts(3)?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub enum PostEvent {
    Genesis {
        thread: EntityId,
        number: u64,
        timestamp_ms: u64,
        name_hash: Option<Hash>,
        text_hash: Option<Hash>,
        media_hashes: Vec<Hash>,
        vote_keys: Vec<Hash>,
        multi_vote: bool,
    },
    SetDeleted {
        deleted: bool,
    },
    SetText {
        text_hash: Option<Hash>,
    },
    BanMedia {
        hashes: Vec<Hash>,
    },
    UnbanMedia {
        hashes: Vec<Hash>,
    },
    SetReaction {
        by: Pubkey,
        ip32: Hash,
        old: Option<Hash>,
        reaction_hash: Hash,
    },
    Vote {
        by: Pubkey,
        ip32: Hash,
        options: Vec<Hash>,
    },
    SetBanned {
        banned: Option<BanKey>,
    },
    SetModNote {
        mod_note: Option<Hash>,
    },
}

#[entity(event = PostEvent, key = (thread, number))]
pub enum PostEntity {
    V1 {
        thread: EntityId,
        number: u64,
        deleted: bool,
        banned: Option<BanKey>,
        text_hash: Option<Hash>,
        banned_media: Vec<Hash>,
        reactions: Vec<(Hash, u64)>,
        reacted: Map<Pubkey, MarkEntry>,
        reacted_alias: Map<Hash, Pubkey>,
        votes: Vec<(Hash, u64)>,
        voted: Map<Hash, Vec<Hash>>,
        mod_note: Option<Hash>,
    },
}

#[entity_impl]
impl PostEntity {
    fn genesis(event: &PostEvent) -> Result<Self> {
        if let PostEvent::Genesis {
            thread,
            number,
            text_hash,
            vote_keys,
            ..
        } = event
        {
            let mut votes = Vec::new();
            for key in vote_keys {
                if votes.iter().any(|(known, _)| known == key) {
                    return Err(Error::Rejected);
                }
                votes.push((*key, 0));
            }
            Ok(Self::V1 {
                thread: *thread,
                number: *number,
                deleted: false,
                banned: None,
                text_hash: *text_hash,
                banned_media: Vec::new(),
                reactions: Vec::new(),
                reacted: Map::default(),
                reacted_alias: Map::default(),
                votes,
                voted: Map::default(),
                mod_note: None,
            })
        } else {
            Err(Error::Rejected)
        }
    }

    fn apply(&mut self, event: &PostEvent) -> Result<()> {
        match event {
            PostEvent::Genesis { .. } => Ok(()),
            PostEvent::SetDeleted { deleted } => {
                *self.deleted_mut()? = *deleted;
                Ok(())
            }
            PostEvent::SetText { text_hash } => {
                *self.text_hash_mut()? = *text_hash;
                Ok(())
            }
            PostEvent::BanMedia { hashes } => {
                let banned = self.banned_media_mut()?;
                for hash in hashes {
                    if !banned.contains(hash) {
                        banned.push(*hash);
                    }
                }
                Ok(())
            }
            PostEvent::UnbanMedia { hashes } => {
                let banned = self.banned_media_mut()?;
                banned.retain(|hash| !hashes.contains(hash));
                Ok(())
            }
            PostEvent::SetReaction {
                by,
                ip32,
                old: _,
                reaction_hash,
            } => {
                if let Some((owner, entry)) = self.reacted_find(by, ip32)? {
                    let entry_hash = *entry.options.first().ok_or(Error::Rejected)?;
                    self.reaction_decrement(&entry_hash)?;
                    self.reacted_mut()?.remove(&owner)?;
                    self.reacted_alias_mut()?.remove(&entry.ip32)?;
                    if entry_hash != *reaction_hash {
                        self.reaction_increment(reaction_hash)?;
                        self.reacted_mut()?.set(
                            by,
                            &MarkEntry {
                                ip32: *ip32,
                                options: vec![*reaction_hash],
                            },
                        )?;
                        self.reacted_alias_mut()?.set(ip32, by)?;
                    }
                } else {
                    self.reaction_increment(reaction_hash)?;
                    self.reacted_mut()?.set(
                        by,
                        &MarkEntry {
                            ip32: *ip32,
                            options: vec![*reaction_hash],
                        },
                    )?;
                    self.reacted_alias_mut()?.set(ip32, by)?;
                }
                Ok(())
            }
            PostEvent::Vote { ip32, options, .. } => {
                let votes = self.votes_mut()?;
                for option in options {
                    let entry = votes
                        .iter_mut()
                        .find(|(hash, _)| hash == option)
                        .ok_or(Error::Rejected)?;
                    entry.1 = entry.1.checked_add(1).ok_or(Error::Overflow)?;
                }
                self.voted_mut()?.set(ip32, options)?;
                Ok(())
            }
            PostEvent::SetBanned { banned } => {
                *self.banned_mut()? = banned.clone();
                Ok(())
            }
            PostEvent::SetModNote { mod_note } => {
                *self.mod_note_mut()? = *mod_note;
                Ok(())
            }
        }
    }
}

impl PostEntity {
    fn record(&mut self, event: &PostEvent) -> ForumResult<()> {
        runtime::apply(self, event)?;
        Ok(())
    }

    fn reaction_increment(&mut self, hash: &Hash) -> Result<()> {
        let reactions = self.reactions_mut()?;
        if let Some(entry) = reactions.iter_mut().find(|(known, _)| known == hash) {
            entry.1 = entry.1.checked_add(1).ok_or(Error::Overflow)?;
        } else {
            reactions.push((*hash, 1));
        }
        Ok(())
    }

    fn reaction_decrement(&mut self, hash: &Hash) -> Result<()> {
        let reactions = self.reactions_mut()?;
        let entry = reactions
            .iter_mut()
            .find(|(known, _)| known == hash)
            .ok_or(Error::Rejected)?;
        entry.1 = entry.1.checked_sub(1).ok_or(Error::Overflow)?;
        if entry.1 == 0 {
            reactions.retain(|(known, _)| known != hash);
        }
        Ok(())
    }

    fn reacted_find(&self, by: &Pubkey, ip32: &Hash) -> Result<Option<(Pubkey, MarkEntry)>> {
        if let Some(entry) = self.reacted()?.get(by)? {
            return Ok(Some((*by, entry)));
        }
        if let Some(owner) = self.reacted_alias()?.get(ip32)? {
            if let Some(entry) = self.reacted()?.get(&owner)? {
                return Ok(Some((owner, entry)));
            }
        }
        Ok(None)
    }

    pub fn creation(&self) -> Result<PostCreation> {
        match self.events()?.get(1)? {
            Some(PostEvent::Genesis {
                thread,
                number,
                timestamp_ms,
                name_hash,
                text_hash,
                media_hashes,
                vote_keys,
                multi_vote,
            }) => Ok(PostCreation {
                thread,
                number,
                timestamp_ms,
                name_hash,
                text_hash,
                media_hashes,
                vote_keys,
                multi_vote,
            }),
            _ => Err(Error::Rejected),
        }
    }

    fn ensure_linked(
        &self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
    ) -> ForumResult<()> {
        if thread.id()? != *self.thread()?
            || *thread.board()? != board.id()?
            || forum.boards()?.get(board.slug()?)?.is_none()
        {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    fn is_moderator(
        &self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
    ) -> ForumResult<bool> {
        Ok(forum.admin()? == *by
            || forum.mods()?.has(by)?
            || board.mods()?.has(by)?
            || *thread.admin()? == Some(*by)
            || thread.mods()?.has(by)?)
    }

    fn ensure_moderator(
        &self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
    ) -> ForumResult<()> {
        if self.is_moderator(forum, board, thread, by)? {
            Ok(())
        } else {
            Err(AppError::NotAuthorized.into())
        }
    }

    fn self_moderate(&self, by: &Pubkey, creator: &Pubkey, now_ms: u64) -> ForumResult<bool> {
        if by != creator {
            return Ok(false);
        }
        let created = self.creation()?.timestamp_ms;
        Ok(now_ms
            .checked_sub(created)
            .is_some_and(|elapsed| elapsed <= SELF_MODERATE_WINDOW_MS))
    }

    pub fn set_deleted(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        creator: &Pubkey,
        now_ms: u64,
        deleted: bool,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        if *self.deleted()? == deleted {
            return Err(AppError::IntentArgsMismatch.into());
        }
        let creation = self.creation()?;
        let empty = creation.media_hashes.is_empty() && self.text_hash()?.is_none();
        let allowed = self.is_moderator(forum, board, thread, by)?
            || self.self_moderate(by, creator, now_ms)?
            || (deleted && empty);
        if !allowed {
            return Err(AppError::NotAuthorized.into());
        }
        self.record(&PostEvent::SetDeleted { deleted })
    }

    pub fn set_text(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        creator: &Pubkey,
        now_ms: u64,
        text_hash: Option<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        let allowed = self.is_moderator(forum, board, thread, by)?
            || self.self_moderate(by, creator, now_ms)?;
        if !allowed {
            return Err(AppError::NotAuthorized.into());
        }
        self.record(&PostEvent::SetText { text_hash })
    }

    pub fn ban_media(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        creator: &Pubkey,
        now_ms: u64,
        hashes: Vec<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        let allowed = self.is_moderator(forum, board, thread, by)?
            || self.self_moderate(by, creator, now_ms)?;
        if !allowed {
            return Err(AppError::NotAuthorized.into());
        }
        let media = self.creation()?.media_hashes;
        for hash in &hashes {
            if !media.contains(hash) {
                return Err(AppError::MediaNotFound.into());
            }
        }
        self.record(&PostEvent::BanMedia { hashes })
    }

    pub fn unban_media(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        hashes: Vec<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        self.ensure_moderator(forum, board, thread, by)?;
        self.record(&PostEvent::UnbanMedia { hashes })
    }

    pub fn set_reaction(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        ip32: &Hash,
        old: Option<Hash>,
        reaction_hash: Hash,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        if !board.reactions()?.contains(&reaction_hash) {
            return Err(AppError::ReactionNotAllowed.into());
        }
        match self.reacted_find(by, ip32)? {
            Some((owner, entry)) => {
                let entry_hash = *entry.options.first().ok_or(AppError::InvalidState)?;
                if owner != *by {
                    return Err(AppError::NotAuthorized.into());
                }
                if old != Some(entry_hash) {
                    return Err(AppError::IntentArgsMismatch.into());
                }
            }
            None => {
                if old.is_some() {
                    return Err(AppError::IntentArgsMismatch.into());
                }
            }
        }
        self.record(&PostEvent::SetReaction {
            by: *by,
            ip32: *ip32,
            old,
            reaction_hash,
        })
    }

    pub fn vote(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        ip32: &Hash,
        options: Vec<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        let multi_vote = self.creation()?.multi_vote;
        if !(options.len() == 1 || (!options.is_empty() && multi_vote)) {
            return Err(AppError::VoteOptionsMismatch.into());
        }
        if self.voted()?.get(ip32)?.is_some() {
            return Err(AppError::AlreadyVoted.into());
        }
        let mut unique: Vec<Hash> = Vec::new();
        for option in &options {
            if !unique.contains(option) {
                unique.push(*option);
            }
        }
        let votes = self.votes()?;
        for option in &unique {
            if !votes.iter().any(|(hash, _)| hash == option) {
                return Err(AppError::VoteOptionsMismatch.into());
            }
        }
        self.record(&PostEvent::Vote {
            by: *by,
            ip32: *ip32,
            options: unique,
        })
    }

    pub fn ensure_banned(&self, key: &BanKey) -> ForumResult<()> {
        if *self.banned()? != Some(key.clone()) {
            return Err(AppError::CrossReferenceMismatch.into());
        }
        Ok(())
    }

    pub fn set_banned(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        banned: Option<BanKey>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        if self.banned()?.is_some() == banned.is_some() {
            return Err(AppError::IntentArgsMismatch.into());
        }
        self.ensure_moderator(forum, board, thread, by)?;
        self.record(&PostEvent::SetBanned { banned })
    }

    pub fn set_mod_note(
        &mut self,
        forum: &ForumEntity,
        board: &BoardEntity,
        thread: &ThreadEntity,
        by: &Pubkey,
        mod_note: Option<Hash>,
    ) -> ForumResult<()> {
        self.ensure_linked(forum, board, thread)?;
        self.ensure_moderator(forum, board, thread, by)?;
        self.record(&PostEvent::SetModNote { mod_note })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PostView {
    pub thread: EntityId,
    pub number: u64,
    pub creator: Pubkey,
    pub timestamp_ms: u64,
    pub deleted: bool,
    pub banned: Option<BanKey>,
    pub text_hash: Option<Hash>,
    pub media_hashes: Vec<Hash>,
    pub banned_media: Vec<Hash>,
    pub reactions: Vec<(Hash, u64)>,
    pub reactions_marks: u64,
    pub votes: Vec<(Hash, u64)>,
    pub votes_marks: u64,
    pub multi_vote: bool,
    pub mod_note: Option<Hash>,
}

impl PostView {
    pub fn of(post: &PostEntity, creator: Pubkey) -> ForumResult<Self> {
        let creation = post.creation()?;
        Ok(Self {
            thread: *post.thread()?,
            number: *post.number()?,
            creator,
            timestamp_ms: creation.timestamp_ms,
            deleted: *post.deleted()?,
            banned: post.banned()?.clone(),
            text_hash: *post.text_hash()?,
            media_hashes: creation.media_hashes,
            banned_media: post.banned_media()?.clone(),
            reactions: post.reactions()?.clone(),
            reactions_marks: post.reacted()?.count()?,
            votes: post.votes()?.clone(),
            votes_marks: post.voted()?.count()?,
            multi_vote: creation.multi_vote,
            mod_note: *post.mod_note()?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NewBoardArgs {
    pub slug: Vec<u8>,
    pub max_media: u64,
    pub bump_limit: u64,
    pub desc_hash: Option<Hash>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NewThreadArgs {
    pub board: EntityId,
    pub topic_hash: Option<Hash>,
    pub text_hash: Option<Hash>,
    pub media_hashes: Vec<Hash>,
    pub name_hash: Option<Hash>,
    pub vote_keys: Vec<Hash>,
    pub multi_vote: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NewThreadResult {
    pub thread: EntityId,
    pub post: EntityId,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NewPostArgs {
    pub board: EntityId,
    pub thread: EntityId,
    pub text_hash: Option<Hash>,
    pub media_hashes: Vec<Hash>,
    pub name_hash: Option<Hash>,
    pub vote_keys: Vec<Hash>,
    pub multi_vote: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, BcsSchema)]
pub enum CommandError {
    Args {
        message: String,
    },
    Encode {
        message: String,
    },
    Entity {
        code: String,
        message: String,
    },
    Forum {
        code: String,
        message: String,
    },
}

impl CommandError {
    pub fn entity(error: &Error) -> Self {
        Self::Entity {
            code: error.as_ref().to_string(),
            message: error.to_string(),
        }
    }

    pub fn forum(error: &AppError) -> Self {
        Self::Forum {
            code: error.as_ref().to_string(),
            message: error.to_string(),
        }
    }
}

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        Self::entity(&error)
    }
}

impl From<ForumError> for CommandError {
    fn from(error: ForumError) -> Self {
        match error {
            ForumError::App(error) => Self::forum(&error),
            ForumError::Entity(error) => Self::entity(&error),
        }
    }
}
