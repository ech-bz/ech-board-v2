import { bcs, type InferBcsType } from '@mysten/bcs'
import { Address, BanKey, BanValue } from './common'
import { BoardObject, ThreadObject, PostObject, ForumObject } from './chain'

const AddressVecPostObject = bcs.vector(bcs.tuple([Address, bcs.vector(PostObject)]))

const AddressPostObjectMap = bcs.vector(bcs.tuple([Address, PostObject]))

const AddressBytesMap = bcs.vector(bcs.tuple([Address, bcs.byteVector()]))

export const MediaMeta = bcs.struct('MediaMeta', {
  mime: bcs.string(),
  width: bcs.u32(),
  height: bcs.u32(),
  duration_ms: bcs.option(bcs.u64()),
  size: bcs.u64(),
})

export type MediaMetaData = InferBcsType<typeof MediaMeta>

const AddressMediaMetaMap = bcs.vector(bcs.tuple([Address, MediaMeta]))

export const Moderators = bcs.struct('Moderators', {
  forum_admin: bcs.option(Address),
  forum_mods: bcs.vector(Address),
  board_mods: bcs.vector(Address),
  thread_mods: bcs.vector(Address),
  thread_admin: bcs.option(Address),
})

export type ModeratorsData = InferBcsType<typeof Moderators>

export const BoardView = bcs.struct('BoardView', {
  board: BoardObject,
  threads: bcs.vector(ThreadObject),
  op_posts: AddressPostObjectMap,
  last_3: AddressVecPostObject,
  text: AddressBytesMap,
  plain_text: AddressBytesMap,
  media_meta: AddressMediaMetaMap,
  next_cursor: bcs.option(bcs.u64()),
  moderators: Moderators,
})

export type BoardViewData = InferBcsType<typeof BoardView>

export const ThreadView = bcs.struct('ThreadView', {
  thread: ThreadObject,
  posts: bcs.vector(PostObject),
  text: AddressBytesMap,
  plain_text: AddressBytesMap,
  media_meta: AddressMediaMetaMap,
  moderators: Moderators,
})

export type ThreadViewData = InferBcsType<typeof ThreadView>

export const PostReactions = bcs.struct('PostReactions', {
  reaction: bcs.option(Address),
})

export const ReactionsBatch = bcs.vector(bcs.tuple([Address, Address]))

export const PostView = bcs.struct('PostView', {
  post: PostObject,
  thread: ThreadObject,
  board: BoardObject,
  text: AddressBytesMap,
  plain_text: AddressBytesMap,
  media_meta: AddressMediaMetaMap,
  moderators: Moderators,
})

export const ForumView = bcs.struct('ForumView', {
  forum: ForumObject,
  boards: bcs.vector(BoardObject),
  plain_text: AddressBytesMap,
  moderators: Moderators,
})

export const NonceInfo = bcs.struct('NonceInfo', {
  nonce: bcs.u64(),
})

export const BanHashes = bcs.struct('BanHashes', {
  hashes: bcs.vector(Address),
})

export const Info = bcs.struct('Info', {
  forum: Address,
  program: Address,
  admin: Address,
})

export const EntityKind = bcs.enum('EntityKind', {
  forum: null,
  board: null,
  thread: null,
  post: null,
})

export const AppliedEvent = bcs.struct('AppliedEvent', {
  kind: EntityKind,
  id: Address,
  counter: bcs.u64(),
  event: bcs.byteVector(),
})

export const SendResponse = bcs.struct('SendResponse', {
  accepted_by: bcs.vector(bcs.string()),
  digest: bcs.string(),
  events: bcs.vector(AppliedEvent),
  created: bcs.vector(Address),
})

export const FeedView = bcs.struct('FeedView', {
  items: bcs.vector(bcs.byteVector()),
  next_cursor: bcs.option(bcs.u64()),
})

const BanEntry = bcs.struct('BanEntry', {
  mask: bcs.u8(),
  ip_hash: Address,
  reason_hash: Address,
  reason: bcs.option(bcs.string()),
  expires: bcs.u64(),
})

export const BansView = bcs.struct('BansView', {
  level: Address,
  bans: bcs.vector(BanEntry),
  next_cursor: bcs.option(bcs.u64()),
})

export const NewBoardArgs = bcs.struct('NewBoardArgs', {
  slug: bcs.byteVector(),
  max_media: bcs.u64(),
  bump_limit: bcs.u64(),
  desc_hash: bcs.option(Address),
})

export const NewThreadArgs = bcs.struct('NewThreadArgs', {
  board: Address,
  topic_hash: bcs.option(Address),
  text_hash: bcs.option(Address),
  media_hashes: bcs.vector(Address),
  name_hash: bcs.option(Address),
  vote_keys: bcs.vector(Address),
  multi_vote: bcs.bool(),
})

