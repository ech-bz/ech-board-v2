import { bcs } from '@mysten/bcs'
import { Address } from '../bcs/common'
import type { PostObject, PostProjectionData } from '../bcs/types'
import { call, type CallDesc, type CallIds, type DecodedEvent } from './core'
import { mapPost } from './projection'
import { toHex } from '../intent/crypto'

const tri = (ids: CallIds): [Uint8Array, Uint8Array, Uint8Array] => [ids.board!, ids.thread!, ids.post!]

const withBool = bcs.tuple([Address, Address, Address, bcs.bool()])
const withOptHash = bcs.tuple([Address, Address, Address, bcs.option(Address)])
const withHashes = bcs.tuple([Address, Address, Address, bcs.vector(Address)])
const withReaction = bcs.tuple([Address, Address, Address, bcs.option(Address), Address])
const withOptions = bcs.tuple([Address, Address, Address, bcs.vector(Address)])

export const PostEvent = {
  setDeleted: (v: boolean): CallDesc =>
    call('set_post_deleted', (ids) => withBool.serialize([...tri(ids), v]).toBytes()),
  setText: (hash: Uint8Array | null): CallDesc =>
    call('set_post_text', (ids) => withOptHash.serialize([...tri(ids), hash]).toBytes()),
  banMedia: (hashes: Uint8Array[]): CallDesc =>
    call('ban_media', (ids) => withHashes.serialize([...tri(ids), hashes]).toBytes()),
  unbanMedia: (hashes: Uint8Array[]): CallDesc =>
    call('unban_media', (ids) => withHashes.serialize([...tri(ids), hashes]).toBytes()),
  setReactionV2: (old: Uint8Array | null, reactionHash: Uint8Array): CallDesc =>
    call('set_reaction_v2', (ids) => withReaction.serialize([...tri(ids), old, reactionHash]).toBytes()),
  voteV2: (options: Uint8Array[]): CallDesc =>
    call('vote_v2', (ids) => withOptions.serialize([...tri(ids), options]).toBytes()),
  setModNote: (note: Uint8Array | null): CallDesc =>
    call('set_mod_note', (ids) => withOptHash.serialize([...tri(ids), note]).toBytes()),
}

export function emptyPostProjection(): PostProjectionData {
  return {
    sender: { pk: new Uint8Array(32), tweak: new Uint8Array(32) },
    thread: new Uint8Array(32),
    number: '0',
    uid: new Uint8Array(0),
    timestamp_ms: '0',
    deleted: false,
    banned: null,
    text_hash: null,
    media_hashes: [],
    banned_media: [],
    reactions: [],
    votes: [],
    multi_vote: false,
    name_hash: null,
    trip: null,
    geo: null,
    mod_note: null,
  }
}

export function applyReaction(post: PostObject, old: Uint8Array | null, next: Uint8Array): PostObject {
  return mapPost(post, (d) => {
    const reactions = d.reactions.map(([h, c]) => [h, c] as [Uint8Array, string])
    const dec = (hash: string) => {
      const i = reactions.findIndex(([h]) => toHex(h) === hash)
      if (i < 0) return
      const cnt = Number(reactions[i][1]) - 1
      if (cnt <= 0) reactions.splice(i, 1)
      else reactions[i][1] = String(cnt)
    }
    const inc = (hash: string) => {
      const i = reactions.findIndex(([h]) => toHex(h) === hash)
      if (i >= 0) reactions[i][1] = String(Number(reactions[i][1]) + 1)
      else reactions.push([next.slice(), '1'])
    }
    if (old) dec(toHex(old))
    if (next && (!old || toHex(old) !== toHex(next))) inc(toHex(next))
    return { ...d, reactions }
  })
}

export function applyVote(post: PostObject, options: Uint8Array[]): PostObject {
  return mapPost(post, (d) => {
    const votes = d.votes.map(([h, c]) => [h, c] as [Uint8Array, string])
    const seen = new Set<string>()
    for (const opt of options) {
      const hex = toHex(opt)
      if (seen.has(hex)) continue
      seen.add(hex)
      const i = votes.findIndex(([h]) => toHex(h) === hex)
      if (i >= 0) votes[i][1] = String(Number(votes[i][1]) + 1)
      else votes.push([opt.slice(), '1'])
    }
    return { ...d, votes }
  })
}

export function applyPostGenesis(post: PostObject, ev: DecodedEvent): PostObject {
  const p = ev.payload
  const votes: [Uint8Array, string][] = (p.vote_keys ?? []).map((k: Uint8Array) => [k, '0'] as [Uint8Array, string])
  return {
    ...post,
    projection: {
      ...post.projection,
      thread: p.thread,
      number: String(p.number),
      timestamp_ms: String(p.timestamp_ms),
      name_hash: p.name_hash ?? null,
      text_hash: p.text_hash ?? null,
      media_hashes: p.media_hashes ?? [],
      votes,
      multi_vote: p.multi_vote,
    },
  }
}

export function applyPost(post: PostObject, ev: DecodedEvent): PostObject {
  switch (ev.$kind) {
    case 'Genesis':
      return applyPostGenesis(post, ev)
    case 'SetDeleted':
      return mapPost(post, (d) => ({ ...d, deleted: ev.payload.deleted }))
    case 'SetText':
      return mapPost(post, (d) => ({ ...d, text_hash: ev.payload.text_hash ?? null }))
    case 'BanMedia': {
      return mapPost(post, (d) => {
        const known = new Set(d.banned_media.map(toHex))
        const extra = (ev.payload.hashes ?? []).filter((h: Uint8Array) => !known.has(toHex(h)))
        return { ...d, banned_media: [...d.banned_media, ...extra] }
      })
    }
    case 'UnbanMedia': {
      const remove = new Set<string>((ev.payload.hashes ?? []).map(toHex))
      return mapPost(post, (d) => ({ ...d, banned_media: d.banned_media.filter((h) => !remove.has(toHex(h))) }))
    }
    case 'SetReaction':
      return applyReaction(post, ev.payload.old ?? null, ev.payload.reaction_hash)
    case 'Vote':
      return applyVote(post, ev.payload.options ?? [])
    case 'SetBanned':
      return mapPost(post, (d) => ({ ...d, banned: ev.payload.banned ?? null }))
    case 'SetModNote':
      return mapPost(post, (d) => ({ ...d, mod_note: ev.payload.mod_note ?? null }))
    default:
      return post
  }
}
