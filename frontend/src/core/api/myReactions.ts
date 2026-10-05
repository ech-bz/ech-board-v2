import { fetchReactions } from './relay'
import { getSecretKeyBytes } from '../keys/storage'
import { computeTweak, derivePostKeypair } from '../intent/crypto'
import { PostProjection, type PostObject } from '../bcs/types'

export async function fetchMyReactions(relayUrl: string, posts: PostObject[]): Promise<Map<string, string>> {
  const masterSecret = getSecretKeyBytes()
  const pairs: [Uint8Array, Uint8Array][] = []
  for (const p of posts) {
    const pp = new PostProjection(p.projection)
    if (pp.reactions().length > 0) {
      pairs.push([p.root.id, derivePostKeypair(masterSecret, computeTweak(p.root.id)).publicKey])
    }
  }
  const my = new Map<string, string>()
  if (pairs.length === 0) return my
  try {
    for (const [pid, r] of await fetchReactions(relayUrl, pairs)) {
      my.set(pid, r)
    }
  } catch {}
  return my
}
