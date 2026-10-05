import { bcs, type InferBcsType } from '@mysten/bcs'
import { Address, Bans, Feed, Table, Sender, EntityRoot, BanKey, Tripcode } from './common'

const ForumProjectionSchema = bcs.struct('ForumProjection', {
  admin: Address,
  mods: Table,
  bans: Bans,
  boards: Table,
  timestamp_precision_ms: bcs.u64(),
})

export type ForumProjectionData = InferBcsType<typeof ForumProjectionSchema>

export class ForumProjection {
  readonly data: ForumProjectionData
  constructor(raw: ForumProjectionData) {
    this.data = raw
  }
  admin() { return this.data.admin }
  mods() { return this.data.mods }
  bans() { return this.data.bans }
  boards() { return this.data.boards }
  timestamp_precision_ms() { return this.data.timestamp_precision_ms }
}

const ForumObjectSchema = bcs.struct('ForumObject', {
  root: EntityRoot,
  projection: ForumProjectionSchema,
})

export type ForumObject = InferBcsType<typeof ForumObjectSchema>
export const ForumObject = ForumObjectSchema

const BoardProjectionSchema = bcs.struct('BoardProjection', {
  slug: bcs.string(),
  description_hash: bcs.option(Address),
  max_media: bcs.u64(),
  bump_limit: bcs.u64(),
  closed: bcs.bool(),
  deleted: bcs.bool(),
  pinned: bcs.vector(Address),
  ignore_forum_bans: bcs.bool(),
  mods: Table,
  bans: Bans,
  reactions: bcs.vector(Address),
  threads: Table,
  posts: Table,
  bumps: Feed,
})

export type BoardProjectionData = InferBcsType<typeof BoardProjectionSchema>

export class BoardProjection {
  readonly data: BoardProjectionData
  constructor(raw: BoardProjectionData) {
    this.data = raw
  }
  slug() { return this.data.slug }
  description_hash() { return this.data.description_hash }
  max_media() { return this.data.max_media }
  bump_limit() { return this.data.bump_limit }
  closed() { return this.data.closed }
  deleted() { return this.data.deleted }
  ignore_forum_bans() { return this.data.ignore_forum_bans }
  mods() { return this.data.mods }
  bans() { return this.data.bans }
  reactions() { return this.data.reactions }
  threads() { return this.data.threads }
  posts() { return this.data.posts }
  bumps() { return this.data.bumps }
  pinned() { return this.data.pinned }
}

const BoardObjectSchema = bcs.struct('BoardObject', {
  root: EntityRoot,
  projection: BoardProjectionSchema,
})

export type BoardObject = InferBcsType<typeof BoardObjectSchema>
export const BoardObject = BoardObjectSchema

const ThreadProjectionSchema = bcs.struct('ThreadProjection', {
  board: Address,
  number: bcs.u64(),
  topic_hash: bcs.option(Address),
  op: Address,
  closed: bcs.bool(),
  deleted: bcs.bool(),
  admin: bcs.option(Address),
  mods: Table,
  bans: Bans,
  posts: Feed,
  posts_deleted: bcs.u64(),
  last_3: bcs.vector(Address),
})

export type ThreadProjectionData = InferBcsType<typeof ThreadProjectionSchema>

export class ThreadProjection {
  readonly data: ThreadProjectionData
  constructor(raw: ThreadProjectionData) {
    this.data = raw
  }
  board() { return this.data.board }
  number() { return this.data.number }
  topic_hash() { return this.data.topic_hash }
  op() { return this.data.op }
  closed() { return this.data.closed }
  deleted() { return this.data.deleted }
  admin() { return this.data.admin }
  mods() { return this.data.mods }
  bans() { return this.data.bans }
  posts() { return this.data.posts }
  last_3() { return this.data.last_3 }
  pinned() { return false }
  posts_deleted() { return this.data.posts_deleted }
}

const ThreadObjectSchema = bcs.struct('ThreadObject', {
  root: EntityRoot,
  projection: ThreadProjectionSchema,
})

export type ThreadObject = InferBcsType<typeof ThreadObjectSchema>
export const ThreadObject = ThreadObjectSchema

const PostProjectionSchema = bcs.struct('PostProjection', {
  sender: Sender,
  thread: Address,
  number: bcs.u64(),
  uid: bcs.byteVector(),
  timestamp_ms: bcs.u64(),
  deleted: bcs.bool(),
  banned: bcs.option(BanKey),
  text_hash: bcs.option(Address),
  media_hashes: bcs.vector(Address),
  banned_media: bcs.vector(Address),
  reactions: bcs.vector(bcs.tuple([Address, bcs.u64()])),
  votes: bcs.vector(bcs.tuple([Address, bcs.u64()])),
  multi_vote: bcs.bool(),
  name_hash: bcs.option(Address),
  trip: bcs.option(Tripcode),
  geo: bcs.option(bcs.u32()),
  mod_note: bcs.option(Address),
})

export type PostProjectionData = InferBcsType<typeof PostProjectionSchema>

export class PostProjection {
  readonly data: PostProjectionData
  constructor(raw: PostProjectionData) {
    this.data = raw
  }
  sender() { return this.data.sender }
  thread() { return this.data.thread }
  number() { return this.data.number }
  uid() { return this.data.uid }
  timestamp_ms() { return this.data.timestamp_ms }
  deleted() { return this.data.deleted }
  banned() { return this.data.banned }
  text_hash() { return this.data.text_hash }
  media_hashes() { return this.data.media_hashes }
  banned_media() { return this.data.banned_media }
  reactions() { return this.data.reactions }
  votes() { return this.data.votes }
  multi_vote() { return this.data.multi_vote }
  name_hash() { return this.data.name_hash }
  trip() { return this.data.trip }
  geo() { return this.data.geo }
  mod_note() { return this.data.mod_note }
}

const PostObjectSchema = bcs.struct('PostObject', {
  root: EntityRoot,
  projection: PostProjectionSchema,
})

export type PostObject = InferBcsType<typeof PostObjectSchema>
export const PostObject = PostObjectSchema
