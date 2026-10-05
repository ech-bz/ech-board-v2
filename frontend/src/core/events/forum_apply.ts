import { bcs } from '@mysten/bcs'
import { Address } from '../bcs/common'
import type { ForumObject, ModeratorsData } from '../bcs/types'
import { call, type CallDesc, type DecodedEvent } from './core'
import { mapForum } from './projection'
import { toHex } from '../intent/crypto'
import { NewBoardArgs } from '../bcs/relay'

const withPubkey = bcs.tuple([Address])
const withU64 = bcs.tuple([bcs.u64()])

export interface NewBoardInput {
  slug: string
  maxMedia: number | bigint
  bumpLimit: number | bigint
  descHash: Uint8Array | null
}

export const ForumEvent = {
  addModerator: (pk: Uint8Array): CallDesc =>
    call('forum_add_moderator', () => withPubkey.serialize([pk]).toBytes()),
  delModerator: (pk: Uint8Array): CallDesc =>
    call('forum_del_moderator', () => withPubkey.serialize([pk]).toBytes()),
  setTimestampPrecision: (precision: number | bigint): CallDesc =>
    call('forum_set_timestamp_precision', () => withU64.serialize([precision]).toBytes()),
  newBoard: (input: NewBoardInput): CallDesc =>
    call('forum_new_board', () =>
      NewBoardArgs.serialize({
        slug: new TextEncoder().encode(input.slug),
        max_media: input.maxMedia,
        bump_limit: input.bumpLimit,
        desc_hash: input.descHash,
      }).toBytes()),
}

export function applyForum(forum: ForumObject, ev: DecodedEvent): ForumObject {
  switch (ev.$kind) {
    case 'SetTimestampPrecision':
      return mapForum(forum, (d) => ({ ...d, timestamp_precision_ms: String(ev.payload.precision) }))
    default:
      return forum
  }
}

export function applyForumModerators(moderators: ModeratorsData | null | undefined, ev: DecodedEvent): ModeratorsData | null | undefined {
  if (!moderators) return moderators
  if (ev.$kind === 'AddModerator') {
    const mod = ev.payload.moderator
    const hex = toHex(mod)
    if (moderators.forum_mods.some((a: Uint8Array) => toHex(a) === hex)) return moderators
    return { ...moderators, forum_mods: [...moderators.forum_mods, mod] }
  }
  if (ev.$kind === 'DelModerator') {
    const hex = toHex(ev.payload.moderator)
    return { ...moderators, forum_mods: moderators.forum_mods.filter((a: Uint8Array) => toHex(a) !== hex) }
  }
  return moderators
}
