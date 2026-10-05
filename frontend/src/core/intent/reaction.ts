import { createIntentBuilder } from './IntentBuilder'
import { PostEvent } from '../events/post_apply'
import { sendIntents } from '../api/relay'
import { fromHex, computeTweak } from './crypto'
import type { PostObject } from '../bcs/types'

export interface ReactionCtx {
  relayUrl: string
  rootKey: string
  forumId: string
  programHash: string
  boardUid: string
}

export async function sendReactionIntent(ctx: ReactionCtx, threadUid: string, post: PostObject, old: Uint8Array | null, next: Uint8Array): Promise<void> {
  const builder = createIntentBuilder({ rootKey: ctx.rootKey, forumId: ctx.forumId, programHash: ctx.programHash, relayUrl: ctx.relayUrl })
    .event(PostEvent.setReactionV2(old, next))
    .board(fromHex(ctx.boardUid))
    .thread(fromHex(threadUid))
    .post(post.root.id)
    .rawTweak(computeTweak(post.root.id))
  const intent = await builder.build()
  await sendIntents(ctx.relayUrl, [{ intent }])
}
