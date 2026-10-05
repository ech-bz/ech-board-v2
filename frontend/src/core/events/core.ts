import { fromHex, toHex } from '../intent/crypto'
import { BoardEventSpec as BoardEventSchema, ForumEvent as ForumEventSchema, PostEventSpec as PostEventSchema, ThreadEventSpec as ThreadEventSchema } from '../bcs/relay'

export type Ns = 'forum' | 'board' | 'thread' | 'post'

export interface CallIds {
  board?: Uint8Array
  thread?: Uint8Array
  post?: Uint8Array
}

export interface CallDesc {
  fn: string
  args: (ids: CallIds) => Uint8Array
}

export function call(fn: string, args: (ids: CallIds) => Uint8Array): CallDesc {
  return { fn, args }
}

export interface DecodedEvent {
  $kind: string
  payload: Record<string, any>
}

const schemas: Record<Ns, { parse: (bytes: Uint8Array) => any }> = {
  forum: { parse: (bytes) => ForumEventSchema.parse(bytes) },
  board: { parse: (bytes) => BoardEventSchema.parse(bytes) },
  thread: { parse: (bytes) => ThreadEventSchema.parse(bytes) },
  post: { parse: (bytes) => PostEventSchema.parse(bytes) },
}

export function parseEvent(ns: Ns, bytes: Uint8Array): DecodedEvent {
  const parsed = schemas[ns].parse(bytes) as Record<string, unknown>
  const variant = Object.keys(parsed)[0]
  const payload = (parsed as Record<string, any>)[variant]
  return { $kind: variant, payload: payload ?? {} }
}

export function decodeRealtimeEvent(ns: Ns, bytesHex: string): DecodedEvent {
  return parseEvent(ns, fromHex(bytesHex))
}

export function hexOf(bytes: Uint8Array): string {
  return toHex(bytes)
}