export const NewPostArgs = bcs.struct('NewPostArgs', {
  board: Address,
  thread: Address,
  text_hash: bcs.option(Address),
  media_hashes: bcs.vector(Address),
  name_hash: bcs.option(Address),
  vote_keys: bcs.vector(Address),
  multi_vote: bcs.bool(),
})

export const NewThreadResult = bcs.struct('NewThreadResult', {
  thread: Address,
  post: Address,
})

export const ForumEvent = bcs.enum('ForumEvent', {
  Genesis: bcs.struct('Genesis', { admin: Address }),
  BoardRegistered: bcs.struct('BoardRegistered', { slug: bcs.byteVector(), board: Address }),
  AddModerator: bcs.struct('AddModerator', { moderator: Address }),
  DelModerator: bcs.struct('DelModerator', { moderator: Address }),
  SetTimestampPrecision: bcs.struct('SetTimestampPrecision', { precision: bcs.u64() }),
  Ban: bcs.struct('Ban', { key: BanKey, value: BanValue }),
  Unban: bcs.struct('Unban', { key: BanKey }),
})

export const BoardEventSpec = bcs.enum('BoardEvent', {
  Genesis: bcs.struct('Genesis', { slug: bcs.byteVector() }),
  SetMaxMedia: bcs.struct('SetMaxMedia', { max_media: bcs.u64() }),
  SetBumpLimit: bcs.struct('SetBumpLimit', { bump_limit: bcs.u64() }),
  SetDescription: bcs.struct('SetDescription', { desc_hash: bcs.option(Address) }),
  SetIgnoreForumBans: bcs.struct('SetIgnoreForumBans', { ignore: bcs.bool() }),
  SetReactions: bcs.struct('SetReactions', { reactions: bcs.vector(Address) }),
  SetPinned: bcs.struct('SetPinned', { pinned: bcs.vector(Address) }),
  SetClosed: bcs.struct('SetClosed', { closed: bcs.bool() }),
  SetDeleted: bcs.struct('SetDeleted', { deleted: bcs.bool() }),
  AddModerator: bcs.struct('AddModerator', { moderator: Address }),
  DelModerator: bcs.struct('DelModerator', { moderator: Address }),
  ThreadRegistered: bcs.struct('ThreadRegistered', { number: bcs.u64(), thread: Address }),
  PostSubmitted: bcs.struct('PostSubmitted', { number: bcs.u64(), post: Address }),
  ThreadBumped: bcs.struct('ThreadBumped', { thread: Address }),
  Ban: bcs.struct('Ban', { key: BanKey, value: BanValue }),
  Unban: bcs.struct('Unban', { key: BanKey }),
})

export const ThreadEventSpec = bcs.enum('ThreadEvent', {
  Genesis: bcs.struct('Genesis', { board: Address, number: bcs.u64(), topic_hash: bcs.option(Address) }),
  AddModerator: bcs.struct('AddModerator', { moderator: Address }),
  DelModerator: bcs.struct('DelModerator', { moderator: Address }),
  SetClosed: bcs.struct('SetClosed', { closed: bcs.bool() }),
  SetDeleted: bcs.struct('SetDeleted', { deleted: bcs.bool() }),
  SetTopic: bcs.struct('SetTopic', { topic_hash: bcs.option(Address) }),
  SetAdmin: bcs.struct('SetAdmin', { admin: bcs.option(Address) }),
  PostAdded: bcs.struct('PostAdded', { post: Address }),
  PostDeletedDelta: bcs.struct('PostDeletedDelta', { deleted: bcs.bool() }),
  Ban: bcs.struct('Ban', { key: BanKey, value: BanValue }),
  Unban: bcs.struct('Unban', { key: BanKey }),
})

export const PostEventSpec = bcs.enum('PostEvent', {
  Genesis: bcs.struct('Genesis', {
    thread: Address,
    number: bcs.u64(),
    timestamp_ms: bcs.u64(),
    name_hash: bcs.option(Address),
    text_hash: bcs.option(Address),
    media_hashes: bcs.vector(Address),
    vote_keys: bcs.vector(Address),
    multi_vote: bcs.bool(),
  }),
  SetDeleted: bcs.struct('SetDeleted', { deleted: bcs.bool() }),
  SetText: bcs.struct('SetText', { text_hash: bcs.option(Address) }),
  BanMedia: bcs.struct('BanMedia', { hashes: bcs.vector(Address) }),
  UnbanMedia: bcs.struct('UnbanMedia', { hashes: bcs.vector(Address) }),
  SetReaction: bcs.struct('SetReaction', { by: Address, ip32: Address, old: bcs.option(Address), reaction_hash: Address }),
  Vote: bcs.struct('Vote', { by: Address, ip32: Address, options: bcs.vector(Address) }),
  SetBanned: bcs.struct('SetBanned', { banned: bcs.option(BanKey) }),
  SetModNote: bcs.struct('SetModNote', { mod_note: bcs.option(Address) }),
})
