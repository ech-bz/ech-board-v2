import { bcs } from '@mysten/bcs'
import { Address } from '../bcs/common'
import type { BoardObject, ModeratorsData } from '../bcs/types'
import { call, type CallDesc, type CallIds, type DecodedEvent } from './core'
import { mapBoard } from './projection'
import { toHex } from '../intent/crypto'
import { NewPostArgs, NewThreadArgs } from '../bcs/relay'

const single = (ids: CallIds): [Uint8Array] => [ids.board!]

const withPubkey = bcs.tuple([Address, Address])
const withU64 = bcs.tuple([Address, bcs.u64()])
const withBool = bcs.tuple([Address, bcs.bool()])
const withOptHash = bcs.tuple([Address, bcs.option(Address)])
const withHashes = bcs.tuple([Address, bcs.vector(Address)])

export interface NewThreadInput {
  topicHash: Uint8Array | null
  textHash: Uint8Array | null
  mediaHashes: Uint8Array[]
  nameHash: Uint8Array | null
  voteKeys: Uint8Array[]
  multiVote: boolean
}

export interface NewPostInput {
  textHash: Uint8Array | null
  mediaHashes: Uint8Array[]
  nameHash: Uint8Array | null
  voteKeys: Uint8Array[]
  multiVote: boolean
}

export const BoardEvent = {
  addModerator: (pk: Uint8Array): CallDesc =>
    call('board_add_moderator', (ids) => withPubkey.serialize([...single(ids), pk]).toBytes()),
  delModerator: (pk: Uint8Array): CallDesc =>
    call('board_del_moderator', (ids) => withPubkey.serialize([...single(ids), pk]).toBytes()),
  setMaxMedia: (v: number | bigint): CallDesc =>
    call('board_set_max_media', (ids) => withU64.serialize([...single(ids), v]).toBytes()),
  setBumpLimit: (v: number | bigint): CallDesc =>
    call('board_set_bump_limit', (ids) => withU64.serialize([...single(ids), v]).toBytes()),
  setClosed: (v: boolean): CallDesc =>
    call('board_set_closed', (ids) => withBool.serialize([...single(ids), v]).toBytes()),
  setDeleted: (v: boolean): CallDesc =>
    call('board_set_deleted', (ids) => withBool.serialize([...single(ids), v]).toBytes()),
  setIgnoreForumBans: (v: boolean): CallDesc =>
    call('board_set_ignore_forum_bans', (ids) => withBool.serialize([...single(ids), v]).toBytes()),
  setDescription: (hash: Uint8Array | null): CallDesc =>
    call('board_set_description', (ids) => withOptHash.serialize([...single(ids), hash]).toBytes()),
  setReactions: (reactions: Uint8Array[]): CallDesc =>
    call('board_set_reactions', (ids) => withHashes.serialize([...single(ids), reactions]).toBytes()),
  setPinned: (pinned: Uint8Array[]): CallDesc =>
    call('board_set_pinned', (ids) => withHashes.serialize([...single(ids), pinned]).toBytes()),
  newThread: (input: NewThreadInput): CallDesc =>
    call('new_thread', (ids) =>
      NewThreadArgs.serialize({
        board: ids.board!,
        topic_hash: input.topicHash,
        text_hash: input.textHash,
        media_hashes: input.mediaHashes,
        name_hash: input.nameHash,
        vote_keys: input.voteKeys,
        multi_vote: input.multiVote,
      }).toBytes()),
  newPost: (input: NewPostInput): CallDesc =>
    call('new_post', (ids) =>
      NewPostArgs.serialize({
        board: ids.board!,
        thread: ids.thread!,
        text_hash: input.textHash,
        media_hashes: input.mediaHashes,
        name_hash: input.nameHash,
        vote_keys: input.voteKeys,
        multi_vote: input.multiVote,
      }).toBytes()),
}

export function applyBoard(board: BoardObject, ev: DecodedEvent): BoardObject {
  switch (ev.$kind) {
    case 'SetDescription':
      return mapBoard(board, (d) => ({ ...d, description_hash: ev.payload.desc_hash ?? null }))
    case 'SetMaxMedia':
      return mapBoard(board, (d) => ({ ...d, max_media: String(ev.payload.max_media) }))
    case 'SetBumpLimit':
      return mapBoard(board, (d) => ({ ...d, bump_limit: String(ev.payload.bump_limit) }))
    case 'SetClosed':
      return mapBoard(board, (d) => ({ ...d, closed: ev.payload.closed }))
    case 'SetDeleted':
      return mapBoard(board, (d) => ({ ...d, deleted: ev.payload.deleted }))
    case 'SetIgnoreForumBans':
      return mapBoard(board, (d) => ({ ...d, ignore_forum_bans: ev.payload.ignore }))
    case 'SetReactions':
      return mapBoard(board, (d) => ({ ...d, reactions: ev.payload.reactions ?? [] }))
    case 'SetPinned':
      return mapBoard(board, (d) => ({ ...d, pinned: ev.payload.pinned ?? [] }))
    default:
      return board
  }
}

export function applyBoardModerators(moderators: ModeratorsData | null | undefined, ev: DecodedEvent): ModeratorsData | null | undefined {
  if (!moderators) return moderators
  if (ev.$kind === 'AddModerator') {
    const mod = ev.payload.moderator
    const hex = toHex(mod)
    if (moderators.board_mods.some((a: Uint8Array) => toHex(a) === hex)) return moderators
    return { ...moderators, board_mods: [...moderators.board_mods, mod] }
  }
  if (ev.$kind === 'DelModerator') {
    const hex = toHex(ev.payload.moderator)
    return { ...moderators, board_mods: moderators.board_mods.filter((a: Uint8Array) => toHex(a) !== hex) }
  }
  return moderators
}
