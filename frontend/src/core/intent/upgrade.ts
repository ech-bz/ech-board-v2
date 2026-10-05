import { BoardObject, ThreadObject, PostObject, ForumObject } from '../bcs/chain'

export type EntityNs = 'forum' | 'board' | 'thread' | 'post'

export const CURRENT_ENTITY_VERSION: Record<EntityNs, number> = {
  forum: 1,
  board: 1,
  thread: 1,
  post: 1,
}

export function entityNs(objectType: unknown): EntityNs {
  if (objectType === BoardObject) return 'board'
  if (objectType === ThreadObject) return 'thread'
  if (objectType === PostObject) return 'post'
  if (objectType === ForumObject) return 'forum'
  return 'forum'
}
