import type { PostObject } from '../bcs/types'
import type { BoardViewData, ThreadViewData } from '../load'
import { decodeRealtimeEvent, type DecodedEvent, type Ns } from './core'
import { applyPost } from './post_apply'
import { applyThread } from './thread_apply'
import { applyBoard } from './board_apply'
import { toHex } from '../intent/crypto'

export interface BatchEvent {
  kind: Ns
  id: string
  counter: number
  event: string
}

export interface Batch {
  forum: string | null
  board: string | null
  thread: string | null
  post: string | null
  events: BatchEvent[]
}

export interface ApplyResult<T> {
  data: T
  reload: boolean
}

export function snapshotThread(d: ThreadViewData): Map<string, number> {
  const m = new Map<string, number>()
  m.set(toHex(d.thread.root.id), Number(d.thread.root.entity.feed.counter))
  for (const p of d.posts) m.set(toHex(p.root.id), Number(p.root.entity.feed.counter))
  return m
}

export function snapshotBoard(d: BoardViewData): Map<string, number> {
  const m = new Map<string, number>()
  for (const t of d.threads) m.set(toHex(t.root.id), Number(t.root.entity.feed.counter))
  for (const p of d.opPosts.values()) m.set(toHex(p.root.id), Number(p.root.entity.feed.counter))
  for (const list of d.last3.values()) for (const p of list) m.set(toHex(p.root.id), Number(p.root.entity.feed.counter))
  return m
}

function freshEvent(env: BatchEvent): DecodedEvent | null {
  try {
    return decodeRealtimeEvent(env.kind, env.event)
  } catch {
    return null
  }
}

export function applyThreadBatch(d: ThreadViewData, batch: Batch, applied: Map<string, number>): ApplyResult<ThreadViewData> {
  let data = d
  let reload = false
  const index = new Map<string, number>()
  for (let i = 0; i < data.posts.length; i++) index.set(toHex(data.posts[i].root.id), i)
  const replacePost = (id: string, next: PostObject) => {
    const i = index.get(id)
    if (i === undefined) return
    const posts = data.posts.slice()
    posts[i] = next
    data = { ...data, posts }
  }
  for (const env of batch.events) {
    if (env.counter <= (applied.get(env.id) ?? 0)) continue
    applied.set(env.id, env.counter)
    const ev = freshEvent(env)
    if (!ev) continue
    if (env.kind === 'post') {
      if (ev.$kind === 'Genesis') {
        if (!index.has(env.id)) reload = true
        continue
      }
      const i = index.get(env.id)
      if (i === undefined) continue
      replacePost(env.id, applyPost(data.posts[i], ev))
      continue
    }
    if (env.kind === 'thread' && env.id === data.threadUid) {
      data = { ...data, thread: applyThread(data.thread, ev) }
    }
  }
  return { data, reload }
}

export function applyBoardBatch(d: BoardViewData, batch: Batch, applied: Map<string, number>): ApplyResult<BoardViewData> {
  let data = d
  let reload = false
  const threadIndex = new Map<string, number>()
  for (let i = 0; i < data.threads.length; i++) threadIndex.set(toHex(data.threads[i].root.id), i)
  for (const env of batch.events) {
    if (env.counter <= (applied.get(env.id) ?? 0)) continue
    applied.set(env.id, env.counter)
    const ev = freshEvent(env)
    if (!ev) continue
    if (env.kind === 'thread') {
      if (ev.$kind === 'Genesis') {
        if (!threadIndex.has(env.id)) reload = true
        continue
      }
      const i = threadIndex.get(env.id)
      if (i === undefined) continue
      const threads = data.threads.slice()
      threads[i] = applyThread(threads[i], ev)
      data = { ...data, threads }
      continue
    }
    if (env.kind === 'post') {
      if (ev.$kind === 'Genesis') {
        const known = data.opPosts.has(env.id) || [...data.last3.values()].some((list) => list.some((p) => toHex(p.root.id) === env.id))
        if (!known) {
          const threadHex = batch.thread
          const belongsHere = threadHex !== null && threadIndex.has(threadHex)
          if (belongsHere) reload = true
        }
        continue
      }
      const opPosts = new Map(data.opPosts)
      let touched = false
      for (const [key, post] of opPosts) {
        if (toHex(post.root.id) !== env.id) continue
        opPosts.set(key, applyPost(post, ev))
        touched = true
      }
      const last3 = new Map<string, PostObject[]>()
      for (const [key, list] of data.last3) {
        if (!list.some((p) => toHex(p.root.id) === env.id)) {
          last3.set(key, list)
          continue
        }
        last3.set(key, list.map((p) => (toHex(p.root.id) === env.id ? applyPost(p, ev) : p)))
        touched = true
      }
      if (touched) data = { ...data, opPosts, last3 }
      continue
    }
    if (env.kind === 'board' && env.id === toHex(data.board.root.id)) {
      data = { ...data, board: applyBoard(data.board, ev) }
    }
  }
  return { data, reload }
}
