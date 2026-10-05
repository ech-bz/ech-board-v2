import { PostProjection, ThreadProjection } from '../core/bcs/types'
import { detectRoles, matchesAuthor, type RoleOption } from '../core/intent/roles'
import { toHex } from '../core/intent/crypto'
import { getSecretKeyBytes } from '../core/keys/storage'
import type { BoardViewData, ThreadViewData } from '../core/load'

export function detectViewRoles(data: ThreadViewData | BoardViewData): RoleOption[] {
  const master = getSecretKeyBytes()
  const base = {
    master,
    forumIdHex: data.forumId,
    boardUidHex: data.boardUid,
    moderators: data.moderators,
  }
  if ('threadUid' in data) {
    const opUid = toHex(new ThreadProjection(data.thread.projection).op())
    const opPost = data.posts.find((post) => toHex(post.root.id) === opUid)
    const pp = opPost ? new PostProjection(opPost.projection) : null
    const op =
      opPost && pp && matchesAuthor(master, pp.sender().pk, pp.sender().tweak)
        ? { tweak: pp.sender().tweak, pk: pp.sender().pk }
        : null
    return detectRoles({ ...base, threadUidHex: data.threadUid, op })
  }
  return detectRoles(base)
}
