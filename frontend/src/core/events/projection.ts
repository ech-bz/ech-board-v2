import type {
  PostObject, ThreadObject, BoardObject, ForumObject,
  PostProjectionData, ThreadProjectionData, BoardProjectionData, ForumProjectionData,
} from '../bcs/types'

export function mapThread(thread: ThreadObject, mutate: (d: ThreadProjectionData) => ThreadProjectionData): ThreadObject {
  return { ...thread, projection: mutate(thread.projection) }
}

export function mapPost(post: PostObject, mutate: (d: PostProjectionData) => PostProjectionData): PostObject {
  return { ...post, projection: mutate(post.projection) }
}

export function mapBoard(board: BoardObject, mutate: (d: BoardProjectionData) => BoardProjectionData): BoardObject {
  return { ...board, projection: mutate(board.projection) }
}

export function mapForum(forum: ForumObject, mutate: (d: ForumProjectionData) => ForumProjectionData): ForumObject {
  return { ...forum, projection: mutate(forum.projection) }
}
