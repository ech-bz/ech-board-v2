import { bcs } from '@mysten/bcs'
import { Address } from '../bcs/common'
import type { ThreadObject, ModeratorsData } from '../bcs/types'
import { call, type CallDesc, type DecodedEvent } from './core'
import { mapThread } from './projection'
import { toHex } from '../intent/crypto'
import type { CallIds as Ids } from './core'

const pair = (ids: Ids): [Uint8Array, Uint8Array] => [ids.board!, ids.thread!]

const withPubkey = bcs.tuple([Address, Address, Address])
const withOptPubkey = bcs.tuple([Address, Address, bcs.option(Address)])
const withBool = bcs.tuple([Address, Address, bcs.bool()])
const withOptHash = bcs.tuple([Address, Address, bcs.option(Address)])

export const ThreadEvent = {
  addModerator: (pk: Uint8Array): CallDesc =>
    call('thread_add_moderator', (ids) => withPubkey.serialize([...pair(ids), pk]).toBytes()),
  delModerator: (pk: Uint8Array): CallDesc =>
    call('thread_del_moderator', (ids) => withPubkey.serialize([...pair(ids), pk]).toBytes()),
  setClosed: (v: boolean): CallDesc =>
    call('thread_set_closed', (ids) => withBool.serialize([...pair(ids), v]).toBytes()),
  setDeleted: (v: boolean): CallDesc =>
    call('thread_set_deleted', (ids) => withBool.serialize([...pair(ids), v]).toBytes()),
  setAdmin: (pk: Uint8Array | null): CallDesc =>
    call('thread_set_admin', (ids) => withOptPubkey.serialize([...pair(ids), pk]).toBytes()),
  setTopic: (hash: Uint8Array | null): CallDesc =>
    call('thread_set_topic', (ids) => withOptHash.serialize([...pair(ids), hash]).toBytes()),
}

export function emptyThreadProjection(): ThreadObject['projection'] {
  return {
    board: new Uint8Array(32),
    number: '0',
    topic_hash: null,
    op: new Uint8Array(32),
    closed: false,
    deleted: false,
    admin: null,
    mods: { id: new Uint8Array(32), size: '0' },
    bans: { level: new Uint8Array(32), ip32: '0', ip24: '0', ip20: '0', ip16: '0' },
    posts: { id: new Uint8Array(32), counter: '0' },
    posts_deleted: '0',
    last_3: [],
  }
}

export function applyThreadGenesis(thread: ThreadObject, ev: DecodedEvent): ThreadObject {
  const p = ev.payload
  return {
    ...thread,
    projection: {
      ...thread.projection,
      board: p.board,
      number: String(p.number),
      topic_hash: p.topic_hash ?? null,
    },
  }
}

export function applyThread(thread: ThreadObject, ev: DecodedEvent): ThreadObject {
  switch (ev.$kind) {
    case 'Genesis':
      return applyThreadGenesis(thread, ev)
    case 'PostAdded':
      return mapThread(thread, (d) => ({
        ...d,
        posts: { ...d.posts, counter: String(Number(d.posts.counter) + 1) },
      }))
    case 'PostDeletedDelta':
      return mapThread(thread, (d) => ({
        ...d,
        posts_deleted: String(Math.max(0, Number(d.posts_deleted) + (ev.payload.deleted ? 1 : -1))),
      }))
    case 'SetClosed':
      return mapThread(thread, (d) => ({ ...d, closed: ev.payload.closed }))
    case 'SetDeleted':
      return mapThread(thread, (d) => ({ ...d, deleted: ev.payload.deleted }))
    case 'SetTopic':
      return mapThread(thread, (d) => ({ ...d, topic_hash: ev.payload.topic_hash ?? null }))
    case 'SetAdmin':
      return mapThread(thread, (d) => ({ ...d, admin: ev.payload.admin ?? null }))
    default:
      return thread
  }
}

export function applyThreadModerators(moderators: ModeratorsData | null | undefined, ev: DecodedEvent): ModeratorsData | null | undefined {
  if (!moderators) return moderators
  if (ev.$kind === 'AddModerator') {
    const mod = ev.payload.moderator
    const hex = toHex(mod)
    if (moderators.thread_mods.some((a: Uint8Array) => toHex(a) === hex)) return moderators
    return { ...moderators, thread_mods: [...moderators.thread_mods, mod] }
  }
  if (ev.$kind === 'DelModerator') {
    const hex = toHex(ev.payload.moderator)
    return { ...moderators, thread_mods: moderators.thread_mods.filter((a: Uint8Array) => toHex(a) !== hex) }
  }
  return moderators
}
